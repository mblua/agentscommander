# Plan: PTY Double-Enter Reliability Fix

**Branch:** `fix/pty-double-enter-reliability`
**Status:** Draft
**Created:** 2026-04-12

---

## Problem

When AC delivers a message to a Claude Code or Codex PTY, the payload is pasted as a text block and a single `\r` is sent after a 1500ms delay to submit it. Sometimes this `\r` doesn't register and the message sits unsubmitted. Observed multiple times with both Claude Code and Codex.

## Fix

Add a second `\r` after an additional 500ms (2000ms total from payload write) as a safety net. If the first Enter worked, the agent is already processing and an extra Enter on empty input is harmless. If it didn't register, the second one catches it.

---

## Change

**File:** `src-tauri/src/pty/inject.rs`, lines 69–81

### Before

```rust
    // Agent CLIs (Claude, Codex): send Enter as a separate write after a delay.
    // The delay must be long enough for the agent to finish processing the pasted
    // text block and exit any internal "paste detection" mode.
    if send_enter {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        log::info!("[inject] sending Enter for session {}", session_id);
        let pty_mgr = app.state::<Arc<Mutex<PtyManager>>>();
        pty_mgr
            .lock()
            .map_err(|_| "PtyManager lock poisoned".to_string())?
            .write(session_id, b"\r")
            .map_err(|e| format!("PTY Enter write failed: {}", e))?;
    }
```

### After

```rust
    // Agent CLIs (Claude, Codex): send Enter twice with staggered delays.
    // Sometimes a single \r doesn't register (race with paste-detection mode).
    // The second \r is a safety net — if the first worked, the agent is already
    // processing and an extra Enter on empty input is harmless.
    if send_enter {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        log::info!("[inject] sending Enter (1/2) for session {}", session_id);
        {
            let pty_mgr = app.state::<Arc<Mutex<PtyManager>>>();
            pty_mgr
                .lock()
                .map_err(|_| "PtyManager lock poisoned".to_string())?
                .write(session_id, b"\r")
                .map_err(|e| format!("PTY Enter (1/2) write failed: {}", e))?;
        }

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        log::info!("[inject] sending Enter (2/2) for session {}", session_id);
        {
            let pty_mgr = app.state::<Arc<Mutex<PtyManager>>>();
            pty_mgr
                .lock()
                .map_err(|_| "PtyManager lock poisoned".to_string())?
                .write(session_id, b"\r")
                .map_err(|e| format!("PTY Enter (2/2) write failed: {}", e))?;
        }
    }
```

---

## Considerations

- **Scoped lock blocks:** Each `pty_mgr.lock()` is in its own `{ }` block so the `MutexGuard` drops before the next `sleep`. This prevents holding the lock across the 500ms delay.
- **Harmless on success:** If the first `\r` worked, the agent is already processing a prompt. An extra `\r` on an empty input line is a no-op in both Claude Code and Codex.
- **No other files affected:** This is contained entirely in `inject.rs`.
- **Total delay:** 2000ms from payload write to final Enter (was 1500ms). The 500ms added latency is negligible for message delivery.

---

## Files Modified

| File | Lines | Change |
|------|-------|--------|
| `src-tauri/src/pty/inject.rs` | 69–81 | Replace single Enter with double Enter + scoped lock blocks |

---

## Dev-Rust Review

**Line numbers verified.** Lines 69–81 in `inject.rs` match the "Before" code exactly. The plan is sound and the change is minimal. One issue found that requires a code change in the proposed implementation.

### Issue: Second Enter failure must not propagate as an error

The plan's proposed code uses `?` to propagate the second Enter's write error:

```rust
.map_err(|e| format!("PTY Enter (2/2) write failed: {}", e))?;
```

This is wrong. If the first `\r` succeeded, the message was submitted. A failure on the second `\r` should not be treated as a delivery failure. The scenario: agent processes the message very quickly, exits or the session is destroyed, and the 500ms-later second write finds no PTY writer → `PtyManager::write` returns an error → `inject_text_into_session` returns `Err` → caller treats it as a failed delivery.

Callers affected:
- **`mailbox.rs:714`** — propagates via `?`, skips emitting `message_delivered` event. The mailbox would log "PTY injection FAILED" even though the message was actually delivered. Could cause duplicate delivery if the caller retries.
- **`mailbox.rs:782`** — same, error is propagated to caller.
- **`bridge.rs:792`** — uses `if let Err(e)`, logs but doesn't propagate. Not affected functionally, but would produce a misleading PTY_ERR log.

