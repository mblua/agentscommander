# Plan: Show Real Session Coding Agent Badge

**Branch:** `fix/show-real-session-coding-agent-badge`  
**Scope:** backend session metadata, session persistence, shared `Session` type, active sidebar renderers  
**Status:** Ready for implementation

---

## Requirement

- The sidebar must show the **actual coding agent used by the live session**.
- This badge must **not** come from `preferredAgentId`.
- The badge must sit with the existing badge rows:
  - replica/workgroup cards: next to branch/coordinator/workgroup badges
  - matrix-agent sessions: next to the branch meta row in `SessionItem`
- If the actual agent cannot be resolved, show no badge.
- Existing shell-command auto-detection already counts as valid; the missing piece is that the resolved `agent_id` / `agent_label` are not preserved on the session model or across restore.
- Do **not** touch `AcDiscoveryPanel`; the active sidebar path is `ProjectPanel` + `SessionItem`.

---

## Design Summary

Persist the resolved agent identity on the session itself as nullable `agent_id` / `agent_label`, carry it through `SessionInfo`, save it into `sessions.json`, and restore it back into both live and deferred sessions. Then change the active sidebar UI to read `session.agentLabel` / `session.agentId` instead of guessing from `preferredAgentId`, repo tooling badges, or shell parsing in the frontend.

This keeps the existing detection logic in `create_session_inner(...)`, avoids new IPC commands, and makes the badge stable across restore, plain restart, and restart-with-agent flows.

---

## Affected Files

### 1. `src-tauri/src/session/session.rs`

**Current anchors**
- `Session` struct: lines 20-48
- `SessionInfo` struct: lines 62-82
- `impl From<&Session> for SessionInfo`: lines 84-103

**Change**

Add nullable actual-agent fields to both Rust models:

```rust
#[serde(default)]
pub agent_id: Option<String>,
#[serde(default)]
pub agent_label: Option<String>,
```

Add them to:
- `Session` immediately after the existing session identity/runtime fields
- `SessionInfo` immediately after `is_claude`
- `SessionInfo::from(&Session)` so IPC and persistence snapshots expose them

**Why**

`create_session_inner(...)` already resolves the real agent, but `Session` currently discards it. Until the session model owns these fields, the frontend and persistence layer have nothing reliable to render.

---

### 2. `src-tauri/src/session/manager.rs`

**Current anchors**
- `create_session(...)` signature: lines 26-33
- `Session` construction inside `create_session(...)`: lines 40-56

**Change**

Extend `SessionManager::create_session(...)` to accept:

```rust
agent_id: Option<String>,
agent_label: Option<String>,
```

and write them directly into the constructed `Session`.

The updated signature should be used by both existing call sites:
- `commands/session.rs:create_session_inner(...)`
- `lib.rs` deferred restore path

**Why**

There are only two `create_session(...)` call sites in the repo. Passing the resolved agent identity at construction time is smaller and less error-prone than constructing a session and then mutating it with a new setter afterward.

---

### 3. `src-tauri/src/commands/session.rs`

**Current anchors**
- `create_session_inner(...)`: lines 194-515
- Current post-create auto-detection block: lines 230-237
- `set_last_coding_agent` writeback block: lines 481-511
- `resolve_agent_command(...)`: lines 920-955
- `resolve_agent_from_shell(...)`: lines 957-984

**Change**

Move actual-agent resolution **before** `mgr.create_session(...)`, then pass the resolved values into `SessionManager::create_session(...)`.

Use this resolution order at the top of `create_session_inner(...)`:

1. If the caller already passed both `agent_id` and `agent_label`, keep them.
2. If the caller passed `agent_id` but no label, resolve the label from settings before creating the session.
3. If no `agent_id` was supplied, keep the existing `resolve_agent_from_shell(&shell, &shell_args, &cfg)` behavior.
4. If nothing resolves, keep both fields `None`.

Recommended helper addition near `resolve_agent_command(...)`:

```rust
fn resolve_agent_label(agent_id: &str, settings: &AppSettings) -> Option<String>
```

