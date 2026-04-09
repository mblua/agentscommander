# Plan: Telegram JSONL Watcher for Claude Code Sessions

**Branch:** `feature/telegram-jsonl-watcher`
**Status:** AWAITING ARCHITECT REVIEW

---

## Problem

The current Telegram bridge captures PTY output via a 6-phase pipeline:

```
PTY bytes -> vt100::Parser -> 800ms stabilization -> AgentFilter -> Buffer -> Telegram
```

This is extremely complex and still produces noisy output because the source is raw terminal bytes full of ANSI escapes, cursor movement, spinner animations, TUI chrome, and box-drawing characters.

## Discovery

Claude Code writes **clean structured session logs** as JSONL files:

- **Location:** `~/.claude/projects/{mangled-cwd}/<session-id>.jsonl`
- **Path mangling:** Replace non-alphanumeric, non-hyphen chars with `-` (already implemented in `commands/session.rs:72-74`)
- **Format:** One JSON object per line with structured message data:

```json
{"type": "permission-mode", "permissionMode": "bypassPermissions", "sessionId": "..."}
{"type": "user", "message": {"role": "user", "content": "..."}}
{"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "clean model output"}, {"type": "tool_use", "name": "Bash"}]}}
{"type": "summary", ...}
```

The `message.content` field for assistant messages contains **clean text without any ANSI codes or terminal artifacts** -- exactly what we want to send to Telegram.

## Requirement

Build a **JSONL file watcher** as an **alternative output source** for the Telegram bridge, specifically for Claude Code sessions. When a session is detected as Claude Code, the bridge should watch the JSONL session file instead of using the PTY-based pipeline.

### Must Have

1. **JSONL Watcher module** (`src-tauri/src/telegram/jsonl_watcher.rs` or similar):
   - Watch the session's JSONL file for new lines appended
   - Parse each new line, extract `role: "assistant"` messages
   - Extract text content from `message.content` (handle both `string` and `[{type: "text", text: "..."}]` formats)
   - Skip tool_use blocks, tool_result blocks, system messages, permission-mode entries
   - Feed extracted text to the existing buffer/send pipeline (Phase 5-6 of current bridge)

2. **Session JSONL path resolution:**
   - Reuse the existing mangling logic from `commands/session.rs:72-74`
   - Find the most recently modified `.jsonl` file in the project directory
   - The session file changes when Claude Code restarts, so must handle file rotation

3. **Integration with existing bridge:**
   - When `spawn_bridge()` is called and the session is detected as Claude Code:
     - Spawn the JSONL watcher task **instead of** the PTY output_task
     - The poll_task (Telegram -> PTY input) remains unchanged
   - When the session is NOT Claude Code, fall back to the existing PTY pipeline
   - The `BridgeHandle`, `BridgeInfo`, `TelegramBridgeManager` interfaces should NOT change

4. **Detection of Claude Code session:**
   - The `is_claude` flag already exists in `commands/session.rs:65`
   - Need to propagate this flag to the session metadata so the bridge can check it at attach time
   - Alternatively, check if the JSONL project dir exists at attach time

5. **File watching strategy:**
   - Use `notify` crate (already common in Rust ecosystem) OR simple polling (e.g., poll every 500ms, check file size, read new bytes)
   - Given that Claude Code writes infrequently (only on message completion), **polling is acceptable and simpler**
   - Track file position (byte offset) to only read new content

### Nice to Have (NOT in scope for v1)

- Watching multiple JSONL files if Claude starts a new session (file rotation)
- Sending user messages to Telegram (only assistant for now)
- Support for other coding agents (Codex, Cursor, etc.) -- this is explicitly deferred

### Architecture Constraint

The JSONL watcher must be **well-isolated** as a self-contained module. The intent is to later add similar watchers for other coding agents that have their own log formats. Think of it as:

```
telegram/
  bridge.rs          -- existing, orchestrates which output source to use
  jsonl_watcher.rs   -- NEW: Claude Code JSONL file watcher
  manager.rs         -- existing, unchanged interface
  ...
```

