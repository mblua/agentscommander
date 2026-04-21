# Bug fix — StatusBar must show effective launch command (incl. dynamic flags)

**Issue**: https://github.com/mblua/AgentsCommander/issues/65
**Branch**: `feature/terminal-full-command` (bundled on top of the existing full-command feature plan — NOT a new branch)
**Repo**: `repo-AgentsCommander`
**Reads on top of**: `_plans/feature-terminal-full-command.md` (same branch). This plan describes **deltas** to the frontend changes proposed there, plus new backend work.

---

## 1. Problem statement

The StatusBar (v0.7.4, introduced on this feature branch) renders `session.shellArgs` — the **configured** launch command as stored in `Session.shell_args`. It does NOT reflect dynamic flags that AC injects at spawn time:

- `--continue` (Claude, when prior conversation dir exists)
- `codex resume --last` (Codex, when no explicit subcommand/resume present)
- `--append-system-prompt-file <path>` (Claude, when materialized context exists)

All three are injected inside `create_session_inner` into a **local mutable** `shell_args` vec (shadowed from the caller's vec at `src-tauri/src/commands/session.rs:249`). That mutated local is handed to `portable-pty::CommandBuilder` at `src-tauri/src/commands/session.rs:368-372`, but it is never written back into the `Session` record. Therefore `SessionInfo::from(&session)` emits the pre-injection args, and the frontend never sees the effective command.

User confirmed on standalone 0.7.4 that the resume logic works (Claude sessions ARE being continued) — the visibility is the gap.

## 2. Expected

StatusBar shows the **literal arg vector handed to `portable-pty::CommandBuilder`** at spawn time, for the session currently displayed in the Terminal window. On session switch, the StatusBar updates to the newly-displayed session's effective args. When effective args are not yet available (dormant sessions restored without PTY spawn, or any pre-spawn race), the StatusBar command block is **empty** — not the configured args, not a placeholder.

---

## 3. Root cause (file:line)

`src-tauri/src/commands/session.rs::create_session_inner`:

| Line | Event |
|------|-------|
| **226-237** | `mgr.create_session(shell.clone(), shell_args.clone(), ...)` stores the Session with `shell_args = configured` (pre-injection). `session` local is a clone of that stored value. |
| **249** | `let mut shell_args = shell_args;` — shadows the outer binding with a local mutable vec. All subsequent injections mutate THIS local, not `session.shell_args` and not the store. |
| **289-314** | Claude `--continue` injection — mutates local. |
| **316-322** | Codex `resume --last` injection — mutates local. |
| **350-366** | Claude `--append-system-prompt-file <path>` injection — mutates local. |
| **368-372** | `pty_mgr.spawn(id, &shell, &shell_args, ...)` — hands the mutated local to portable-pty. This is the **effective arg vector**. |
| **438-439** | `SessionInfo::from(&session)` → `emit("session_created", info)`. But `session.shell_args` is still the configured vec from step 226 — so the emit never carries the injected flags. |

**The fix is a single capture point**: between line 366 and line 368, write the final `shell_args` into a new `Session.effective_shell_args` field and copy it onto the local `session` clone so the emit at line 439 picks it up.

---

## 4. Activation paths (backend)

All user-facing PTY-spawning paths go through `create_session_inner`, so a single capture point covers them:

| Path | Caller | `skip_auto_resume` | Spawns PTY? |
|------|--------|-------------------:|:-----------:|
| Fresh session | `commands::session::create_session` → `create_session_inner` (L532) | `false` | ✅ |
| Restart | `commands::session::restart_session` → strips auto args (L711 via `strip_auto_injected_args`) → `create_session_inner` (L728) | **`true`** | ✅ |
| Root agent | `commands::session::create_root_agent_session` → `create_session_inner` (L1100) | `false` | ✅ |
| Startup restore (normal) | `lib.rs:575` → `create_session_inner` | `false` | ✅ |
| Startup restore (**dormant**, non-coordinator when `start_only_coordinators=true`) | `lib.rs:543` → `mgr.create_session` (no PTY), `mark_exited(0)`, emits `session_created` with status Exited | n/a | ❌ |
| Web remote | `web/commands.rs:68` → `create_session_inner` | `false` | ✅ |
| Mailbox wake | `phone/mailbox.rs:513, 1580` → `create_session_inner` | `false` | ✅ |
| Git-watcher test | `pty/git_watcher.rs:206` — inside `#[cfg(test)]` only | n/a | ❌ (test) |

**Dormant sessions** (path #5) never run `create_session_inner`, so they naturally have `effective_shell_args = None` → StatusBar hides the command block. Matches spec §3 exactly.

If a dormant session is later woken up (user clicks to restart it), `restart_session` runs `create_session_inner` and `effective_shell_args` is populated at that point.

---

## 5. Backend design (Rust)

### 5.1 `src-tauri/src/session/session.rs`

**(a)** Add a new field to `Session` (struct at lines 41-81). Insert immediately after `pub shell_args: Vec<String>,` (line 47):

```rust
    /// Effective arg vector actually handed to portable-pty at spawn time,
    /// including dynamic injections (`--continue`, `codex resume --last`,
    /// `--append-system-prompt-file <path>`). `None` until the PTY is
    /// spawned for this session; set once by `create_session_inner` right
    /// before `pty_mgr.spawn`. Runtime-only — NOT persisted to `sessions.toml`
    /// (configured args in `shell_args` are the persistence recipe; the
    /// effective args are re-derived at every spawn from current settings).
    #[serde(skip)]
    pub effective_shell_args: Option<Vec<String>>,
```

`#[serde(skip)]` ensures this field is neither read from nor written to `sessions.toml`. On restart, we always re-derive from the configured `shell_args` + current injection rules.

**(b)** Add a matching field to `SessionInfo` (struct at lines 95-118). Insert after `pub shell_args: Vec<String>,` (line 99):

```rust
    /// See `Session::effective_shell_args`. `None` means "not yet registered"
    /// (dormant or pre-spawn). On the wire, serializes as `null`.
    #[serde(default)]
    pub effective_shell_args: Option<Vec<String>>,
```

Use `#[serde(default)]` (NOT `skip_serializing_if`). This way the field is always serialized, either as `null` or as an array, making the TS contract deterministic. On deserialization (sessions.toml restore of `PersistedSession` → not applicable here; `SessionInfo` is purely IPC, not persisted), missing field → `None` via `default`.

**(c)** Update `impl From<&Session> for SessionInfo` (lines 120-141). Add inside the struct-literal, after `shell_args: s.shell_args.clone(),` (line 126):

```rust
            effective_shell_args: s.effective_shell_args.clone(),
```

### 5.2 `src-tauri/src/session/manager.rs`

**(a)** In `SessionManager::create_session` (lines 26-76), initialize the new field to `None`. Inside the `Session` struct literal at lines 42-60, add after `shell_args,` (line 46):

```rust
            effective_shell_args: None,
```

**(b)** Add a new method on `SessionManager` to set the field. Place it immediately after `set_is_claude` (lines 236-241) to group it with other post-creation setters:

```rust
    /// Register the effective arg vector actually handed to portable-pty
    /// at spawn time. Called by `create_session_inner` immediately before
    /// `pty_mgr.spawn`. Idempotent — callers write the final vec once per
    /// session lifetime. Overwrites on re-call (defensive; not expected in
    /// normal flow).
    pub async fn set_effective_shell_args(&self, id: Uuid, args: Vec<String>) {
        let mut sessions = self.sessions.write().await;
        if let Some(s) = sessions.get_mut(&id) {
            s.effective_shell_args = Some(args);
        }
    }
```

### 5.3 `src-tauri/src/commands/session.rs::create_session_inner`

Add the capture call **between lines 366 and 368** — after all injection logic, immediately before `pty_mgr.spawn`. Replace the current block at lines 368-372:

```rust
    pty_mgr
        .lock()
        .unwrap()
        .spawn(id, &shell, &shell_args, &cwd, 120, 30, app.clone())
        .map_err(|e| e.to_string())?;
```

with:

```rust
    // Capture the effective arg vector BEFORE spawn so SessionInfo::from(&session)
    // (emitted at line ~439 as "session_created") carries the injected flags.
    // The store write is for consumers that call mgr.get_session later; the
    // local-clone write is for the imminent emit below.
    mgr.set_effective_shell_args(id, shell_args.clone()).await;
    session.effective_shell_args = Some(shell_args.clone());

    pty_mgr
        .lock()
        .unwrap()
        .spawn(id, &shell, &shell_args, &cwd, 120, 30, app.clone())
        .map_err(|e| e.to_string())?;
```

Rationale for placement:
- AFTER all injections (lines 276-366) — so the captured vec is the effective one.
- BEFORE `pty_mgr.spawn` — if spawn fails (line 372 returns `Err`), the session record still carries the args we *attempted* to spawn, which is the honest state to report. The session remains in the manager on spawn failure anyway (existing behavior, out of scope).
- Uses the existing `mgr` guard (acquired at line 225 as a read-lock on the outer `RwLock<SessionManager>`). `set_effective_shell_args` only needs `&self`, and takes a write lock on the inner `sessions` map — no deadlock risk.

**IMPORTANT — capture shape**: the captured vector is the **pre-wrapping** `shell_args`, i.e. the logical user-level args (e.g. `["--dangerously-skip-permissions", "--effort", "max", "--continue"]`). The `cmd.exe /C <cmd>` wrapping applied inside `PtyManager::spawn` (pty/manager.rs:173-187) for non-.exe Windows commands is a platform detail and MUST NOT be captured. The user wants to see `claude-mb --continue`, not `cmd.exe /C claude-mb --continue`.

The StatusBar will render these captured args concatenated with `shell` — so for a session with `shell="claude-mb"` and effective args `["--dangerously-skip-permissions","--effort","max","--continue"]`, the display is `claude-mb --dangerously-skip-permissions --effort max --continue`.

### 5.4 No new IPC event needed

The existing `session_created` emit at line 439 already carries `SessionInfo` — with the new field added at §5.1.b, it automatically ships to the frontend. No new command, no new event.

For dormant sessions (lib.rs:543), the emit at L559 uses `SessionInfo::from(&updated)` — the `updated` session has `effective_shell_args = None`, which serializes as `null`. Frontend handles null by hiding the block. No code change needed in `lib.rs`.

For later activation of a dormant session (via `restart_session`): the new session from `create_session_inner` carries effective args in its `session_created` emit. The frontend re-fetches the active session list on switch, so it picks up the populated field.

### 5.5 Persistence interaction

`strip_auto_injected_args` (config/sessions_persistence.rs:355) is already designed for this: on every save, `Session.shell_args` is stripped to remove auto-injected flags, so `sessions.toml` holds the configured recipe only. `effective_shell_args` has `#[serde(skip)]` and is never persisted. On restore, `create_session_inner` re-runs injection logic against current settings and re-captures.

**No changes to sessions_persistence.rs needed.** Confirmed.

---

## 6. Frontend design — DELTA on top of `feature-terminal-full-command.md`

The feature plan (already being implemented on this same branch by `dev-webpage-ui`) introduces a frontend store signal `activeShellArgs: string[]` sourced from `session.shellArgs`. This bug fix **replaces the semantics**: the signal must carry the effective args and be nullable, with `null` meaning "hide the command block entirely".

### 6.1 `src/shared/types.ts`

Add one field to the `Session` interface (lines 7-23). Insert immediately after `shellArgs: string[];` (line 11):

```ts
  effectiveShellArgs: string[] | null;
```

No other type changes. The trailing `| null` matches the Rust `Option<Vec<String>>` serialization.

### 6.2 `src/terminal/stores/terminal.ts` — DELTA

The feature plan adds:
```ts
const [activeShellArgs, setActiveShellArgs] = createSignal<string[]>([]);
```

**REPLACE with** (nullable, default null):
```ts
const [activeShellArgs, setActiveShellArgs] = createSignal<string[] | null>(null);
```

Everything else in the feature plan (`get activeShellArgs()`, `setActiveSession` signature with `shellArgs?: string[]`) must widen to accept `string[] | null`:

**REPLACE** the feature plan's `setActiveSession` signature with:
```ts
  setActiveSession(
    id: string | null,
    name?: string,
    shell?: string,
    shellArgs?: string[] | null,
    workingDirectory?: string
  ) {
    setActiveSessionId(id);
    if (name !== undefined) setActiveSessionName(name);
    if (shell !== undefined) setActiveShell(shell);
    if (shellArgs !== undefined) setActiveShellArgs(shellArgs);
    if (workingDirectory !== undefined) setActiveWorkingDirectory(workingDirectory);
  },
```

The `if (shellArgs !== undefined)` predicate covers both the `null` case (explicitly "no effective args → hide") and the `string[]` case (known effective args → show). Only `undefined` (rename-only call) skips the update.

### 6.3 `src/terminal/App.tsx` — DELTA

The feature plan threads `session.shellArgs` through the four setting calls and `[]` through the three clearing calls. **REPLACE both**:

- **All 4 setting calls** (lines 40, 54, 80-85, 93-98 per the feature plan) pass `session.effectiveShellArgs` (or `active.effectiveShellArgs`) instead of `session.shellArgs`.
- **All 3 clearing calls** (lines 43, 57, 74 per the feature plan) pass `null` instead of `[]`.
- **Rename call** (line 123 per the feature plan) stays unchanged.

Explicit replacements:

| Site | Feature-plan arg | Bug-fix arg |
|------|------------------|-------------|
| L40 | `session.shellArgs` | `session.effectiveShellArgs` |
| L54 | `active.shellArgs` | `active.effectiveShellArgs` |
| L80-85 (multi-line) | `session.shellArgs,` | `session.effectiveShellArgs,` |
| L93-98 (multi-line) | `session.shellArgs,` | `session.effectiveShellArgs,` |
| L43 | `[]` | `null` |
| L57 | `[]` | `null` |
| L74 | `[]` | `null` |
| L123 (rename) | unchanged | unchanged |

### 6.4 `src/terminal/components/StatusBar.tsx` — DELTA

The feature plan defines:
```tsx
const fullCommand = createMemo(() => {
  const shell = terminalStore.activeShell;
  const args = terminalStore.activeShellArgs;
  if (!shell) return "";
  return args.length > 0 ? `${shell} ${args.join(" ")}` : shell;
});
```

**REPLACE with** (handle null args by returning empty string, which makes the surrounding `<Show when={fullCommand()}>` not render):

```tsx
const fullCommand = createMemo(() => {
  const shell = terminalStore.activeShell;
  const args = terminalStore.activeShellArgs;
  if (!shell || args === null) return "";
  return args.length > 0 ? `${shell} ${args.join(" ")}` : shell;
});
```

Behavior table:

| `shell` | `args` | `fullCommand()` | Rendered? |
|---------|--------|-----------------|-----------|
| `""` | any | `""` | No (hidden) |
| `"bash"` | `null` | `""` | **No** (effective not registered) |
| `"bash"` | `[]` | `"bash"` | Yes (native shell, no args) |
| `"claude-mb"` | `["--continue"]` | `"claude-mb --continue"` | Yes |

No other StatusBar change.

### 6.5 No sidebar changes

The sidebar never renders effective launch command and should continue to display `session.shellArgs` (configured) for its tooltips/shell-type badges. That's correct behavior — sidebar is a session-config view; StatusBar is a live-session view. Do NOT touch `src/sidebar/`.

---

## 7. On-wire representation + empty-state semantics

| Rust | JSON | TS | Render |
|------|------|----|--------|
| `effective_shell_args: None` | `"effectiveShellArgs": null` | `effectiveShellArgs: null` | Hidden |
| `effective_shell_args: Some(vec![])` | `"effectiveShellArgs": []` | `effectiveShellArgs: []` | shell alone |
| `effective_shell_args: Some(vec!["--continue"])` | `"effectiveShellArgs": ["--continue"]` | `effectiveShellArgs: ["--continue"]` | `shell --continue` |

The distinction between `None` (hide) and `Some([])` (show shell alone) is load-bearing — native shells launched via agents without args hit the middle row and must still render the binary. Captured-at-spawn for a native shell like `cmd.exe` with no args → `Some(vec![])` → StatusBar shows `cmd.exe`.

**Clarification**: for native shells launched as `shell: "powershell.exe", shell_args: []`, after `create_session_inner` runs (no injections apply since is_claude=false, is_codex=false), the local `shell_args` is still `[]`, so `effective_shell_args = Some(vec![])`. StatusBar shows `powershell.exe`. ✅ Spec §3 item 3 ("sessions without args: show just the binary").

---

## 8. Testing approach

### 8.1 Manual verification

Dev should verify each of these before handoff:

- [ ] **Claude with prior history**: Start a Claude session at a CWD where `~/.claude/projects/<mangled-cwd>/` exists. Verify StatusBar shows `<shell> ... --continue --append-system-prompt-file <path>` (the appended file path).
- [ ] **Claude without prior history**: Delete `~/.claude/projects/<mangled-cwd>/`, create a fresh session. Verify StatusBar shows the shell+configured args, NO `--continue`.
- [ ] **Restart a Claude session**: Click restart on an existing Claude session. Verify StatusBar shows the configured args **without** `--continue` (skip_auto_resume=true on restart).
- [ ] **Codex fresh session**: Start `codex -m gpt-5`. Verify StatusBar shows `codex resume --last -m gpt-5`.
- [ ] **Codex with explicit `resume`**: Start `codex resume`. Verify NO double-injection (StatusBar shows `codex resume`, not `codex resume resume --last`).
- [ ] **Native shell (powershell.exe)**: Start a native shell session. Verify StatusBar shows just `powershell.exe` (or whatever binary), no injected flags.
- [ ] **Dormant session**: With `start_only_coordinators=true`, restart the app. Non-coordinator sessions appear as dormant (Exited) in the sidebar. Click one and observe: StatusBar command block is **empty** (no text, no shell alone). After clicking "Restart" to spawn it, StatusBar shows the effective command.
- [ ] **Session switch**: Create two sessions with different shells/agents. Switch between them. StatusBar updates each time to the active session's effective command.
- [ ] **cmd.exe wrapper path**: Configure an agent with a non-.exe shell name (e.g. `claude` without extension on Windows). Start a session. StatusBar shows `claude ...` (NOT `cmd.exe /C claude ...` — the platform wrapping is invisible).
- [ ] **Tooltip on long command**: For a command that overflows the StatusBar width, hover to confirm the native `title` tooltip shows the full effective command string.
- [ ] **No regression in sidebar**: Sidebar shell-type display still shows configured args (for agent sessions) — unchanged behavior.
- [ ] `npx tsc --noEmit` passes.
- [ ] `cd src-tauri && cargo check` passes.
- [ ] `cd src-tauri && cargo test` passes (existing `inject_codex_resume_*` and `strip_auto_injected_args_*` tests must still pass; no regressions from the `effective_shell_args` addition).

### 8.2 Unit-testable seams

1. **`SessionInfo::from(&Session)`** — add a test that a `Session` with `effective_shell_args: Some(vec!["--continue".into()])` produces a `SessionInfo` with the matching field. Placement: `src-tauri/src/session/session.rs` inside a new `#[cfg(test)] mod tests` block (file currently has none).

2. **`SessionManager::set_effective_shell_args`** — add a test that verifies the field is set after calling the method, starting from `None`. Placement: new test alongside the existing `set_git_repos_if_gen_rejects_stale_gen` test in `pty/git_watcher.rs:201+` OR a new test module in `session/manager.rs` (preferred — cohesion).

3. **`create_session_inner` injection visibility** — harder to integration-test because it requires Tauri State setup. The existing `inject_codex_resume_*` tests already cover the injection logic itself. The new capture point is a two-line addition whose correctness is trivial to inspect.

**No new test framework or dependencies required.** All additions use the existing `#[tokio::test]` pattern visible in `pty/git_watcher.rs:202`.

---

## 9. Scope recommendation — **keep full scope (all session types)**

The spec gave an escape hatch: "TODAS. excepto que me digas que hay que hacer un quilombo de codigo".

**Verdict**: no quilombo. The design covers all session types uniformly with a single capture point (`create_session_inner` immediately before `pty_mgr.spawn`). The reasons:

1. **Single spawn funnel**: every user-facing PTY-spawning path already flows through `create_session_inner` (verified in §4). No shell-abstraction refactor needed.
2. **Native shells are automatic**: for sessions where `is_claude = false` and `is_codex = false`, the injection blocks at lines 276-366 are no-ops, so the captured local `shell_args` equals the configured one. Spec §4 ("effective == configured, behavior is a no-op") satisfied without branching.
3. **No persistence churn**: `#[serde(skip)]` keeps `effective_shell_args` out of `sessions.toml`, so no migration and no interaction with `strip_auto_injected_args`.
4. **Minimal blast radius**: 3 struct-field additions + 1 manager method + 2 lines inside `create_session_inner` + 1 TS field + ~5 lines of frontend diff (mostly swapping `.shellArgs` for `.effectiveShellArgs` and `[]` for `null`).

**Reduction path (NOT recommended, documented for completeness)**: if for some reason the dev discovers a platform-specific issue (e.g. macOS/Linux spawning quirks I can't foresee from Windows), scope could shrink to "Claude only" by gating the capture call behind `if is_claude { ... }`. This would leave native shells and Codex unchanged (still display configured args), which would reintroduce the exact bug for Codex sessions. **Not worth it** — the proposed full-scope fix is a two-line capture.

---

## 10. What the dev must NOT do

- Do NOT persist `effective_shell_args` to `sessions.toml`. The `#[serde(skip)]` on the `Session` field is load-bearing — on restore, re-injection via `create_session_inner` is the source of truth for the new run. Persisting would bake dynamic flags into the recipe and cause self-perpetuation across restarts (the exact bug `strip_auto_injected_args` was written to prevent).
- Do NOT capture `effective_shell_args` AFTER the `cmd.exe /C` wrapping happens inside `PtyManager::spawn`. Capture BEFORE the `spawn` call, with the logical pre-wrapping vec. The wrapping is a platform quirk the user doesn't want to see.
- Do NOT add a new Tauri event (e.g. `session_effective_args_changed`). The existing `session_created` emit already carries `SessionInfo` and covers every activation path (verified in §4).
- Do NOT modify `strip_auto_injected_args` (config/sessions_persistence.rs:355). It operates on the persisted `shell_args` field (configured recipe), which is unchanged by this fix.
- Do NOT modify the sidebar. StatusBar-only scope.
- Do NOT bump the version. The feature plan (`feature-terminal-full-command.md`) already bumps `0.7.3 → 0.7.4` on this branch; this bug fix ships inside the same release. If `0.7.4` has already been cut to a build before this bug fix lands, bump to `0.7.5` (coordinate with tech-lead / Shipper).
- Do NOT delete, rename, or rework `Session.shell_args`. It remains the persisted configured recipe; `effective_shell_args` is ADDITIVE.
- Do NOT rename `activeShellArgs` → `activeEffectiveShellArgs` in the frontend store. Keep the short name already introduced by the feature plan; change only the type (`string[] | null`) and its data source (`effectiveShellArgs` instead of `shellArgs`). This minimizes diff across the stacked plans and the DELTA is easier to review.

---

## 11. Edge cases

1. **Spawn failure**: if `pty_mgr.spawn` at line 372 returns `Err`, the session still has `effective_shell_args = Some(<attempted vec>)`. The session record exists in the manager with a failed PTY. Existing behavior; not changed by this fix. The StatusBar would briefly show the command before the session is cleaned up by the user.

2. **Context materialization failure** (lines 328-343): `create_session_inner` can `return Err(e)` BEFORE reaching the spawn. In that path, `set_effective_shell_args` is never called, so `effective_shell_args` remains `None`. The session is also auto-destroyed by the error branch (line 336 `destroy_session`). No visibility issue.

3. **Restart race**: `restart_session` (L676) first calls `destroy_session_inner` (L725), then `create_session_inner` (L728). Between the destroy and the new `session_created` emit, the frontend may briefly receive `session_destroyed` + auto-switch to a sibling. The old session's effective args are gone; the new session's are captured and emitted normally. No race hazard.

4. **Back-compat with older frontend/backend pairings**: within the same branch, both ship together. If a dev runs a mismatched pair locally:
   - Newer backend → older frontend: `effectiveShellArgs` field in JSON is ignored; frontend still uses `shellArgs` (old behavior) — no crash.
   - Older backend → newer frontend: JSON lacks the field; TS receives `undefined`. The `fullCommand` memo checks `args === null` but not `undefined`. Fix the memo to handle both:
     ```ts
     if (!shell || args === null || args === undefined) return "";
     ```
     (Updated in §6.4. Treat `null` and `undefined` identically.)

5. **Very long args with spaces**: args like `--prompt "hello world"` are rendered as unquoted join per the feature plan (`args.join(" ")`). Display-only limitation carried forward from the feature plan; tooltip still shows the exact raw string.

6. **Dormant-then-activated session**: user clicks a dormant session's "Restart" button. `restart_session` runs, `create_session_inner` captures effective args, emits `session_created` with populated field. Frontend re-fetches on switch and displays correctly. ✅

7. **Two sessions with the same shell+configured-args but different effective args** (e.g. one with `--continue`, one without because its `~/.claude/projects/` dir was deleted): each session carries its own `effective_shell_args` captured at its spawn time. StatusBar correctly distinguishes on switch. ✅

8. **Settings change mid-session**: user edits an agent's `command` in settings while a session is running. Current session's `effective_shell_args` was captured at the original spawn time — unchanged. A restart would re-capture from the new settings. This matches the mental model "effective args = what was passed to this PTY at its spawn" and is intentional.

---

## 12. Files touched (summary)

**Backend (Rust)**:
- `src-tauri/src/session/session.rs` — add field to `Session` + `SessionInfo`, extend `impl From<&Session> for SessionInfo`. ~8 lines.
- `src-tauri/src/session/manager.rs` — init field in `create_session`, add `set_effective_shell_args` method. ~12 lines.
- `src-tauri/src/commands/session.rs` — 2 lines inserted in `create_session_inner` immediately before `pty_mgr.spawn`. ~2 lines.
- *(Optional)* new tests in `session/session.rs` and `session/manager.rs`. ~30 lines.

**Frontend (TS/SolidJS)**:
- `src/shared/types.ts` — 1 line added to `Session` interface.
- `src/terminal/stores/terminal.ts` — type widening (`string[] | null`), signal init null. ~3 lines changed from the feature-plan baseline.
- `src/terminal/App.tsx` — swap 4 identifiers (`shellArgs` → `effectiveShellArgs`) + 3 literals (`[]` → `null`). ~7 lines changed from the feature-plan baseline.
- `src/terminal/components/StatusBar.tsx` — 1 line changed in the `fullCommand` memo. ~1 line.

**Persistence / CSS / Sidebar / Titlebar**: untouched.

**Total estimated diff**: ~60 lines incl. tests. Very contained.

---

## 13. Dev-Rust enrichment

Appended after reviewing the plan against branch `feature/terminal-full-command` at tip `70743eb`. The architect's analysis is sound; the items below are verifications, clarifications, and a handful of corner cases the grinch should be aware of before implementing.

### 13.1 Line-number verification (tip `70743eb`)

All cited line numbers confirmed against the current tip. No drift. Grinch can trust the plan's offsets.

| Plan cite | Actual | Status |
|-----------|--------|:------:|
| `commands/session.rs:226-237` (`mgr.create_session(...)`) | L226-237 | ✅ |
| `commands/session.rs:249` (`let mut shell_args = shell_args;`) | L249 | ✅ |
| `commands/session.rs:276-314` (Claude `--continue`) | L276-314 | ✅ |
| `commands/session.rs:316-322` (Codex `resume --last`) | L316-322 | ✅ |
| `commands/session.rs:328-343` (context error path) | L327-343 | ✅ |
| `commands/session.rs:350-366` (Claude `--append-system-prompt-file`) | L349-366 | ✅ (off-by-1 comment, identical block) |
| `commands/session.rs:368-372` (`pty_mgr.spawn`) | L368-372 | ✅ |
| `commands/session.rs:438-439` (emit `session_created`) | L438-439 | ✅ |
| `session/session.rs:41-81` (`Session`) | L41-81 | ✅ |
| `session/session.rs:95-118` (`SessionInfo`) | L93-118 | ✅ (doc comment L92 shifts numbering; struct body identical) |
| `session/session.rs:120-141` (`impl From<&Session>`) | L120-141 | ✅ |
| `session/manager.rs:26-76` (`create_session`) | L26-76 | ✅ |
| `session/manager.rs:236-241` (`set_is_claude`) | L236-241 | ✅ |
| `pty/manager.rs:173-187` (cmd.exe /C wrapping) | L173-187 | ✅ |
| `lib.rs:543` (dormant path) | L543 | ✅ |
| `lib.rs:575` (normal restore) | L575 | ✅ |
| `commands/session.rs:711-712` (restart strip_auto_injected_args) | L711-712 | ✅ |
| `commands/session.rs:728` (restart → create_session_inner) | L728 | ✅ |
| `commands/session.rs:1100` (root agent → create_session_inner) | `create_root_agent_session` defined L1021, routes to create_session_inner | ✅ |

### 13.2 Confirmation: pre-wrap capture is genuinely clean

Per §5.3 the plan captures BEFORE `pty_mgr.spawn`. Verified by reading `pty/manager.rs:173-187`: the Windows `cmd.exe /C` wrapping builds a fresh local `CommandBuilder` and copies args into it via `c.arg(arg)` — the caller's `&[String]` slice is NEVER mutated. So capturing `shell_args.clone()` right before the call yields the exact pre-wrapping vec the user wants to see, regardless of whether the wrapping branch fires. No risk of leaking `cmd.exe /C` into the StatusBar.

**Why this matters**: the alternative (capture AFTER spawn, or capture from inside `PtyManager::spawn`) would pollute the wire with platform detail. The plan's placement is correct and defensible.

### 13.3 `inject_codex_resume` signature — capture shape already correct

`inject_codex_resume` at `commands/session.rs:132` takes `shell_args: &mut Vec<String>` and mutates in place (inserts `resume` and `--last` at index 0 in direct-exec, or into the appropriate slot in `cmd.exe` wrapper paths). So by the time we reach the capture point, `shell_args` already reflects Codex injection AND Claude `--continue` AND `--append-system-prompt-file` — all three injection mechanisms (direct push, `&mut Vec` helper, and in-place `*last = format!(...)` on the cmd-path) converge on the same local binding. The single capture `mgr.set_effective_shell_args(id, shell_args.clone()).await` captures all of them uniformly. No per-injection gating needed.

**Why this matters**: confirms §5.3's "single capture point" claim is robust against the three heterogeneous injection styles.

### 13.4 Restart flow — `--append-system-prompt-file` IS still injected on restart

The architect's §4 table correctly marks restart with `skip_auto_resume=true`. But note: `skip_auto_resume` only gates the **resume** injections (Claude `--continue` at L289 and Codex `resume --last` at L316). The Claude `--append-system-prompt-file` injection at L349-366 is NOT behind `skip_auto_resume` — it only checks `is_claude && materialized_context_path.is_some()`. This is intentional: `--append-system-prompt-file` materializes the current CLAUDE.md every spawn and is not "resume" semantics.

**Correction to §8.1 manual test #3**: the restart case should read:

> **Restart a Claude session**: Click restart on an existing Claude session with a `repo-*` repo that has CLAUDE.md. Verify StatusBar shows configured args + `--append-system-prompt-file "<path>"` BUT NOT `--continue`. If the session's CWD has no CLAUDE.md, `--append-system-prompt-file` is also absent — just the configured args.

**Why this matters**: without this clarification, a tester might flag the presence of `--append-system-prompt-file` on restart as a regression.

### 13.5 Concurrency/locking — established pattern, no new risk

The proposed `mgr.set_effective_shell_args(id, shell_args.clone()).await` sits inside the scope of the outer `mgr` read-guard (held from L225 through the `pty_mgr.spawn` call). Two lock levels:

1. **Outer** `Arc<tokio::sync::RwLock<SessionManager>>` — held as read-guard via `let mgr = session_mgr.read().await;`
2. **Inner** `Arc<RwLock<HashMap<Uuid, Session>>>` (field `sessions` of `SessionManager`) — `set_effective_shell_args` acquires write.

These are independent locks. No deadlock risk — same topology as the existing `mgr.set_is_claude(id, true).await` at L272 which has shipped to users. Tokio's `RwLock` permits guards crossing await points (unlike `std::sync::RwLock`). The guard is already held across `pty_mgr.spawn`, so the new `set_effective_shell_args` adds a negligible extra await under the same guard.

**Why this matters**: grinch can skip lock-analysis; the capture is architecturally identical to `set_is_claude`.

### 13.6 `SessionInfo::from` call-site inventory — all four automatically covered

`SessionInfo::from(&Session)` is invoked in exactly four places on the tree; all of them will ship the new `effective_shell_args` field without any additional code:

1. `commands/session.rs:438` — emit `session_created` from `create_session_inner` (primary path)
2. `lib.rs:558` — emit `session_created` for dormant sessions during startup restore
3. `session/manager.rs:157` — `list_sessions()` (used by frontend re-fetch)
4. `session/manager.rs:367` — `find_by_token()` (used by CLI/web auth)

For #3 and #4, the field value depends on whether the capture point has executed:
- Sessions whose PTY was spawned via `create_session_inner` → `Some(vec)`.
- Dormant sessions and spawn-failed sessions → `None`.
- Never-serialized back to TOML (because `#[serde(skip)]` on `Session.effective_shell_args`).

**Why this matters**: confirms §5.4's "no new IPC event needed" — the existing four emit/list call sites fan the field out to every consumer automatically. No additional backend work.

### 13.7 Serde attribute interaction with struct-level `rename_all = "camelCase"`

Both `Session` and `SessionInfo` carry `#[serde(rename_all = "camelCase")]` at the struct level. The new `effective_shell_args` field will wire-serialize as `effectiveShellArgs` (matching the TS contract in §6.1) automatically.

Field-level attributes compose cleanly with `rename_all`:
- `#[serde(skip)]` on `Session.effective_shell_args` → field is neither serialized nor deserialized. If `Session` is ever deserialized (e.g. in a future test fixture), the field defaults to `None` via `Option::default()`. No manual `Default` impl needed because `Option<T>: Default`.
- `#[serde(default)]` on `SessionInfo.effective_shell_args` → on a hypothetical deserialize of a `SessionInfo` payload missing the field, default is `None`. `SessionInfo` is IPC-only today, but the attribute is future-proof.

**Why this matters**: grinch doesn't need to write a custom `Default` impl or worry about camelCase overrides.

### 13.8 `PersistedSession` is a separate struct — zero persistence contamination

`strip_auto_injected_args` (config/sessions_persistence.rs:355) operates on `PersistedSession.shell_args`, a struct distinct from `Session` in `session/session.rs`. The new field lives on `Session` and `SessionInfo` only. `PersistedSession` is not modified, so no interaction with the strip logic or any existing `strip_auto_injected_args_*` test.

Confirmed grep: `PersistedSession` has no `effective_shell_args` field and no persistence path adds one. §10's "do NOT persist" rule is enforced by construction (different struct, not just a missing field on the same struct).

### 13.9 Spawn-failure edge case — pre-existing behavior, acknowledged

§11 item 1 correctly notes: if `pty_mgr.spawn` at L372 returns Err, the session record survives in the manager (no cleanup inside `create_session_inner`). With the fix, that survivor would carry `effective_shell_args = Some(<attempted vec>)` and `session_created` would NOT be emitted (the `?` returns early before L438-439).

Verified: `create_session_inner` does NOT call `destroy_session` on spawn failure — the session lingers as a zombie until the user manually acts. **Pre-existing behavior, out of scope for this bug fix.** Grinch should NOT fix the zombie-on-spawn-failure path as part of this change — it's a separate concern and the architect's plan explicitly puts it out of scope.

**Why this matters**: grinch might be tempted to add a `destroy_session` on the error path "while in the neighborhood". Resist. Scope creep.

### 13.10 Test-module placement — both files lack `#[cfg(test)]` today

- `session/session.rs` has **no** `#[cfg(test)] mod tests` block. Adding one at the bottom is clean — no reorganization needed. Constructing a `Session` literal takes ~15 fields; suggest a private `fn sample_session(effective: Option<Vec<String>>) -> Session` helper inside the test mod to keep individual tests terse.
- `session/manager.rs` has **no** `#[cfg(test)] mod tests` block either. Adding one for `set_effective_shell_args` requires a `tokio::test` since the method is async. The test needs `SessionManager::new()` + `create_session(...)` + `set_effective_shell_args(...)` + `get_session(...)`-then-assert.

Architect's cross-module placement suggestion (`pty/git_watcher.rs:201+` as alternative) would work but is less cohesive — keep tests next to their production code.

### 13.11 `cargo build` in addition to `cargo check`

§8.1 lists `cargo check` and `cargo test`. Suggest adding `cargo build` once before commit — `check` is usually sufficient for this change (no new dependencies, no linker-sensitive code), but a full build catches e.g. `#[serde(skip)]` interactions with derive macros that `check` occasionally misses in incremental mode. Cheap insurance (~30s on warm cache on this repo).

### 13.12 Capture-shape example — plan example is minimal, real-world example is longer

Plan §5.3 shows: `claude-mb --dangerously-skip-permissions --effort max --continue`.

A realistic Claude session (existing conversation + context available) produces, after all three injection points:
```
claude-mb --dangerously-skip-permissions --effort max --continue --append-system-prompt-file "C:\Users\maria\...\CLAUDE.md"
```

For a Codex session with no explicit subcommand:
```
codex resume --last -m gpt-5
```

Worth noting for StatusBar width/truncation testing — realistic effective-command strings on Windows can exceed 150 chars with a full context-file path. The feature plan's existing `title` tooltip handles overflow via browser-native tooltip; confirm at manual-test time.

### 13.13 `SessionInfo` field-order footgun that ISN'T a footgun

The plan inserts `effective_shell_args` after `shell_args` on both `Session` and `SessionInfo`. `serde_json` serializes struct fields in declaration order. Nothing in the frontend depends on field order, and nothing in the backend does either — so declaration-order placement is cosmetic.

One mild preference: on `SessionInfo` insert the field IMMEDIATELY after `shell_args` (declaration order L99 → L100 in the new layout) so `shellArgs` and `effectiveShellArgs` sit adjacent on the wire for human-readable debugging. Plan already does this. ✅ just reaffirming.

### 13.14 `SessionInfo::from` implementation — only one line changes

§5.1.c asks to add `effective_shell_args: s.effective_shell_args.clone(),` inside the struct literal. That's one line. `SessionInfo::from` then contains 16 field-initialization lines — no other logic touched. No `From` change needed anywhere else in the tree (verified: `SessionInfo` has exactly one `impl From` for it, at `session/session.rs:120`).

### 13.15 Summary of enrichment impact on dev-rust-grinch's work

Nothing in this enrichment changes the code the grinch will write. The plan's §5 code blocks are correct. The enrichment:

- Confirms every file:line cite against the branch tip.
- Clarifies one manual-test expectation (§13.4 — restart DOES retain `--append-system-prompt-file`).
- Removes ambiguity about three injection-style convergence (§13.3).
- Removes ambiguity about locking (§13.5).
- Confirms no persistence-struct touch needed (§13.8).
- Flags a scope-creep temptation (§13.9 — spawn-failure cleanup).
- Offers test-placement details (§13.10) and one extra verification command (§13.11).

**Verdict**: the architect's plan is implementable as-written. No sections rewritten, no disagreements.