**Fix:** The second Enter should log a warning on failure but not propagate:

```rust
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        log::info!("[inject] sending Enter (2/2) for session {}", session_id);
        {
            let pty_mgr = app.state::<Arc<Mutex<PtyManager>>>();
            match pty_mgr
                .lock()
                .map_err(|_| "PtyManager lock poisoned".to_string())
                .and_then(|mut mgr| mgr.write(session_id, b"\r").map_err(|e| e.to_string()))
            {
                Ok(()) => {}
                Err(e) => log::warn!("[inject] Enter (2/2) failed for session {} (non-fatal): {}", session_id, e),
            }
        }
```

This ensures the function returns `Ok(())` as long as the text write + first Enter succeeded, regardless of whether the safety-net second Enter worked.

### Note: Doc comment update

The function's doc comment (lines 30–32) says Enter is sent "after a 1500 ms delay". Update to reflect the double-Enter behavior:

```rust
///   For agents that require explicit Enter (Claude, Codex), `\r` is sent
///   twice — at 1500 ms and 2000 ms after the text write — as a reliability
///   measure against Enter not registering on the first attempt.
```

### Verified

- Scoped lock blocks are correct — `MutexGuard` drops before each `sleep`.
- `app.state()` called twice is fine — it's a cheap `Arc` lookup, and the `State<'_>` borrow can't survive across the `await` point anyway.
- "Harmless on success" claim is correct for both Claude Code and Codex — empty Enter on their prompt is a no-op.
- 500ms gap between the two Enters is reasonable. The issue is about `\r` not registering (race), not about insufficient delay.

---

## Grinch Review

**VERDICT: APPROVED — dev-rust's error-propagation fix is mandatory. One additional finding (non-blocking).**

### Verification of Dev-Rust's Finding

**Confirmed critical.** Traced all three call sites:

1. **`mailbox.rs:714`** — Propagates via `?`. If second Enter fails (session destroyed during 500ms window), execution skips the `message_delivered` event emission at line 721 and returns Err. The mailbox logs "PTY injection FAILED" even though the first Enter already submitted the message. The `message_delivered` event is how the system marks delivery complete — skipping it is a bug.

2. **`mailbox.rs:782`** (`inject_followup_after_idle_static`) — Returns the error to line 658, which uses `if let Err(e)` and logs a warning. **Less severe than dev-rust suggests** — this caller already handles the error gracefully. No retry, no skipped events. Just a misleading warning log.

3. **`bridge.rs:792`** — Uses `if let Err(e)`, logs `PTY_ERR`. No propagation. Functionally correct but would produce a misleading error log.

Dev-rust's proposed fix (match + log::warn for second Enter) is the correct solution. The function must return `Ok(())` once the first Enter succeeds.

### Additional Finding

**[INFO] "Harmless on success" claim — mostly correct with one theoretical edge case.**

`PtyManager::write` (manager.rs:307–322) writes to the PTY writer and flushes. The `\r` goes directly into the PTY's stdin pipe. If the first Enter submits the message and the agent processes it in under 500ms (completes response + presents new prompt), the second `\r` arrives at a fresh prompt.

For Claude Code and Codex, empty Enter on a fresh prompt is indeed a no-op — both agents require actual content before submitting. Verified by the existing comment and confirmed by dev-rust.

The edge case only matters if:
- Agent processes the message in < 500ms (unrealistic for any meaningful prompt)
- AND returns to a state where Enter has side effects (e.g., a confirmation prompt from the PREVIOUS response's tool use)

The 500ms window makes this essentially unreachable. Not worth adding complexity to guard against.

### What I Checked That Passed

- **Lock safety:** `std::sync::Mutex<PtyManager>` is scoped with `{ }` blocks. `MutexGuard` drops before each `.await` point. No tokio runtime blocking across sleep. ✓
- **Session destruction race (1500ms window):** Between text write and Enter 1, if session is destroyed, Enter 1 fails and error propagates. This is correct — if the PTY is gone before submission, the message was NOT submitted. ✓
- **Concurrent injection:** Two messages arriving simultaneously would interleave their text and Enters. This is pre-existing (not introduced by this plan). The double-Enter doesn't make it worse — just more no-op Enters. ✓
- **`app.state()` called twice:** Cheap `Arc` lookup from Tauri's managed state map. No allocation, no contention. ✓
- **PtyManager inner mutex** (`instance.writer.lock().unwrap()` at manager.rs:313): Uses `.unwrap()` which panics on poison. Pre-existing, not introduced by this plan. ✓
- **No new files, no new structs, no scope creep.** ✓
