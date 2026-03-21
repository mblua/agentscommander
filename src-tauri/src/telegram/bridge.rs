use std::io::Write as IoWrite;
use std::sync::{Arc, Mutex};

use tauri::Emitter;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::pty::manager::PtyManager;
use crate::telegram::api;
use crate::telegram::types::BridgeInfo;

// ── File logger ──────────────────────────────────────────────

struct BridgeLogger {
    file: Option<std::fs::File>,
}

impl BridgeLogger {
    fn new(session_id: &str) -> Self {
        let file = dirs::home_dir()
            .map(|h| h.join(".summongate"))
            .and_then(|dir| {
                std::fs::create_dir_all(&dir).ok()?;
                let path = dir.join("telegram-bridge.log");
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .ok()
            });

        if let Some(ref f) = file {
            let path = f.metadata().ok();
            log::info!(
                "Bridge logger active for session {} ({} bytes)",
                session_id,
                path.map(|m| m.len()).unwrap_or(0)
            );
        }

        Self { file }
    }

    fn log(&mut self, direction: &str, session_id: &str, text: &str) {
        if let Some(ref mut f) = self.file {
            let now = chrono::Utc::now().format("%H:%M:%S%.3f");
            let preview = if text.len() > 500 {
                let mut end = 500;
                while !text.is_char_boundary(end) { end -= 1; }
                format!("{}...[{}b total]", &text[..end], text.len())
            } else {
                text.to_string()
            };
            let _ = writeln!(f, "[{}] {} sid={} | {}", now, direction, session_id, preview);
            let _ = f.flush();
        }
    }
}

// ── Diagnostic logger (full capture, no truncation) ─────────

struct DiagLogger {
    raw_file: Option<std::fs::File>,
    sent_file: Option<std::fs::File>,
}

impl DiagLogger {
    fn new() -> Self {
        let dir = dirs::home_dir().map(|h| h.join(".summongate"));

        let open = |name: &str| -> Option<std::fs::File> {
            let dir = dir.as_ref()?;
            std::fs::create_dir_all(dir).ok()?;
            let path = dir.join(name);
            // Truncate on each bridge start so we get a clean capture
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .ok()
        };

        let raw_file = open("diag-raw.log");
        let sent_file = open("diag-sent.log");

        if raw_file.is_some() && sent_file.is_some() {
            log::info!("Diagnostic logger active: ~/.summongate/diag-raw.log + diag-sent.log");
        }

        Self { raw_file, sent_file }
    }

    fn log_raw(&mut self, text: &str) {
        if let Some(ref mut f) = self.raw_file {
            let now = chrono::Utc::now().format("%H:%M:%S%.3f");
            let _ = writeln!(f, "--- [{}] ---", now);
            let _ = writeln!(f, "{}", text);
            let _ = f.flush();
        }
    }

    fn log_sent(&mut self, text: &str) {
        if let Some(ref mut f) = self.sent_file {
            let now = chrono::Utc::now().format("%H:%M:%S%.3f");
            let _ = writeln!(f, "--- [{}] ---", now);
            let _ = writeln!(f, "{}", text);
            let _ = f.flush();
        }
    }
}

// ── Bridge spawn ─────────────────────────────────────────────

pub struct BridgeHandle {
    pub info: BridgeInfo,
    pub cancel: CancellationToken,
    pub output_sender: mpsc::Sender<Vec<u8>>,
}

pub fn spawn_bridge(
    bot_token: String,
    chat_id: i64,
    session_id: Uuid,
    info: BridgeInfo,
    pty_mgr: Arc<Mutex<PtyManager>>,
    app_handle: tauri::AppHandle,
) -> BridgeHandle {
    let cancel = CancellationToken::new();
    let (tx, rx) = mpsc::channel::<Vec<u8>>(256);

    let session_id_str = session_id.to_string();

    // Output task: PTY bytes → strip ANSI → filter → buffer → Telegram
    tokio::spawn(output_task(
        rx,
        bot_token.clone(),
        chat_id,
        session_id_str.clone(),
        cancel.clone(),
        app_handle.clone(),
    ));

    // Poll task: Telegram getUpdates → write to PTY stdin
    tokio::spawn(poll_task(
        bot_token,
        chat_id,
        session_id,
        session_id_str,
        pty_mgr,
        cancel.clone(),
        app_handle,
    ));

    BridgeHandle {
        info,
        cancel,
        output_sender: tx,
    }
}