Future agents would add their own watcher modules (e.g., `codex_watcher.rs`) with the same output interface.

## Existing Code References

- **Path mangling:** `src-tauri/src/commands/session.rs:72-74`
- **Claude detection:** `src-tauri/src/commands/session.rs:65`
- **Bridge spawn:** `src-tauri/src/telegram/bridge.rs:413-452`
- **Output task (to be replaced for Claude):** `src-tauri/src/telegram/bridge.rs:470-499`
- **Buffer + send phases:** reuse from existing bridge.rs (Phase 5-6)
- **AgentFilter trait:** already exists in bridge.rs, may not be needed for JSONL (text is already clean)
- **Telegram API send:** `src-tauri/src/telegram/api.rs`

## JSONL Parsing Details

Each line is independent JSON. Relevant message types:

| type | role | action |
|------|------|--------|
| `"user"` | `"user"` | SKIP (v1) |
| `"assistant"` | `"assistant"` | EXTRACT text, send to Telegram |
| `"permission-mode"` | - | SKIP |
| `"summary"` | - | SKIP |

For `message.content`:
- If `string`: use directly
- If `array`: iterate, collect all `{type: "text", text: "..."}` blocks, join with newline
- Skip `{type: "tool_use", ...}` and `{type: "tool_result", ...}` blocks

## Open Questions for Architect

1. Should the watcher trait be generic from the start (e.g., `trait SessionLogWatcher`) or just a concrete struct for now?
2. Should we add a `notify` crate dependency or use simple polling? The JSONL is written infrequently.
3. Where should the `is_claude` flag be stored in session metadata? Currently it's a local variable in `create_session`. Options: add to `Session` struct, or re-derive at attach time from the session's CWD.

---

## Architect Design

**Author:** Architect Agent
**Date:** 2025-04-09

### Open Questions — Decisions

**Q1: Trait vs concrete struct?**
**Decision: Concrete struct, no trait for v1.** The plan's architecture constraint ("well-isolated, self-contained module") is satisfied by a standalone `jsonl_watcher.rs` module with a clean public API. A `trait SessionLogWatcher` adds coupling without a second implementation to validate it against. When `codex_watcher.rs` arrives, extracting a trait from two concrete implementations is trivial and will produce a better abstraction than one designed speculatively.

**Q2: `notify` crate vs polling?**
**Decision: Simple polling at 500ms.** Reasons:
- Claude Code writes JSONL infrequently (one line per completed message, not character-by-character)
- 500ms poll matches the existing `FLUSH_DELAY_MS` and is plenty responsive for Telegram delivery
- The file is append-only — polling is just: compare file size vs last offset, read delta, split by `\n`
- `notify` on Windows has known quirks (double events, file-lock interactions, missed events on some filesystems)
- Zero new dependencies

**Q3: Where to store `is_claude`?**
**Decision: Add `is_claude: bool` to the `Session` struct** in `session/session.rs`, with `#[serde(default)]` for backward compatibility with persisted sessions. Rationale:
- Already computed at creation time (`commands/session.rs:65`) — just persist the result
- Re-deriving from `shell`/`shell_args` at attach time is fragile (args get mutated with `--continue`, `--append-system-prompt-file`, etc.)
- The bridge attach path needs this flag, and the Session struct is the canonical place for session metadata
- `#[serde(default)]` means existing persisted sessions default to `false` (PTY mode) — correct behavior

---

### Architecture Overview

```
CURRENT (PTY pipeline):
  PTY bytes → output_senders[session_id] → rx channel → output_task (6 phases) → Telegram

NEW (JSONL mode, Claude Code sessions only):
  ~/.claude/projects/{mangled_cwd}/*.jsonl → poll → parse → buffer → Telegram
  (PTY bytes still flow to xterm.js, just not to Telegram)
```