Then replace the current block:

```rust
// current lines 230-237
let (agent_id, agent_label) = if agent_id.is_some() {
    (agent_id, agent_label)
} else {
    ...
};
```

with logic that guarantees the session is created with the final resolved pair.

**Important behavior requirements**

- Keep `resolve_agent_from_shell(...)` as the fallback detection path. No new heuristic is needed for this issue.
- Do **not** serialize `"Unknown"` into the session model for UI use. Unknown means `None`, and the frontend must show no badge.
- If an explicit `agent_id` no longer resolves and launch falls back to the default shell, store `agent_id = None` and `agent_label = None` on the session. The requested-but-missing agent is not the actual launched agent, so keeping that stale ID would create false badges later.
- The existing `set_last_coding_agent(...)` block may still fall back for config persistence, but `SessionInfo.agentLabel` must come from the resolved nullable field, not from a forced `"Unknown"` display value.
- `restart_session(...)` does need a small logic update even though its command signature stays the same: when no new `agent_id` override is supplied, read the existing session's stored `agent_id` / `agent_label` and pass them back into `create_session_inner(...)` instead of dropping to `(shell, clean_args, None)`. Otherwise, explicit-but-not-auto-detectable launches can still lose the badge on restart.
- Existing `create_session_inner(...)` callers in `web/commands.rs` and `phone/mailbox.rs` already pass `Some(agent_id), None` in some flows. No caller signature change is required there, but the pre-create resolution step must continue resolving the label inside `create_session_inner(...)` so those paths keep producing a usable `agentLabel`.

**Why**

This is the narrowest place to preserve:
- explicit launch agent choice
- auto-detected agent identity from the actual shell command
- restored agent identity for relaunched sessions

without adding new commands or frontend-side guessing.

---

### 4. `src-tauri/src/lib.rs`

**Current anchors**
- Deferred non-coordinator restore path: lines 548-565
- Normal PTY restore call into `create_session_inner(...)`: lines 580-594

**Change**

Pass persisted agent identity through both restore paths:

1. Deferred sessions created via `mgr.create_session(...)` must now pass:

```rust
ps.agent_id.clone(),
ps.agent_label.clone(),
```

2. Normal PTY restores must call:

```rust
create_session_inner(...,
    Some(ps.name.clone()),
    ps.agent_id.clone(),
    ps.agent_label.clone(),
    ...
)
```

instead of the current `None, None`.

**Why**

If this file is not updated, restored sessions will regress to:
- losing the badge completely on restart
- or re-detecting from current settings instead of preserving the actual agent used last time

The deferred restore path is especially important because it bypasses `create_session_inner(...)`; without updating it, dormant team members would still lose their badge.

---

### 5. `src-tauri/src/config/sessions_persistence.rs`

**Current anchors**
- `PersistedSession` struct: lines 16-44
- snapshot mapping: lines 238-251

**Change**

Add persisted optional recipe fields:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub agent_id: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub agent_label: Option<String>,
```

Place them with the core restore recipe fields, not in the runtime-only section, because restore needs them.

Populate them in `snapshot_sessions(...)` from `SessionInfo`:

```rust
agent_id: s.agent_id.clone(),
agent_label: s.agent_label.clone(),
```

**Do not change**
- `deduplicate(...)`
- `strip_auto_injected_args(...)`
- runtime snapshot fields (`id`, `status`, `waiting_for_input`, `created_at`)

**Why**

These two fields must survive app restart. `serde(default)` keeps older `sessions.json` files readable, and `skip_serializing_if` keeps unknown-agent sessions clean.

---

### 6. `src/shared/types.ts`

**Current anchor**
- `Session` interface: lines 1-16

**Change**

Add two nullable IPC fields:

```ts
agentId: string | null;
agentLabel: string | null;
```

Because these fields are nullable-but-required on `Session`, also update `src/sidebar/stores/sessions.ts:makeInactiveEntry(...)` to initialize:

```ts
agentId: null,
agentLabel: null,
```

No `ipc.ts` change is required because the command/event payload shape remains `SessionInfo`; only the shared interface grows.

**Why**

Both active sidebar renderers already consume `Session`. Once these fields exist on the shared type, the frontend can stop inferring the agent from unrelated data.

---

### 7. `src/sidebar/components/ProjectPanel.tsx`

**Current anchors**
- `renderReplicaItem(...)`: lines 396-492
- existing badge row inside replica cards: lines 482-491

**Change**

Inside `renderReplicaItem(...)`, derive the badge from the live session:

```ts
const liveAgentLabel = () => {
  const s = session();
  if (!s) return null;
  if (s.agentLabel) return s.agentLabel;
  const configured = settingsStore.current?.agents?.find((a) => a.id === s.agentId);
  return configured?.label ?? null;
};
```

Then insert a new badge into the existing `.ac-discovery-badges` block, between the branch badge and the coordinator/workgroup badges:

```tsx
<Show when={liveAgentLabel()}>
  <span class="ac-discovery-badge agent">{liveAgentLabel()}</span>
