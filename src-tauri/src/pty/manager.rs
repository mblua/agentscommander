use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::errors::AppError;
use crate::pty::git_watcher::GitWatcher;
use crate::pty::idle_detector::IdleDetector;
use crate::telegram::manager::OutputSenderMap;

struct PtyInstance {
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
}

pub struct PtyManager {
    ptys: Arc<Mutex<HashMap<Uuid, PtyInstance>>>,
    output_senders: OutputSenderMap,
    idle_detector: Arc<IdleDetector>,
    git_watcher: Arc<GitWatcher>,
    /// Optional WS broadcaster for remote access
    ws_broadcaster: Option<crate::web::broadcast::WsBroadcaster>,
    /// VT100 screen state per session for replay to late-joining WS clients
    screen_parsers: Arc<Mutex<HashMap<Uuid, vt100::Parser>>>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PtyOutputPayload {
    session_id: String,
    data: Vec<u8>,
}

/// Strip ANSI escape sequences so that marker detection is not fooled
/// by terminal color/cursor codes. Handles:
/// - CSI sequences: ESC [ ... final_byte (colors, cursor, SGR)
/// - OSC sequences: ESC ] ... BEL/ST (title, hyperlinks, shell integration)
/// - DCS sequences: ESC P ... ST (device control strings)
/// - Non-CSI two-byte escapes: ESC + one byte (resets, keypad mode)
fn strip_ansi_csi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some(&'[') => {
                    chars.next(); // skip '['
                                  // CSI: skip parameter/intermediate bytes until final byte (0x40..=0x7E)
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(&']') => {
                    chars.next(); // skip ']'
                                  // OSC: consume until BEL (\x07) or ST (ESC \)
                    while let Some(&ch) = chars.peek() {
                        if ch == '\x07' {
                            chars.next();
                            break;
                        }
                        if ch == '\x1b' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                                break; // proper ST terminator
                            }
                            // ESC not followed by \ — not ST, keep consuming
                            continue;
                        }
                        chars.next();
                    }
                }
                Some(&'P') => {
                    chars.next(); // skip 'P'
                                  // DCS: consume until ST (ESC \)
                    while let Some(&ch) = chars.peek() {
                        if ch == '\x1b' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                                break; // proper ST terminator
                            }
                            // ESC not followed by \ — not ST, keep consuming
                            continue;
                        }
                        chars.next();
                    }
                }
                Some(_) => {
                    // Non-CSI two-byte escape (e.g. ESC c, ESC M)
                    chars.next();
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

impl PtyManager {
    pub fn new(
        output_senders: OutputSenderMap,
        idle_detector: Arc<IdleDetector>,
        git_watcher: Arc<GitWatcher>,
        ws_broadcaster: Option<crate::web::broadcast::WsBroadcaster>,
    ) -> Self {
        Self {
            ptys: Arc::new(Mutex::new(HashMap::new())),
            output_senders,
            idle_detector,
            git_watcher,
            ws_broadcaster,
            screen_parsers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn spawn(
        &self,
        id: Uuid,
        cmd: &str,
        args: &[String],
        cwd: &str,
        cols: u16,
        rows: u16,
        app_handle: AppHandle,
    ) -> Result<(), AppError> {
        let pty_system = native_pty_system();

        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system
            .openpty(size)
            .map_err(|e| AppError::PtyError(e.to_string()))?;

        // On Windows, non-.exe commands (like .cmd, .bat, or bare names that
        // resolve to .cmd scripts) need to be wrapped with cmd.exe /C so the
        // shell can resolve them from PATH.
        let is_direct_exe = cmd.to_lowercase().ends_with(".exe")
            || std::path::Path::new(cmd)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));

        let mut command = if cfg!(windows) && !is_direct_exe {
            let mut c = CommandBuilder::new("cmd.exe");
            c.arg("/C");
            c.arg(cmd);
            for arg in args {
                c.arg(arg);
            }
            c
        } else {
            let mut c = CommandBuilder::new(cmd);
            for arg in args {
                c.arg(arg);
            }
            c
        };
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| AppError::PtyError(e.to_string()))?;

        // Drop the slave side — we only need the master
        drop(pair.slave);

        let writer = pair
            .master
            .take_writer()
            .map_err(|e| AppError::PtyError(e.to_string()))?;

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| AppError::PtyError(e.to_string()))?;

        let instance = PtyInstance {
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
            _child: child,
        };

        self.ptys.lock().unwrap().insert(id, instance);

        // Initialize vt100 screen parser for this session (for WS replay)
        {
            let parser = vt100::Parser::new(rows, cols, 0);
            self.screen_parsers.lock().unwrap().insert(id, parser);
        }

        // Spawn read loop that emits PTY output to the frontend and feeds active mirrors.
        let session_id_str = id.to_string();
        let output_senders = self.output_senders.clone();
        let idle_detector = Arc::clone(&self.idle_detector);
        let ws_broadcaster = self.ws_broadcaster.clone();
        let screen_parsers = Arc::clone(&self.screen_parsers);
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let data = buf[..n].to_vec();

                        let text = String::from_utf8_lossy(&data);
                        if text.contains('\u{FFFD}') {
                            log::debug!(
                                "[PTY] session {} chunk had invalid UTF-8 at buffer boundary ({} bytes, {} replacement chars)",
                                id, n, text.matches('\u{FFFD}').count()
                            );
                        }

                        // Record PTY activity for idle detection — but only if the
                        // output contains meaningful visible content. Terminal escape
                        // sequences (cursor moves, title updates, color resets, prompt
                        // redraws) are NOT user/agent activity and must not flip the
                        // session to busy. Strip ANSI escapes and check for printable
                        // characters above ASCII space.
                        let is_printable = |c: char| c > ' ' && c != '\u{FFFD}';
                        let has_printable = if text.contains('\x1b') {
                            strip_ansi_csi(&text).chars().any(is_printable)
                        } else {
                            text.chars().any(is_printable)
                        };
                        if has_printable {
                            idle_detector.record_activity_with_bytes(id, n);
                        } else {
                            log::info!(
                                "[idle] SKIPPED activity for {} ({} bytes, escape-only output)",
                                &id.to_string()[..8],
                                n
                            );
                        }

                        // Feed Telegram bridge if active (non-blocking)
                        if let Ok(senders) = output_senders.lock() {
                            if let Some(tx) = senders.get(&id) {
                                let _ = tx.try_send(data.clone());
                            }
                        }

                        // Feed vt100 screen parser for WS replay
                        if let Ok(mut parsers) = screen_parsers.lock() {
                            if let Some(parser) = parsers.get_mut(&id) {
                                parser.process(&data);
                            }
                        }

                        // Broadcast to WebSocket clients (non-blocking)
                        if let Some(ref bc) = ws_broadcaster {
                            bc.broadcast_pty_output(&session_id_str, &data);
                        }

                        let payload = PtyOutputPayload {
                            session_id: session_id_str.clone(),
                            data,
                        };
                        let _ = app_handle.emit("pty_output", payload);
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(())
    }

    pub fn write(&self, id: Uuid, data: &[u8]) -> Result<(), AppError> {
        let ptys = self.ptys.lock().unwrap();
        let instance = ptys
            .get(&id)
            .ok_or_else(|| AppError::SessionNotFound(id.to_string()))?;

        let mut writer = instance.writer.lock().unwrap();
        writer
            .write_all(data)
            .map_err(|e| AppError::PtyError(e.to_string()))?;
        writer
            .flush()
            .map_err(|e| AppError::PtyError(e.to_string()))?;

        Ok(())
    }

    pub fn resize(&self, id: Uuid, cols: u16, rows: u16) -> Result<(), AppError> {
        // Tell idle detector to ignore PTY output caused by this resize
        self.idle_detector.record_resize(id);

        let ptys = self.ptys.lock().unwrap();
        let instance = ptys
            .get(&id)
            .ok_or_else(|| AppError::SessionNotFound(id.to_string()))?;

        let master = instance.master.lock().unwrap();
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| AppError::PtyError(e.to_string()))?;

        // Keep the vt100 screen parser in sync so snapshots match the new size
        if let Ok(mut parsers) = self.screen_parsers.lock() {
            if let Some(parser) = parsers.get_mut(&id) {
                parser.set_size(rows, cols);
            }
        }

        // Broadcast resize to WS clients so browser mirrors can update dimensions
        if let Some(ref bc) = self.ws_broadcaster {
            bc.broadcast_event(
                "pty_resized",
                &serde_json::json!({
                    "sessionId": id.to_string(),
                    "cols": cols,
                    "rows": rows,
                }),
            );
        }

        Ok(())
    }

    pub fn kill(&self, id: Uuid) -> Result<(), AppError> {
        let mut ptys = self.ptys.lock().unwrap();
        // Dropping the PtyInstance will close the master, which signals the child
        ptys.remove(&id);
        self.idle_detector.remove_session(id);
        self.git_watcher.remove_session(id);

        // Clean up vt100 screen parser
        if let Ok(mut parsers) = self.screen_parsers.lock() {
            parsers.remove(&id);
        }

        Ok(())
    }

    /// Get a screen snapshot for replay to late-joining WS clients.
    /// Returns the visible screen content as raw bytes that can be written to xterm.js.
    pub fn get_screen_snapshot(&self, id: Uuid) -> Option<Vec<u8>> {
        let parsers = self.screen_parsers.lock().ok()?;
        let parser = parsers.get(&id)?;
        let screen = parser.screen();
        Some(screen.contents_formatted())
    }

    /// Get the current PTY dimensions (rows, cols) from the vt100 parser.
    pub fn get_pty_size(&self, id: Uuid) -> Option<(u16, u16)> {
        let parsers = self.screen_parsers.lock().ok()?;
        let parser = parsers.get(&id)?;
        Some(parser.screen().size())
    }
}
