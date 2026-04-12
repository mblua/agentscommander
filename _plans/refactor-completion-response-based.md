# Plan: Refactor Completion Detection — Response-Based (No Phrase)

**Branch:** `feature/agent-completion-detection` (continue existing)
**Status:** Approved for implementation

---

## Summary

Replace phrase-based completion detection with response-tracking logic:
- **Completed** = idle 5+ min AND agent already responded to its last received message
- **Follow-up** = idle 5+ min AND agent has NOT responded → inject reminder message
- **Hung** = after follow-up, still idle 5+ min with no response → mark hung

Remove all phrase scanning, phrase config, and phrase context injection.

---

## State Machine

```
Message injected → Working (has_pending_response=true, followup_sent=false)
Agent sends via outbox → has_pending_response=false (stays Working)

Watcher (every 5s):
  idle > timeout AND !has_pending_response → Completed
  idle > timeout AND has_pending_response AND !followup_sent → inject follow-up, set followup_sent=true
  idle > timeout AND has_pending_response AND followup_sent → Hung
```

**Follow-up injection** does NOT call `reset()` — it writes directly to PTY via `inject_text_into_session`. The natural PTY activity from injection resets `idle_since` via the idle detector, so another 5 min must pass before hung triggers.

**Regular message injection** calls `reset()` (clears everything) then `record_message_received()` (sets has_pending_response=true).

---

## Changes by File

### 1. `src-tauri/src/pty/completion_tracker.rs` — Major refactor

**Remove:**
- `phrase: String` field from CompletionTracker
- `scan_phrase()` method
- `record_phrase_detected()` method
- `phrase_detected: bool` from SessionCompletionState

**Add:**
- `has_pending_response: bool` to SessionCompletionState (default: false)
- `followup_sent: bool` to SessionCompletionState (default: false)
- `on_followup: Arc<dyn Fn(Uuid, String) + Send + Sync>` callback to CompletionTracker

**New methods:**
- `record_message_received(&self, session_id: Uuid)` — sets has_pending_response=true
- `record_response_sent(&self, session_id: Uuid)` — sets has_pending_response=false
- `mark_followup_sent(&self, session_id: Uuid)` — sets followup_sent=true (called by the on_followup callback after injection)

**Modify `new()`:**
- Remove `phrase: String` parameter
- Add `on_followup` callback parameter (signature: `Fn(Uuid, String) + Send + Sync`)
- Keep `hung_timeout_secs` (still used for idle threshold)

**Modify `reset()`:**
- Remove `phrase_detected = false`
- Add `has_pending_response = false`, `followup_sent = false`
- Keep `idle_since = None`, `status = Working`, `hung_notified = false`

**Modify watcher loop (lines 155-172):**
```rust
// OLD:
// if phrase_detected && idle_since.is_some() → Completed
// if !phrase_detected && idle > timeout → Hung

// NEW:
// if !has_pending_response && idle > timeout → Completed
// if has_pending_response && idle > timeout && !followup_sent → fire on_followup
// if has_pending_response && idle > timeout && followup_sent && !hung_notified → Hung
```

Collect follow-up events in a vec alongside completed/hung vecs, fire all callbacks outside the lock.

### 2. `src-tauri/src/lib.rs` — Update tracker initialization (lines 184-219)

**Modify CompletionTracker::new() call:**
- Remove `settings_for_tracker.completion_phrase.clone()` argument
- Add third callback (`on_followup`) that:
  1. Injects reminder text into the session's PTY using `inject_text_into_session`
  2. Calls `tracker.mark_followup_sent(session_id)` after injection
  3. Emits `"agent_followup_sent"` event for frontend awareness

**Followup callback needs:** `app_handle` (via OnceLock) to access PTY inject + tracker.

**Followup message text:**
```
[System Reminder] You appear to be idle. Don't forget to reply to whoever requested your last task. Use the send command to report your results.
```

### 3. `src-tauri/src/config/settings.rs` — Remove phrase setting (lines 107-109, 143-145)

**Remove:**
- `completion_phrase: String` field (line 108-109)
- `default_completion_phrase()` function (lines 143-145)
- Keep `hung_timeout_secs` (still needed)