</Show>
```

Rules:
- Do not read `replica.preferredAgentId` for display.
- If the live session has no stored agent identity, render nothing.
- Prefer `session.agentLabel` over a fresh settings lookup. Only use the settings lookup as a compatibility fallback when the stored label is missing.
- Keep the existing branch/coordinator/workgroup badge logic unchanged.

**Why**

This is the exact replica card path the user sees today. The fix is to render from `replicaSession(wg, replica)`, not from discovery-time metadata.

---

### 8. `src/sidebar/components/SessionItem.tsx`

**Current anchors**
- old shell-guess helpers: lines 18-37
- old repo badge source: lines 53-59
- current agent/branch UI block: lines 320-340

**Change**

Replace the current repo/tooling badge logic with a single actual-session badge.

Remove the frontend guessing path:
- `AGENT_BADGES`
- `shellMatchesAgent(...)`
- `agentBadges()`

Add a session-derived helper instead:

```ts
const sessionAgentLabel = () => {
  if (props.session.agentLabel) return props.session.agentLabel;
  const configured = settingsStore.current?.agents?.find((a) => a.id === props.session.agentId);
  return configured?.label ?? null;
};
```

Replace the existing badge block with a single badge rendered only when the actual agent is known.

Recommended markup shape:

```tsx
<div class="session-item-meta">
  <Show when={sessionAgentLabel()}>
    <span class="agent-badge running">{sessionAgentLabel()}</span>
  </Show>
  <Show when={!isInactive() && props.session.gitBranch}>
    <div class="session-item-branch" title={props.session.gitBranch!}>
      {props.session.gitBranch}
    </div>
  </Show>
</div>
```

Rules:
- Show one badge, not one badge per repo tooling option.
- Do not derive display state from `RepoMatch.agents`, `workingDirectory`, or shell string parsing.
- Keep the existing restart/switch/picker interactions unchanged.

**Why**

The current implementation shows available repo tooling and highlights a guessed match. That is not the same thing as “the actual coding agent used by this session”, and it fails for sessions whose repo tooling list does not match the launched command.

---

### 9. `src/sidebar/styles/sidebar.css`

**Current anchors**
- `.session-item-branch`: lines 430-438
- `.session-item-agent-badges` / `.agent-badge`: lines 477-496
- `.ac-discovery-badges`: lines 2283-2317

**Change**

Add a small meta-row style for `SessionItem` and a dedicated replica badge variant:

```css
.session-item-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
  min-width: 0;
  margin-top: 2px;
}