// ── Output task (PTY → Telegram) ─────────────────────────────

async fn output_task(
    mut rx: mpsc::Receiver<Vec<u8>>,
    token: String,
    chat_id: i64,
    session_id: String,
    cancel: CancellationToken,
    app: tauri::AppHandle,
) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    let mut logger = BridgeLogger::new(&session_id);
    let mut diag = DiagLogger::new();
    let mut buffer = String::new();
    let far_future = tokio::time::Duration::from_secs(86400);
    let flush_timeout = tokio::time::Duration::from_millis(500);
    let mut deadline = tokio::time::Instant::now() + far_future;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep_until(deadline) => {
                if !buffer.is_empty() {
                    flush_buffer(&mut buffer, &client, &token, chat_id, &session_id, &app, &mut logger, &mut diag).await;
                }
                deadline = tokio::time::Instant::now() + far_future;
            }
            maybe_data = rx.recv() => {
                match maybe_data {
                    Some(data) => {
                        let stripped = strip_ansi_escapes::strip(&data);
                        let raw_text = String::from_utf8_lossy(&stripped);
                        logger.log("RAW_IN", &session_id, &raw_text);
                        diag.log_raw(&raw_text);

                        let cleaned = clean_terminal_output(&raw_text);
                        if !cleaned.is_empty() {
                            logger.log("FILTERED", &session_id, &cleaned);
                            buffer.push_str(&cleaned);
                            // Only reset deadline when we actually have new content
                            // Otherwise noise (thinking animations) keeps pushing
                            // the timeout forward and nothing ever flushes
                            deadline = tokio::time::Instant::now() + flush_timeout;
                        }

                        let meaningful_len = buffer.trim().len();
                        if meaningful_len > 2000 || (buffer.contains('\n') && meaningful_len >= 10) {
                            flush_buffer(&mut buffer, &client, &token, chat_id, &session_id, &app, &mut logger, &mut diag).await;
                            deadline = tokio::time::Instant::now() + far_future;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    // Final flush
    if !buffer.is_empty() {
        flush_buffer(&mut buffer, &client, &token, chat_id, &session_id, &app, &mut logger, &mut diag).await;
    }
}

// ── Filter ───────────────────────────────────────────────────

/// Claude Code TUI spinner/thinking characters
const CLAUDE_SPINNERS: &[char] = &['✻', '✶', '*', '✢', '·', '●', '✽'];

/// Words Claude Code uses for "thinking" status (randomized verbs)
const THINKING_VERBS: &[&str] = &[
    "Thinking",
    "Pontificating",
    "Discombobulating",
    "Bloviating",
    "Ruminating",
    "Cogitating",
    "Deliberating",
    "Contemplating",
    "Mulling",
    "Musing",
    "Meditating",
    "Reflecting",
    "Considering",
    "Pondering",
    "Reasoning",
    "Analyzing",
    "Processing",
    "Evaluating",
    "Assessing",
    "Garnishing",
    "Waiting",
];

/// Patterns that indicate Claude Code TUI chrome (status bar, prompts, etc.)
const TUI_CHROME_PATTERNS: &[&str] = &[
    "bypass permissions",
    "shift+tab to cycle",
    "ctrl+b to run in background",
    "/doctor for",
    "settings issue",
    "Tip: /mobile",
    "Tip: Use git",
    "Context \u{2591}",  // ░ progress bar
    "Usage \u{2588}",    // █ usage bar
    "(syncing...)",
    "(resets in",
    "Found 1 settings issue",
];

/// Check if a string is mostly Claude Code spinner/thinking noise.
/// These come through as concatenated spinner chars + thinking verbs
/// when ANSI escapes are stripped from screen redraws.
fn is_thinking_noise(s: &str) -> bool {
    // Check for thinking verb patterns anywhere in the string
    for verb in THINKING_VERBS {
        if s.contains(&format!("{}...", verb)) || s.contains(&format!("{}…", verb)) {
            return true;
        }
    }
    false
}

/// Check if a line is TUI chrome (status bar, mode indicator, etc.)
fn is_tui_chrome(s: &str) -> bool {
    for pattern in TUI_CHROME_PATTERNS {
        if s.contains(pattern) {
            return true;
        }
    }
    // Lines that are all box-drawing (─━═│┃ etc.)
    if s.len() > 10 && s.chars().all(|c| c == '─' || c == '━' || c == '═' || c == ' ' || c == '│' || c == '┃') {
        return true;
    }
    false
}

/// Detect screen redraws: long lines with high density of spinner chars
/// and mixed content that result from stripping ANSI from full TUI redraws.
fn is_screen_redraw(s: &str) -> bool {
    if s.len() < 200 {
        return false;
    }
    let total = s.chars().count();
    let spinner_count = s.chars().filter(|c| CLAUDE_SPINNERS.contains(c)).count();
    // If more than 5% of chars are spinner symbols, it is a redraw
    if spinner_count > 0 && (spinner_count as f32 / total as f32) > 0.05 {
        return true;
    }
    // Long lines with lots of repeated ─ are status bar redraws
    let dash_count = s.chars().filter(|&c| c == '─' || c == '━').count();
    if dash_count > 20 && (dash_count as f32 / total as f32) > 0.15 {
        return true;
    }
    false
}

/// Clean terminal output for Telegram consumption.
/// Handles carriage returns (inline overwrites), filters noise patterns
/// from coding agents (thinking indicators, progress bars, spinners).
fn clean_terminal_output(raw: &str) -> String {
    let mut result = Vec::new();

    for line in raw.split('\n') {
        // Simulate carriage return: keep only content after last \r
        // Terminal uses \r to overwrite the current line (spinners, progress)
        let line = if let Some(pos) = line.rfind('\r') {
            &line[pos + 1..]
        } else {
            line
        };

        let trimmed = line.trim();

        // Skip empty lines
        if trimmed.is_empty() {
            continue;
        }

        // Skip screen redraws (long concatenated TUI garbage)
        if is_screen_redraw(trimmed) {
            continue;
        }

        // Skip thinking/spinner noise
        if is_thinking_noise(trimmed) {
            continue;
        }

        // Skip TUI chrome (status bar, mode indicators, tips)
        if is_tui_chrome(trimmed) {
            continue;
        }

        // Skip lines that start with Claude spinner chars followed by whitespace
        // (standalone spinner frames like "✻ Pontificating…")
        let first_char = trimmed.chars().next().unwrap_or(' ');
        if CLAUDE_SPINNERS.contains(&first_char) {
            // Check if the rest is a thinking verb or empty
            let rest = trimmed[first_char.len_utf8()..].trim();
            if rest.is_empty() {
                continue;
            }
            for verb in THINKING_VERBS {
                if rest.starts_with(verb) {
                    continue;  // This continue only exits the inner for loop
                }
            }
            // Check: if it starts with spinner but rest is thinking, skip entire line
            let is_thinking_line = THINKING_VERBS.iter().any(|v| rest.starts_with(v));
            if is_thinking_line {
                continue;
            }
        }

        // Skip old-style thinking indicators
        if trimmed.contains("(thinking)") || trimmed.contains("⟡ thinking") {
            continue;
        }

        // Skip Braille spinners (⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏)
        if trimmed.starts_with(|c: char| ('\u{2800}'..='\u{28FF}').contains(&c)) {
            continue;
        }

        // Skip running hook notifications
        if trimmed.contains("(running stop hook") || trimmed.contains("(running start hook") {
            continue;
        }

        // Skip lines that are mostly box-drawing / progress bar / special chars
        // (less than 30% alphanumeric content)
        let total: usize = trimmed.chars().count();
        if total > 5 {
            let alnum: usize = trimmed
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == ' ')
                .count();
            if (alnum as f32 / total as f32) < 0.30 {
                continue;
            }
        }

        // Skip lines that are just a prompt marker (❯ alone or with whitespace)
        if trimmed == "❯" || trimmed == ">" {
            continue;
        }

        result.push(line.to_string());
    }

    result.join("\n")
}

// ── Flush to Telegram ────────────────────────────────────────

async fn flush_buffer(
    buffer: &mut String,
    client: &reqwest::Client,
    token: &str,
    chat_id: i64,
    session_id: &str,
    app: &tauri::AppHandle,
    logger: &mut BridgeLogger,
    diag: &mut DiagLogger,
) {
    let text = std::mem::take(buffer);
    // Deduplicate consecutive identical lines
    let mut lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if lines.last().map(|l: &&str| l.trim()) != Some(trimmed) {
            lines.push(line);
        }
    }
    let text = lines.join("\n");
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }

    for chunk in chunk_text(&text, 4000) {
        logger.log("SEND_TG", session_id, &chunk);
        diag.log_sent(&chunk);

        if let Err(e) = api::send_message(client, token, chat_id, &chunk).await {
            logger.log("SEND_ERR", session_id, &e.to_string());
            log::error!("Telegram send error for session {}: {}", session_id, e);
            let _ = app.emit(
                "telegram_bridge_error",
                serde_json::json!({
                    "sessionId": session_id,
                    "error": e.to_string(),
                }),
            );
        }
        // Rate limit: 35ms between sends
        tokio::time::sleep(tokio::time::Duration::from_millis(35)).await;
    }
}

