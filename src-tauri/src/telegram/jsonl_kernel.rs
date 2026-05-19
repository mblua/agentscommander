// Shared scaffold for append-only JSONL session-file readers.
// Hosts the primitives reused by claude_watcher, codex_watcher, and
// gemini_watcher: directory scan, offset-based incremental read,
// truncation detection, and rotation-flicker constants.

use std::io::{Read as IoRead, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub(crate) const POLL_INTERVAL_MS: u64 = 500;
/// Duration a tracked file must be stale before switching to a newer one (file rotation guard)
pub(crate) const ROTATION_STALE_SECS: u64 = 3;

/// Find the most recently modified .jsonl file in a directory (non-recursive).
/// Used by the Claude watcher; Codex and Gemini have their own per-poll discovery.
pub(crate) fn find_latest_jsonl(project_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(project_dir).ok()?;
    let mut best: Option<(PathBuf, SystemTime)> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if let Ok(modified) = meta.modified() {
                match &best {
                    Some((_, best_time)) if modified > *best_time => {
                        best = Some((path, modified));
                    }
                    None => {
                        best = Some((path, modified));
                    }
                    _ => {}
                }
            }
        }
    }

    best.map(|(p, _)| p)
}

/// Read new bytes from a file starting at the given byte offset.
/// Returns parsed complete lines and updates the offset by actual bytes read.
/// Partial lines are accumulated in `remainder` for the next poll.
///
/// On shrink (truncation), resets `offset = 0` and clears `remainder` so the
/// next read starts from the beginning. The H2 reset-to-EOF semantics land in
/// commit 5 (preamble scan + truncation-skip fix).
pub(crate) fn read_new_lines(
    path: &Path,
    offset: &mut u64,
    remainder: &mut String,
) -> std::io::Result<Vec<String>> {
    let mut file = std::fs::File::open(path)?;
    // Use metadata on the open handle (avoids TOCTOU with path-based metadata)
    let file_len = file.metadata()?.len();

    // G3: File truncation/shrink detection — reset to beginning
    if file_len < *offset {
        log::warn!(
            "[JSONL_TRUNCATE] File shrank ({} < {}), resetting offset",
            file_len,
            *offset
        );
        *offset = 0;
        remainder.clear();
    }

    if file_len <= *offset {
        return Ok(vec![]);
    }

    file.seek(SeekFrom::Start(*offset))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;

    // G2: Track offset by actual bytes read, not reported file length
    *offset += buf.len() as u64;

    // Prepend any partial line from previous read
    if !remainder.is_empty() {
        let mut combined = std::mem::take(remainder);
        combined.push_str(&buf);
        buf = combined;
    }

    let mut lines = Vec::new();
    let mut last_newline = 0;

    for (i, ch) in buf.char_indices() {
        if ch == '\n' {
            let line = &buf[last_newline..i];
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
            last_newline = i + 1;
        }
    }

    // Keep unterminated tail in remainder for next poll
    if last_newline < buf.len() {
        *remainder = buf[last_newline..].to_string();
    }

    Ok(lines)
}