The JSONL watcher **replaces** `output_task` (phases 1-4 are unnecessary — JSONL content is already clean text). It **reuses** the buffer/send logic (phases 5-6: `flush_buffer`, `chunk_text`).

---

### File Changes

#### 1. New file: `src-tauri/src/telegram/jsonl_watcher.rs`

Self-contained module. Public API:

```rust
/// Spawn a JSONL file watcher task that polls for new assistant messages
/// and sends them to Telegram via the shared buffer/send pipeline.
pub fn spawn_watch_task(
    cwd: String,
    bot_token: String,
    chat_id: i64,
    session_id: String,
    cancel: CancellationToken,
    app: tauri::AppHandle,
) -> tokio::task::JoinHandle<()>
```

Internal components:

```rust
/// Mangle a CWD path the same way Claude Code does.
/// Reuses the same logic from commands/session.rs:72-74.
fn mangle_cwd(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect()
}

/// Find the most recently modified .jsonl file in the project directory.
fn find_latest_jsonl(project_dir: &Path) -> Option<PathBuf> {
    // Read dir, filter *.jsonl, sort by modified time desc, return first
}

/// Parse a single JSONL line and extract assistant text content.
/// Returns None for non-assistant messages, tool_use blocks, system messages, etc.
fn extract_assistant_text(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    // Only process type == "assistant"
    // Handle content as String or Array of {type: "text", text: "..."} blocks
    // Skip tool_use, tool_result blocks
}

/// Read new bytes from a file starting at the given byte offset.
/// Returns parsed lines and updates the offset.
fn read_new_lines(path: &Path, offset: &mut u64) -> io::Result<Vec<String>> {
    // Seek to offset, read to end, split by \n
    // Handle partial last line (keep remainder for next poll)
}
```

**Main loop structure:**

```rust
async fn watch_loop(...) {
    let project_dir = home_dir().join(".claude/projects").join(mangle_cwd(&cwd));
    let mut current_file: Option<PathBuf> = None;
    let mut file_offset: u64 = 0;
    let mut line_remainder = String::new();  // partial line buffer
    
    // Buffer/send state (same as output_task phases 5-6)
    let client = reqwest::Client::builder()...;
    let mut buffer = String::new();
    let mut last_buffer_add = Instant::now();
    let mut logger = BridgeLogger::new(&session_id);
    let mut diag = DiagLogger::new();
    
    let mut poll_interval = tokio::time::interval(Duration::from_millis(500));
    
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = poll_interval.tick() => {
                let latest = find_latest_jsonl(&project_dir);
                
                // Handle file rotation
                if latest != current_file {
                    if current_file.is_none() {
                        // First attach: skip existing content
                        file_offset = latest.as_ref()
                            .and_then(|p| std::fs::metadata(p).ok())
                            .map(|m| m.len())
                            .unwrap_or(0);
                    } else {
                        // File rotation (new Claude session): read from start
                        file_offset = 0;
                    }
                    current_file = latest;
                    line_remainder.clear();
                }
                
                if let Some(ref path) = current_file {
                    if let Ok(new_lines) = read_new_lines(path, &mut file_offset, &mut line_remainder) {
                        for line in new_lines {
                            if let Some(text) = extract_assistant_text(&line) {
                                logger.log("JSONL_EXTRACT", &session_id, &text);
                                buffer.push_str(&text);
                                buffer.push('\n');
                                last_buffer_add = Instant::now();
                            }
                        }
                    }
                }
                
                // Flush decision (shared logic with output_task)
                if !buffer.is_empty() {
                    let elapsed = last_buffer_add.elapsed();
                    if elapsed >= flush_delay || buffer.len() > 2000 {
                        flush_buffer(&mut buffer, &client, &token, chat_id,
                            &session_id, &app, &mut logger, &mut diag).await;
                    }
                }
            }
        }
    }
}
```

#### 2. Modified: `src-tauri/src/telegram/mod.rs`

Add module declaration:
```rust
pub mod jsonl_watcher;  // NEW
```

#### 3. Modified: `src-tauri/src/telegram/bridge.rs`