.ac-discovery-badge.agent {
  background: rgba(16, 185, 129, 0.14);
  color: #34d399;
  text-transform: none;
}
```

Implementation notes:
- Keep `.agent-badge` as the SessionItem visual primitive.
- Do not rely on the old multi-badge `.session-item-agent-badges` container once the single-badge meta row is in place.
- No theme-wide refactor is needed for this issue.

---

## Risks and Edge Cases

1. **Backward compatibility with existing `sessions.json`**
   - Older persisted sessions will not have `agentId` / `agentLabel`.
   - `serde(default)` plus the existing shell auto-detection fallback in `create_session_inner(...)` keeps restore compatible.

2. **Explicit `agent_id` without label**
   - Some callers pass `agent_id` and rely on later label lookup.
   - If the label is not resolved before session creation, the badge will disappear even though the real agent is known.

3. **Plain restart without agent switch**
   - `restart_session(...)` currently reconstructs from `shell` / cleaned args only when no new `agent_id` is requested.
   - If the original session came from an explicit agent selection whose command is not re-detectable from the shell string, the badge will disappear unless the stored session agent fields are carried forward.

4. **Deferred restore path**
   - `start_only_coordinators` creates dormant sessions through `SessionManager::create_session(...)` directly.
   - If that path is missed, coordinator-only restores will still lose the badge.

5. **Do not display `"Unknown"`**
   - The requirement says “show no badge” when the real agent is not known.
   - Preserve `None` in the session/UI model; do not invent a display label.

6. **No `AcDiscoveryPanel` drift**
   - The unused discovery panel should remain untouched to avoid widening scope.

---

## Validation

1. `cargo check`
2. `npx tsc --noEmit`
3. Manual: create a replica session with an explicit agent switch via restart/picker; the badge updates to the new actual agent, not `preferredAgentId`.
4. Manual: create a session where `create_session_inner(...)` auto-detects the agent from the launched shell command; the badge appears without any preferred-agent fallback.
5. Manual: create a plain shell session or any unrecognized command; no agent badge renders.
6. Manual: restart a session without changing agents; an explicitly chosen but non-auto-detectable agent remains visible after the fresh PTY comes up.
7. Manual: restart the app; restored live sessions and deferred restore entries retain the same badge.

---

## Dependencies

No new crates or npm packages.

---

## Dev-Rust Review

**Verdict:** implementable with adjustments.

**Additions made in this review**

- Added a required `restart_session(...)` carry-forward step for stored `agent_id` / `agent_label` when no new agent override is supplied. The draft covered full restore, but the plain restart path could still drop the real agent for explicit launches that are not re-detectable from `shell` + `shellArgs`.
- Added an explicit note that `web/commands.rs` and `phone/mailbox.rs` already call `create_session_inner(...)` with `Some(agent_id), None` in some flows. That means the inner pre-create label resolution is required behavior for existing callers, not optional cleanup.
- Added a manual validation case for restart-without-switch so implementation verifies the corrected path rather than only initial create and full app restore.

## Grinch Review

1. **What**: The draft UI helpers preferred the current settings label over the session's stored `agentLabel`.
   **Why**: If the configured agent label changes after launch, the sidebar would display a fresh settings value instead of the actual session metadata. That violates the requirement to show the real agent used by the session, not a newly-derived guess.
   **Fix**: Prefer `session.agentLabel` first in both `ProjectPanel` and `SessionItem`. Only fall back to the settings lookup when the stored label is missing.

2. **What**: The plan grew the shared `Session` interface but did not cover the inactive placeholder session objects.
   **Why**: `src/sidebar/stores/sessions.ts:makeInactiveEntry(...)` constructs `Session` literals directly. Once `agentId` / `agentLabel` become required nullable fields, `tsc` will fail unless those placeholders initialize both fields.
   **Fix**: Update `makeInactiveEntry(...)` to set `agentId: null` and `agentLabel: null`.

3. **What**: The draft did not state what happens when a caller supplies an `agent_id` that no longer resolves.
   **Why**: `resolve_agent_command(...)` already falls back to the default shell when the configured agent is missing. If the session still persists the stale requested `agent_id`, a later UI/settings lookup can show a false badge for an agent that was never actually launched.
   **Fix**: Make the normalization explicit in `create_session_inner(...)`: when explicit agent resolution fails and launch falls back, persist `None` / `None` as the actual-agent fields.

Verdict: ready with adjustments.