fn chunk_text(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let end = (start + max_len).min(text.len());
        let actual_end = if end < text.len() {
            text[start..end]
                .rfind('\n')
                .map(|i| start + i + 1)
                .unwrap_or(end)
        } else {
            end
        };
        chunks.push(text[start..actual_end].to_string());
        start = actual_end;
    }
    chunks
}

// ── Poll task (Telegram → PTY) ───────────────────────────────

async fn poll_task(
    token: String,
    chat_id: i64,
    session_id: Uuid,
    session_id_str: String,
    pty_mgr: Arc<Mutex<PtyManager>>,
    cancel: CancellationToken,
    app: tauri::AppHandle,
) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    let mut logger = BridgeLogger::new(&session_id_str);
    let mut offset: i64 = 0;

    // Skip old messages
    match api::get_updates(&client, &token, 0, 0).await {
        Ok(updates) => {
            if let Some(last) = updates.last() {
                offset = last.update_id + 1;
                logger.log("POLL_INIT", &session_id_str, &format!("skipped {} old messages, offset={}", updates.len(), offset));
            }
        }
        Err(e) => {
            logger.log("POLL_ERR", &session_id_str, &e.to_string());
            log::warn!("Initial getUpdates failed: {}", e);
        }
    }

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            result = api::get_updates(&client, &token, offset, 5) => {
                match result {
                    Ok(updates) => {
                        for update in updates {
                            offset = update.update_id + 1;

                            // Only process messages from the target chat
                            if update.chat_id != chat_id {
                                logger.log("POLL_SKIP", &session_id_str, &format!("wrong chat_id={} from={}", update.chat_id, update.from_name));
                                continue;
                            }

                            logger.log("RECV_TG", &session_id_str, &format!("from={} text={}", update.from_name, update.text));

                            // Write to PTY stdin — use \r (carriage return) not \n
                            // Terminals send \r when Enter is pressed
                            let input = format!("{}\r", update.text);
                            if let Ok(mgr) = pty_mgr.lock() {
                                if let Err(e) = mgr.write(session_id, input.as_bytes()) {
                                    logger.log("PTY_ERR", &session_id_str, &e.to_string());
                                    log::error!("Failed to write Telegram input to PTY: {}", e);
                                }
                            }

                            // Emit event for UI
                            let _ = app.emit(
                                "telegram_incoming",
                                serde_json::json!({
                                    "sessionId": session_id_str,
                                    "text": update.text,
                                    "from": update.from_name,
                                }),
                            );

                            // Update last-prompt display in terminal window
                            let _ = app.emit(
                                "last_prompt",
                                serde_json::json!({
                                    "text": format!("[TG] {}", update.text),
                                }),
                            );
                        }
                    }
                    Err(e) => {
                        logger.log("POLL_ERR", &session_id_str, &e.to_string());
                        log::error!("Telegram poll error: {}", e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                    }
                }
            }
        }
    }
}