**Note:** Add `#[serde(default)]` skip for deserialization backward compat — old TOML files with `completion_phrase` key won't error. Use `#[serde(skip_serializing, default)]` or just let unknown keys be ignored (serde `deny_unknown_fields` is not set).

### 4. `src-tauri/src/config/session_context.rs` — Remove phrase instruction (lines 443-460)

**Remove entire "Task Completion Signal" section** (lines 443-460), including the closing `"#)` bracket adjustment.

### 5. `src-tauri/src/pty/manager.rs` — Remove phrase scanning (lines 279-282)

**Remove:**
```rust
// Scan for completion phrase
if has_printable && completion_tracker.scan_phrase(&text) {
    completion_tracker.record_phrase_detected(id);
}
```

### 6. `src-tauri/src/phone/mailbox.rs` — Track message receipt + response sent

**At message injection (line 717-720):**
After `tracker.reset(session_id)`, add:
```rust
tracker.record_message_received(session_id);
```

**At follow-up injection (line 791-794):**
Same: after `tracker.reset(session_id)`, add:
```rust
tracker.record_message_received(session_id);
```

**At successful message delivery (line 336-339, in `process_message`):**
After mode-based dispatch succeeds, before `move_to_delivered()`, add logic to detect sender's session_id and record response:
```rust
// Track that sender has responded (for completion detection)
if let Some(ref token_str) = msg.token {
    if let Ok(token_uuid) = Uuid::parse_str(token_str) {
        let session_mgr = app.state::<Arc<tokio::sync::RwLock<SessionManager>>>();
        let mgr = session_mgr.read().await;
        if let Some(sender_session) = mgr.find_by_token(token_uuid).await {
            if let Some(tracker) = app.try_state::<Arc<CompletionTracker>>() {
                tracker.record_response_sent(sender_session.id);
            }
        }
    }
}
```

This covers the case when an agent uses `send` command (which includes their session token) — successful delivery means the agent has responded.

### 7. Frontend: `src/shared/ipc.ts` — Add followup event listener

Add:
```typescript
export const onAgentFollowupSent = (cb: (payload: { id: string; name: string }) => void) =>
  listen<{ id: string; name: string }>("agent_followup_sent", (e) => cb(e.payload));
```

### 8. Frontend: `src/sidebar/App.tsx` — Register followup listener

Register `onAgentFollowupSent` listener that optionally adds a notification of type "followup" so the user can see in the bell icon history that a follow-up was sent.

### 9. Frontend: `src/shared/types.ts` — Extend NotificationType

Add `"followup"` to the NotificationType union if it doesn't already include it.

---

## What stays the same

- Green dot for completed sessions (CSS `.completed`)
- Amber pulsing dot for hung sessions (CSS `.hung`)
- HungNotification popup component
- NotificationsModal (bell icon) with history
- `hung_timeout_secs` setting (default 300s = 5 min)
- IdleDetector integration (mark_idle / mark_busy)
- Session registration (only Claude sessions)
- Reset on regular message injection + emit `completion_status_reset`

---

## Edge Cases

1. **Agent never receives a message** (e.g., user types directly in terminal):
   - `has_pending_response` stays false → idle 5 min → Completed. Correct behavior.

2. **Agent receives message, responds, then keeps working silently**:
   - has_pending_response=false after send → idle 5 min → Completed. Correct.

3. **Agent receives message, never responds, follow-up sent, still nothing**:
   - has_pending_response=true → follow-up at 5 min → followup_sent=true → idle resets from injection activity → another 5 min → Hung. Correct.

4. **Agent responds AFTER follow-up**:
   - Agent sends via outbox → record_response_sent → has_pending_response=false → idle 5 min → Completed. Correct (followup_sent doesn't block completion once agent responded).

5. **New message arrives while status is Completed/Hung**:
   - inject_into_pty → reset() + record_message_received() → back to Working. Correct.

6. **Multiple messages in quick succession**:
   - Each inject calls reset() then record_message_received(). Only the last cycle matters. Correct.

---

## Build Sequence

1. Backend changes (Rust): settings → completion_tracker → lib.rs → manager.rs → mailbox.rs → session_context.rs
2. Frontend changes (TS): types.ts → ipc.ts → App.tsx
3. `cargo check` to verify compilation
4. `npx tsc --noEmit` to verify frontend types
5. Ship build for testing
