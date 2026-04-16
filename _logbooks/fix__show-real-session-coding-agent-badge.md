# Issue #58 Logbook

## Problem Statement

The sidebar should show the actual coding agent used by each live session. The current implementation does not preserve that identity on the session model, so the UI falls back to `preferredAgentId`, repo tooling badges, or shell-string guessing. That produces false badges, loses the badge on restart/restore paths, and cannot reliably represent unknown agents as "no badge".

Expected:
- Live sessions expose the actual launched agent as session metadata.
- Restart, restore, and deferred restore preserve that metadata.
- Unknown or unrecognized launches render no badge.

Observed:
- `create_session_inner(...)` resolves agent identity but does not persist it on `Session` / `SessionInfo`.
- `SessionItem` derives badges from repo tooling plus shell matching.
- `ProjectPanel` has no live-session agent badge in the replica badge row.
- `restart_session(...)` drops stored agent identity when no new agent override is supplied.

## Investigation Log

### 2026-04-16 14:00 - Reproduce by code inspection

What changed:
- No code change. Inspected `src-tauri/src/commands/session.rs`, `src-tauri/src/session/session.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/config/sessions_persistence.rs`, `src/shared/types.ts`, `src/sidebar/components/ProjectPanel.tsx`, `src/sidebar/components/SessionItem.tsx`, and `src/sidebar/stores/sessions.ts`.

How it was tested:
- Verified the live session create, restart, restore, deferred restore, and sidebar rendering paths against the issue requirements.

Result:
- Confirmed the bug exists in code: the resolved agent identity is not carried on the session model, restart without agent switch can drop identity, and the active sidebar badge rendering uses frontend guessing instead of authoritative session metadata.

### 2026-04-16 14:25 - Implement session agent metadata preservation

What changed:
- Added nullable `agent_id` / `agent_label` to Rust `Session`, `SessionInfo`, and persisted session snapshots.
- Resolved the actual launched agent before session record creation in `create_session_inner(...)`, with shell auto-detection as fallback and normalization to `None` / `None` when an explicit agent request falls back to the default shell.
- Updated restore and deferred restore to carry persisted agent metadata.
- Updated `restart_session(...)` to carry forward stored agent metadata when no new agent override is requested.
- Added `agentId` / `agentLabel` to the shared TypeScript `Session` shape and inactive placeholder sessions.
- Switched `ProjectPanel` and `SessionItem` badges to live session metadata, preferring the stored label and only falling back to settings lookup when the stored label is missing.

How it was tested:
- Static verification of all affected create/restart/restore/deferred-restore call sites after the edits.

Result:
- All scoped issue paths now read from authoritative session metadata instead of frontend guessing.

### 2026-04-16 14:33 - Compile and type-check validation

What changed:
- Ran formatter on the touched Rust files as part of `cargo fmt`; restored unrelated formatter-only file changes outside the issue scope.

How it was tested:
- `cargo check` in `src-tauri/`
- `npx tsc --noEmit` at repo root

Result:
- Both validation commands passed.

### 2026-04-16 14:36 - Manual reasoning validation

What changed:
- No code change. Validated the requested scenarios against the final code paths.

How it was tested:
- Explicit launch: `create_session(...)` passes requested agent input to `create_session_inner(...)`, which resolves and stores the actual pair before session creation.
- Auto-detected launch: when no explicit agent metadata is provided, `create_session_inner(...)` falls back to `resolve_agent_from_shell(...)` and persists the detected pair.
- Restart without changing agent: `restart_session(...)` now reads the existing session's stored `agent_id` / `agent_label` and passes them forward when there is no override.
- Restore/deferred restore: persisted `agent_id` / `agent_label` now flow through both `create_session_inner(...)` and the direct dormant-session `SessionManager::create_session(...)` path in `lib.rs`.
- Unknown/unrecognized launch: if an explicit agent request cannot be resolved and launch falls back, `create_session_inner(...)` normalizes the actual agent fields to `None` / `None`, and the frontend renders no badge when neither stored label nor fallback lookup is available.

Result:
- The implemented paths satisfy the required badge semantics by inspection, including the no-badge case for unknown agents.

### 2026-04-16 14:48 - Grinch follow-up: shell validation must be authoritative

What changed:
- Tightened `resolve_actual_agent(...)` so caller-supplied agent metadata is no longer trusted before validating the final launched `shell` + `shell_args`.
- The resolver now:
  - keeps the detected agent when the final shell validates the requested agent,
  - uses the shell-detected agent when it positively resolves to a different configured agent,
  - clears actual-agent metadata to `None` / `None` when the requested agent cannot be validated against the final launched shell.
- Added focused unit tests covering the validated, unresolved, and mismatch cases.

How it was tested:
- Code-path inspection against explicit launch, malformed explicit launch, and restart/restore behavior.

Result:
- The remaining false-badge path reported by grinch is removed without widening scope beyond the resolver.

### 2026-04-16 15:02 - Grinch follow-up: preserve stored label on validated restart/restore

What changed:
- Updated `resolve_actual_agent(...)` so a validated explicit `requested_agent_id` preserves `requested_agent_label` when present instead of drifting to the current settings-derived label.
- Added a test covering the stored-label-wins case and a second validated-match test confirming the detected/settings label is still used when no stored label exists.

How it was tested:
- Code-path inspection against live restart/restore and targeted resolver unit tests.

Result:
- Live restart/restore now preserves the stored label for a validated agent and no longer drifts when settings labels are renamed later.