**Extract shared utilities** — make `flush_buffer`, `chunk_text`, `BridgeLogger`, and `DiagLogger` visible to sibling modules:

```rust
// Change from private to pub(super):
pub(super) struct BridgeLogger { ... }
pub(super) struct DiagLogger { ... }
pub(super) async fn flush_buffer(...) { ... }
pub(super) fn chunk_text(...) -> Vec<String> { ... }
```

**Modify `spawn_bridge` signature** — add `jsonl_cwd` parameter:

```rust
pub fn spawn_bridge(
    bot_token: String,
    chat_id: i64,
    session_id: Uuid,
    info: BridgeInfo,
    pty_mgr: Arc<Mutex<PtyManager>>,
    app_handle: tauri::AppHandle,
    jsonl_cwd: Option<String>,  // NEW: if Some, use JSONL watcher instead of PTY pipeline
) -> BridgeHandle {
    let cancel = CancellationToken::new();
    let (tx, rx) = mpsc::channel::<Vec<u8>>(256);
    let session_id_str = session_id.to_string();

    if let Some(cwd) = jsonl_cwd {
        // JSONL mode: watch Claude Code session log
        drop(rx);  // not needed — no PTY bytes feed
        super::jsonl_watcher::spawn_watch_task(
            cwd,
            bot_token.clone(),
            chat_id,
            session_id_str.clone(),
            cancel.clone(),
            app_handle.clone(),
        );
    } else {
        // PTY mode: existing 6-phase pipeline
        tokio::spawn(output_task(rx, bot_token.clone(), chat_id,
            session_id_str.clone(), cancel.clone(), app_handle.clone()));
    }

    // poll_task runs in BOTH modes (Telegram → PTY input is always needed)
    tokio::spawn(poll_task(bot_token, chat_id, session_id, session_id_str,
        pty_mgr, cancel.clone(), app_handle));

    BridgeHandle { info, cancel, output_sender: tx }
}
```

Note: `tx` is still created and stored in `BridgeHandle` even in JSONL mode. It's unused but harmless — keeps `BridgeHandle` unchanged.

#### 4. Modified: `src-tauri/src/telegram/manager.rs`

**Modify `attach` signature** — add `jsonl_cwd` and conditionally skip `output_senders` registration:

```rust
pub fn attach(
    &mut self,
    session_id: Uuid,
    bot: &TelegramBotConfig,
    pty_mgr: Arc<Mutex<PtyManager>>,
    app_handle: tauri::AppHandle,
    jsonl_cwd: Option<String>,  // NEW
) -> Result<BridgeInfo, AppError> {
    // ... existing validation unchanged ...

    let handle = bridge::spawn_bridge(
        bot.token.clone(), bot.chat_id, session_id,
        info.clone(), pty_mgr, app_handle,
        jsonl_cwd.clone(),  // NEW: pass through
    );

    // Only register output sender for PTY mode.
    // In JSONL mode, the watcher reads directly from file — no PTY byte feed needed.
    if jsonl_cwd.is_none() {
        if let Ok(mut senders) = self.output_senders.lock() {
            senders.insert(session_id, handle.output_sender.clone());
        }
    }

    // ... rest unchanged ...
}
```

#### 5. Modified: `src-tauri/src/session/session.rs`

Add `is_claude` field to `Session` struct:

```rust
pub struct Session {
    // ... existing fields ...
    
    /// True if this session runs Claude Code (detected at creation time).
    /// Used by the Telegram bridge to choose JSONL watcher vs PTY pipeline.
    #[serde(default)]
    pub is_claude: bool,
}
```

#### 6. Modified: `src-tauri/src/commands/session.rs`

At session creation (after `is_claude` is computed on line 65), set it on the session:

```rust
// After session is created, set the flag:
session.is_claude = is_claude;
```

At all `tg.attach(...)` call sites (lines 397, 575, 815), pass the CWD when session is Claude:

```rust
let jsonl_cwd = if session.is_claude {
    Some(session.working_directory.clone())
} else {
    None
};
tg.attach(id, &bot, pty_arc, app.clone(), jsonl_cwd)
```

#### 7. Modified: `src-tauri/src/commands/telegram.rs`

The `telegram_attach` command (line 34) needs to look up the session to check `is_claude`:

```rust
// Look up session to check is_claude flag
let mgr = session_mgr.read().await;
let session = mgr.get_session(uuid).ok_or("Session not found")?;
let jsonl_cwd = if session.is_claude {
    Some(session.working_directory.clone())
} else {
    None
};
drop(mgr);

let info = tg.attach(uuid, &bot, pty_arc, app.clone(), jsonl_cwd)
    .map_err(|e| e.to_string())?;
```

---

### Design Decisions — Details

#### Initial File Offset Strategy

When the JSONL watcher first starts (bridge attached to an existing session):
- **Seek to END of file** — skip historical messages. The user attached the bridge "now"; they want future messages, not a dump of the entire conversation so far.

When a file rotation occurs (Claude restarts, new session file):
- **Start from offset 0** — this is a new conversation; capture everything.

#### Partial Line Handling

JSONL files are written one complete line at a time by Claude Code, but the OS may buffer writes. The watcher must handle reading a partial line at the end of a file:
- `read_new_lines` keeps a `line_remainder: String` buffer
- Bytes read are appended to `line_remainder`, then split by `\n`
- Only complete lines (terminated by `\n`) are returned for parsing
- The unterminated tail stays in `line_remainder` for the next poll

#### No `output_senders` Registration for JSONL Mode

In PTY mode, the PTY read loop sends bytes to `output_senders[session_id]`, which feeds the bridge's `output_task`. In JSONL mode, the watcher reads directly from the filesystem — no byte channel needed. Skipping registration avoids PTY bytes being sent to a channel that nobody reads (the `rx` is dropped).

#### Logging

The JSONL watcher uses the same `BridgeLogger` and `DiagLogger` as `output_task`. Log tags:
- `JSONL_INIT` — watcher started, project dir path
- `JSONL_FILE` — current file path and offset
- `JSONL_ROTATE` — file rotation detected
- `JSONL_EXTRACT` — assistant text extracted (truncated in log)
- `JSONL_ERR` — file read or parse errors

---

### Files Summary

| File | Action | Description |
|------|--------|-------------|
| `telegram/jsonl_watcher.rs` | **CREATE** | New module: JSONL file watcher with polling, parsing, buffer/send |
| `telegram/mod.rs` | MODIFY | Add `pub mod jsonl_watcher;` |
| `telegram/bridge.rs` | MODIFY | Add `jsonl_cwd` param to `spawn_bridge`; make `flush_buffer`, `chunk_text`, `BridgeLogger`, `DiagLogger` `pub(super)` |
| `telegram/manager.rs` | MODIFY | Add `jsonl_cwd` param to `attach`; conditional `output_senders` registration |
| `session/session.rs` | MODIFY | Add `is_claude: bool` field with `#[serde(default)]` |
| `commands/session.rs` | MODIFY | Set `session.is_claude` at creation; pass `jsonl_cwd` to all `tg.attach()` calls |
| `commands/telegram.rs` | MODIFY | Look up session to derive `jsonl_cwd`; pass to `tg.attach()` |

### Dependencies

No new crate dependencies. Uses only: `serde_json` (already in Cargo.toml), `std::fs`, `std::io::Seek`, `tokio::time`, `tokio_util::sync::CancellationToken` (already used).

### Risk Assessment

- **Low risk**: `Session` struct change is backward-compatible (`#[serde(default)]`)
- **Low risk**: PTY pipeline unchanged — JSONL is an alternative path, not a modification
- **Medium risk**: JSONL file path resolution depends on Claude Code's undocumented mangling convention — but we already implement it (`commands/session.rs:72-74`) and it works
- **Low risk**: Polling approach has no race conditions (append-only file, single reader)
