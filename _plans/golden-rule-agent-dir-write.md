# Plan: Extend GOLDEN RULE to Allow Writes in Agent's Own Replica Dir

**Branch:** `feature/golden-rule-agent-dir-write`
**Status:** Draft
**Created:** 2026-04-12

---

## Problem

The current GOLDEN RULE (in `session_context.rs:373-381`) only permits writes to repositories whose root folder starts with `repo-`. Agents legitimately need to write inside their own replica directory (`__agent_*` or `_agent_*` folders) for plans scratch, role drafts, personal notes, and session artifacts like `last_ac_context.md`. Without an exception, the rule forbids this even though such writes are safe and expected.

## Goal

Allow each agent to modify files inside its own replica directory, while preserving the existing guarantee that non-`repo-*` repositories stay untouched.

---

## Ambiguity Resolution

The tech-lead flagged two approaches:

### Approach A — Pattern-based (static)
Allow writes to any folder matching `_agent_*` or `__agent_*`. One-line change to `default_context()`, no plumbing.

**Downside:** any agent can write into any other agent's dir (e.g., `dev-rust` could corrupt `dev-rust-grinch`'s notes).

### Approach B — Dynamic (per-session agent root)
Inject the specific agent's absolute root path into the context. Agent X can write only to its own dir, not siblings'.

**Downside:** requires `ensure_global_context()` and `default_context()` to accept an agent root parameter, plus per-agent context file naming to avoid races.

### Recommendation: **Approach B (Dynamic)**

Rationale:
- Matches the tech-lead's stated preference ("more restrictive and safer").
- Prevents cross-agent writes — agents in the same workgroup should not accidentally overwrite each other's work.
- The plumbing is modest: only one function signature changes (`ensure_global_context(cwd: &str)`), and `build_replica_context()` already has `cwd: &str` available.
- Race condition with shared `AgentsCommanderContext.md` is resolved by writing a per-agent copy (`AgentsCommanderContext-{hash}.md`) inside the existing `context-cache/` dir.

Approach A is documented as a fallback in §5 in case the dynamic plumbing raises concerns during review.

---

## 1. Rust Source Changes

### 1.1 `src-tauri/src/config/session_context.rs`

#### 1.1.1 Make `default_context` accept the agent root

**Current (line 367):**
```rust
fn default_context() -> String {
    String::from(
r#"# AgentsCommander Context
...
```

**Proposed:**
```rust
fn default_context(agent_root: &str) -> String {
    format!(
r#"# AgentsCommander Context
...
"#,
        agent_root = agent_root,
    )
}
```

#### 1.1.2 Update the GOLDEN RULE block

**Current (lines 373-381):**
```markdown
## GOLDEN RULE — Repository Write Restrictions

**ABSOLUTE AND NON-NEGOTIABLE:** You may ONLY modify repositories whose root folder name starts with `repo-`. If a repository's root folder does NOT begin with `repo-`, you MUST NOT modify it — no file edits, no file creation, no file deletion, no git commits, no branch creation, no git operations that alter state.

- **Allowed**: Read-only operations on ANY repository (reading files, searching, git log, git status, git diff)
- **Allowed**: Full read/write operations on repositories inside `repo-*` folders
- **FORBIDDEN**: Any write operation on repositories NOT inside `repo-*` folders

If instructed to modify a non-`repo-` repository, REFUSE the modification and explain this restriction. There are NO exceptions to this rule.
```

**Proposed replacement:**
```markdown
## GOLDEN RULE — Write Restrictions

**ABSOLUTE AND NON-NEGOTIABLE:** You may ONLY modify files in two places:

1. **Repositories whose root folder name starts with `repo-`** (e.g. `repo-AgentsCommander`, `repo-myapp`). These are the working repos you are meant to edit.
2. **Your own agent replica directory and its subdirectories** — your assigned root:
   ```
   {agent_root}
   ```
   Use this for plans scratch, personal notes, role drafts, and session artifacts. Do NOT write into other agents' replica directories.

Any repository or directory outside the two places above is READ-ONLY.

- **Allowed**: Read-only operations on ANY path (reading files, searching, git log, git status, git diff)
- **Allowed**: Full read/write inside `repo-*` folders
- **Allowed**: Full read/write inside your own replica root ({agent_root}) and its subdirectories
- **FORBIDDEN**: Any write operation outside those two zones — including other agents' replica directories, the workspace root, parent project dirs, user home files, or arbitrary paths on disk

**Clarification on git operations:** Your replica directory is typically inside a parent repository's `.ac-new/` folder, which is `.gitignore`d. Do NOT run `git` commands that alter state (commit, branch, reset, etc.) from inside your replica directory — that would affect the parent repo unintentionally. `git status`, `git log`, `git diff` are fine.

If instructed to modify a path outside these zones, REFUSE and explain this restriction. There are NO exceptions.
```

The `{agent_root}` placeholders are filled by `format!` at runtime. The field will contain a real absolute path like `C:\Users\maria\0_repos\agentscommander\.ac-new\wg-2-dev-team\__agent_tech-lead`.

#### 1.1.3 Update `ensure_global_context` to accept agent root

**Current (lines 6-18):**
```rust
pub fn ensure_global_context() -> Result<String, String> {
    let config_dir = super::config_dir()
        .ok_or_else(|| "Could not resolve app config directory".to_string())?;
    let file_path = config_dir.join("AgentsCommanderContext.md");

    std::fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create config dir: {}", e))?;
    std::fs::write(&file_path, &default_context())
        .map_err(|e| format!("Failed to write AgentsCommanderContext.md: {}", e))?;
    log::info!("Refreshed global AgentsCommanderContext.md at {:?}", file_path);

    Ok(file_path.to_string_lossy().to_string())
}
```

**Proposed:**
```rust
/// Writes a per-agent copy of AgentsCommanderContext.md with the agent's own
/// root path interpolated into the GOLDEN RULE. Uses a deterministic filename
/// based on the cwd to prevent races between concurrent session launches.
pub fn ensure_global_context(agent_root: &str) -> Result<String, String> {
    let config_dir = super::config_dir()
        .ok_or_else(|| "Could not resolve app config directory".to_string())?;
    let context_dir = config_dir.join("context-cache");
    std::fs::create_dir_all(&context_dir)
        .map_err(|e| format!("Failed to create context-cache dir: {}", e))?;

    let hash = simple_hash(agent_root);
    let file_path = context_dir.join(format!("ac-context-{}.md", hash));

    std::fs::write(&file_path, &default_context(agent_root))
        .map_err(|e| format!("Failed to write per-agent AgentsCommanderContext.md: {}", e))?;
    log::info!(
        "Refreshed per-agent AgentsCommanderContext.md for {} → {:?}",
        agent_root, file_path
    );

    Ok(file_path.to_string_lossy().to_string())
}
```

**Notes:**
- The global shared file at `<config_dir>/AgentsCommanderContext.md` is no longer written by this function. Two options:
  - **(a)** Keep writing it with a generic (pattern-based) rule for external readers (Codex `developer_instructions`, docs, etc.).
  - **(b)** Remove entirely — no known consumers besides `ensure_codex_context()`.
- **Recommendation:** Option (a). `ensure_codex_context()` calls this function for a user-level config injection; Codex `developer_instructions` is shared across all Codex sessions on this machine, so a per-agent path is inappropriate there. Either keep a generic static version for Codex, or pass `"<YOUR_OWN_REPLICA_ROOT>"` as a placeholder when called by Codex.
- **Decision:** Split into two helpers:
  - `ensure_session_context(agent_root: &str) -> Result<String, String>` — per-agent, new path, used by replica launch.
  - `ensure_global_context_generic() -> Result<String, String>` — static, used by Codex. Uses a generic "your own replica directory" wording, no specific path.

#### 1.1.4 Add `ensure_global_context_generic` and update `ensure_codex_context`

**Proposed new helper:**
```rust
/// Writes the shared AgentsCommanderContext.md with generic (non-per-agent)
/// wording in the GOLDEN RULE — used for Codex developer_instructions, which
/// is shared across all Codex sessions on the machine.
pub fn ensure_global_context_generic() -> Result<String, String> {
    let config_dir = super::config_dir()
        .ok_or_else(|| "Could not resolve app config directory".to_string())?;
    let file_path = config_dir.join("AgentsCommanderContext.md");

    std::fs::create_dir_all(&config_dir)
        .map_err(|e| format!("Failed to create config dir: {}", e))?;
    std::fs::write(&file_path, &default_context_generic())
        .map_err(|e| format!("Failed to write AgentsCommanderContext.md: {}", e))?;
    log::info!("Refreshed global (generic) AgentsCommanderContext.md at {:?}", file_path);

    Ok(file_path.to_string_lossy().to_string())
}

fn default_context_generic() -> String {
    // Same body as default_context(), but with a generic placeholder for the
    // replica root. Codex sessions read their own root from the Session
    // Credentials block (also injected into the context).
    default_context("<YOUR OWN REPLICA ROOT — see Session Credentials below>")
}
```

**Update `ensure_codex_context` (line 33):**
```rust
// BEFORE:
let context_path = ensure_global_context()?;

// AFTER:
let context_path = ensure_global_context_generic()?;
```

#### 1.1.5 Update `build_replica_context` to pass cwd

**Current (line 281):**
```rust
if raw == CONTEXT_TOKEN_GLOBAL {
    let global_path = ensure_global_context()?;
```

**Proposed:**
```rust
if raw == CONTEXT_TOKEN_GLOBAL {
    let global_path = ensure_session_context(cwd)?;
```

`cwd` is already the function parameter (line 249), so no new plumbing is needed here.

### 1.2 `src-tauri/src/commands/session.rs` line 118

**Current:**
```rust
Ok(None) => {
    // No replica context[] — use global context only
    match crate::config::session_context::ensure_global_context() {
```

**Proposed:**
```rust
Ok(None) => {
    // No replica context[] — use per-agent context only
    match crate::config::session_context::ensure_session_context(&cwd) {
```

`cwd` is already in scope (the session cwd).

### 1.3 Function renaming summary

| Old name | New name | Signature |
|----------|----------|-----------|
| `ensure_global_context` | `ensure_session_context` | `(cwd: &str) -> Result<String, String>` |
| (new) | `ensure_global_context_generic` | `() -> Result<String, String>` |
| `default_context` | `default_context` | `(agent_root: &str) -> String` |
| (new) | `default_context_generic` | `() -> String` |

---

## 2. External Documentation Changes

### 2.1 `C:\Users\maria\0_repos\agentscommander\ROLE_AC_BUILDER.md` line 226

**⚠ OUTSIDE `repo-*` — cannot be modified by the implementing dev agent under the current GOLDEN RULE.**

The path `C:\Users\maria\0_repos\agentscommander\ROLE_AC_BUILDER.md` lives in the `agentscommander` folder, which does NOT start with `repo-`. Per the current rule, no agent may edit this file.

**Resolution options:**
- **(Recommended)** The user edits this file manually once the Rust change lands — it's a single descriptive line, not a functional dependency. Suggested new wording:
  > "Repo prefix: Cloned repos inside workgroups use `repo-` prefix (e.g., `repo-AgentsCommander`). This is critical — the golden rule allows write access to `repo-*` folders AND to each agent's own replica directory."
- **(Alternative)** Move `ROLE_AC_BUILDER.md` into a `repo-*` folder in a separate commit before this plan is implemented.

The implementing dev agent MUST flag this in its completion report rather than silently skipping it.

### 2.2 Individual Role.md files inside `__agent_*` / `_agent_*`

Searched across the workgroup — only one `Role.md` file exists (`repo-AgentsCommander/.ac-new/_agent_shipper/ROLE.md`). It lives inside a `repo-*` folder and may reference the GOLDEN RULE.

**Action for the dev agent:**
- Read `repo-AgentsCommander/.ac-new/_agent_shipper/ROLE.md`.
- If it restates the GOLDEN RULE verbatim, update to match the new wording.
- If it only references the rule by name ("per the GOLDEN RULE"), leave it alone — it will pick up the new rule via context injection.

No `last_ac_context.md` files need updating; those are session artifacts regenerated on next launch.

---

## 3. Implementation Sequence

| Step | File | Change |
|------|------|--------|
| 1 | `src-tauri/src/config/session_context.rs` | Refactor `default_context()` to accept `agent_root: &str` and interpolate |
| 2 | `src-tauri/src/config/session_context.rs` | Rewrite the GOLDEN RULE block with the new two-zone wording and `{agent_root}` placeholder |
| 3 | `src-tauri/src/config/session_context.rs` | Rename `ensure_global_context` → `ensure_session_context(cwd: &str)`; write to per-hash file in `context-cache/` |
| 4 | `src-tauri/src/config/session_context.rs` | Add `ensure_global_context_generic()` and `default_context_generic()` |
| 5 | `src-tauri/src/config/session_context.rs` | Update `build_replica_context` line 281 to call `ensure_session_context(cwd)` |
| 6 | `src-tauri/src/config/session_context.rs` | Update `ensure_codex_context` line 33 to call `ensure_global_context_generic()` |
| 7 | `src-tauri/src/commands/session.rs` | Update line 118 call site to `ensure_session_context(&cwd)` |
| 8 | `repo-AgentsCommander/.ac-new/_agent_shipper/ROLE.md` | Check for GOLDEN RULE reference; update wording if it restates it verbatim |
| 9 | Verify | `cargo check` passes |
| 10 | Verify | Launch a Claude session; confirm `last_ac_context.md` in the agent's cwd contains the new per-agent rule with the absolute agent root |
| 11 | Report | Flag that `ROLE_AC_BUILDER.md` line 226 could not be modified (outside `repo-*`), suggest user update wording |

---

## 4. Files Modified

| File | Change | Can dev-agent modify? |
|------|--------|------------------------|
| `src-tauri/src/config/session_context.rs` | Function rename, signature change, new helpers, rule body rewrite | YES (`repo-*`) |
| `src-tauri/src/commands/session.rs` | Call-site update (1 line) | YES (`repo-*`) |
| `repo-AgentsCommander/.ac-new/_agent_shipper/ROLE.md` | Check + conditional wording update | YES (`repo-*`) |
| `C:\Users\maria\0_repos\agentscommander\ROLE_AC_BUILDER.md` | Wording update | **NO** — outside `repo-*`, user must update |

**No new files. No new structs. No new Tauri commands.**

---

## 5. Fallback: Approach A (Pattern-only)

If the dynamic plumbing is rejected during review, the minimal alternative is a single-string update to `default_context()`:

```markdown
## GOLDEN RULE — Write Restrictions

**ABSOLUTE AND NON-NEGOTIABLE:** You may ONLY modify files in:

1. Repositories whose root folder name starts with `repo-`.
2. Directories named `_agent_*` or `__agent_*` (agent replica directories), **especially your own**.

All other paths are READ-ONLY.
...
```

This is a ~5-line change to `default_context()`, zero plumbing, no new helpers, no Codex-context split. Trade-off: one agent can write into another agent's dir (mitigated only by the agent's reading comprehension of "especially your own").

---

## 6. Validation

| Test | Expected |
|------|----------|
| Launch a Claude session at `__agent_tech-lead` | `last_ac_context.md` in that dir contains the absolute path `…\__agent_tech-lead` in the GOLDEN RULE body |
| Launch a second Claude session at `__agent_shipper` concurrently | Each session's `last_ac_context.md` has its OWN agent root, no cross-contamination |
| Launch a Codex session | `~/.codex/config.toml` `developer_instructions` contains the GENERIC rule (no specific path — the placeholder text) |
| `cargo check` / `cargo clippy` | Clean — no unused-function warnings for the renamed helpers |
| Existing replica with `$AGENTSCOMMANDER_CONTEXT` token in `context[]` | Resolves to a per-agent file under `context-cache/ac-context-{hash}.md` |
| Agent attempts to write into a sibling `__agent_*` dir | Agent refuses per the updated rule |
| Agent attempts to write into its own `__agent_*` dir | Allowed |

---

## 7. Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Race between concurrent session launches on the same agent | Deterministic per-agent filename via `simple_hash(cwd)` — each agent has its own file. No race possible between different agents. Re-launches of the same agent overwrite with identical content. |
| Codex sessions miss the specific agent path | By design — Codex's `developer_instructions` is user-level and shared across all sessions on this machine. The credentials block injected per-session provides the per-agent root separately. |
| Function rename breaks external callers | Searched all callers — only two call sites: `commands/session.rs:118` and `session_context.rs:33` (internal). Both updated in this plan. |
| `ROLE_AC_BUILDER.md` remains out of sync until user updates manually | Dev agent must explicitly report this. The file is documentation, not operational — drift is low-risk short-term. |
| Agents misinterpret "your own replica directory" | The new rule embeds the literal absolute path inline — no interpretation needed. |

---

## Dev-Rust Review

**All line numbers and code references verified against current HEAD.** The plan is implementable as written. Findings below.

### Verified ✅

| Claim | Actual | Status |
|-------|--------|--------|
| `default_context()` at line 367 | Line 367 | ✅ |
| GOLDEN RULE block at lines 373-381 | Lines 373-381 | ✅ |
| `ensure_global_context` at lines 6-18 | Lines 6-18 | ✅ |
| `ensure_codex_context` call at line 33 | Line 33 | ✅ |
| `build_replica_context` call at line 281 | Line 281 | ✅ |
| `commands/session.rs` call at line 118 | Line 118 | ✅ |
| `simple_hash()` exists | Line 358, already used at lines 147 and 342 | ✅ |
| `cwd` available in `build_replica_context` | Function param at line 249 | ✅ |
| `cwd` available in `create_session_inner` | Function param (line 28) | ✅ |
| No literal `{` or `}` in current template | Confirmed — all placeholders use `<ANGLE_BRACKETS>` | ✅ `format!()` is safe |
| Per-hash file naming is race-safe | Different agents → different hashes → different files. Same agent → same hash → identical content overwrite | ✅ |
| Shipper ROLE.md references GOLDEN RULE | Does NOT mention it — build/deploy only. No update needed | ✅ |

### Issue 1 — Risk assessment undercounts call sites

**§7 Risk Assessment** says: "Searched all callers — only two call sites: `commands/session.rs:118` and `session_context.rs:33` (internal)."

There are actually **three** call sites:
1. `session_context.rs:33` (inside `ensure_codex_context`) — handled by §1.1.4
2. `session_context.rs:281` (inside `build_replica_context`) — handled by §1.1.5
3. `commands/session.rs:118` (inside `create_session_inner`) — handled by §1.2

The implementation sequence correctly addresses all three. The risk table just has a counting error. **Not a functional gap** — all call sites are covered in §1.1.4, §1.1.5, and §1.2.

### Issue 2 — Parameter naming inconsistency

§1.1.3 defines the new signature as `ensure_global_context(agent_root: &str)`, but §1.3 (renaming summary) says `ensure_session_context(cwd: &str)`. The function is called with `cwd` from `build_replica_context` and `&cwd` from `create_session_inner`.

**Recommendation:** Use `agent_root` as the parameter name for `ensure_session_context` AND `default_context` — it's more explicit about what the value represents. `cwd` is the caller's variable name, not the callee's concern. At the call sites, pass `cwd` into the `agent_root` parameter:

```rust
// In build_replica_context:
let global_path = ensure_session_context(cwd)?;

// In create_session_inner:
match crate::config::session_context::ensure_session_context(&cwd) {
```

Both are valid Rust — the variable name doesn't need to match the parameter name. Not blocking, but should be resolved before implementation.

### Issue 3 — Dead function `global_context_path()` (line 21-22)

`pub fn global_context_path() -> Option<PathBuf>` is defined at line 21 but has **zero callers** anywhere in the codebase. It returns the path to `AgentsCommanderContext.md` — which will still be written by `ensure_global_context_generic()`.

**Recommendation:** Leave it. It's pre-existing dead code. After the refactor, it still returns a valid path (the generic file). Cleaning it up would be scope creep. But if clippy flags it as unused, the implementer should be prepared to either add `#[allow(dead_code)]` or remove it.

### Issue 4 — Codex path: `ensure_codex_context` reads file content, not just path

At lines 33-35, `ensure_codex_context` calls `ensure_global_context()` then reads the file:
```rust
let context_path = ensure_global_context()?;
let context_content = std::fs::read_to_string(&context_path)...
```

After the rename to `ensure_global_context_generic()`, the written file will contain the generic placeholder `<YOUR OWN REPLICA ROOT — see Session Credentials below>` instead of a real path. This text gets injected into `~/.codex/config.toml` as `developer_instructions`.

**This is correct by design** — Codex's `developer_instructions` is user-level and shared across all sessions on the machine. The actual agent root comes from the Session Credentials block (injected separately per session). No issue here, just confirming the plan's reasoning is sound.

### Note: `format!()` safety

The current `default_context()` uses `String::from(r#"..."#)`. Changing to `format!(r#"..."#, agent_root = agent_root)` requires that no literal `{` or `}` appear in the template. I confirmed the entire template (lines 369-442) uses only `<ANGLE_BRACKETS>` for placeholders and contains zero literal braces. The `format!()` change is safe without any escaping.

### Summary

The plan is correct and ready for implementation. Three call sites are all handled (despite the risk table counting error). The function rename, per-hash naming, Codex split, and GOLDEN RULE rewrite are all sound. Resolve the `agent_root` vs `cwd` parameter naming before implementation. The shipper ROLE.md needs no changes.

---

## Grinch Review

**VERDICT: APPROVED — no blocking issues. One concrete recommendation (non-blocking), rest are verified-clean.**

### Security Analysis: Can an Agent Exploit the New Rule?

The GOLDEN RULE is a prompt-level instruction, not an enforcement mechanism. "Exploitation" means: can an agent misinterpret the rule text in a way that grants unintended write access?

**1. `{agent_root}` injection is safe.** The value is the `cwd` parameter from `create_session_inner` (session.rs:28). This is set by the app during session creation — via session-request, user UI, or restore — never by the agent itself. An agent cannot modify its own `cwd` after launch. No injection vector exists.

**2. `format!()` is safe.** Independent verification: the raw string at lines 369-442 contains zero literal `{` or `}` characters (only `<ANGLE_BRACKETS>` for placeholders). Named argument `{agent_root}` appears twice in the proposed replacement (code block + bullet point). Rust's `format!` does not do recursive formatting — braces in the VALUE are output literally. Even if `cwd` contained `{` or `}` (legal in Windows paths), the format call would produce correct output.

**3. The rule wording is airtight.** Two allowed zones are explicitly enumerated. The "FORBIDDEN" bullet is comprehensive (lists sibling agents, workspace root, parent dirs, user home). The injected path is a literal absolute path — no interpretation required. An agent would need to deliberately ignore the rule, not misunderstand it.

**4. Git clarification paragraph is correct and necessary.** Agents in `.ac-new/` subdirectories running `git commit` would affect the parent repo. The paragraph warns: "Do NOT run `git` commands that alter state from inside your replica directory — that would affect the parent repo unintentionally." Clear enough.

### Verified Claims

| Claim | Verdict |
|-------|---------|
| `format!()` with `{agent_root}` is safe (no braces in template) | ✅ Confirmed — lines 369-442 have zero `{` or `}` in the string body |
| All 3 call sites handled | ✅ Confirmed: session_context.rs:33, session_context.rs:281, commands/session.rs:118 |
| `simple_hash` exists and is reusable | ✅ Line 358, same djb2 pattern used at lines 147 and 342 |
| `cwd` available at both call sites | ✅ `build_replica_context` param at line 249; `create_session_inner` param at line 28 |
| Per-hash file naming prevents cross-agent races | ✅ Different cwd → different hash → different file. Same cwd → identical content overwrite |
| "Extra Enter is harmless" for Codex placeholder | N/A (Codex gets generic text, credentials injected separately — plan §1.1.4 is correct) |

### Finding: Recommend `canonicalize` on `agent_root` Before Injection

**[INFO, non-blocking]** `cwd` is injected into the GOLDEN RULE as-is. If it contains non-canonical components (e.g., mixed separators `C:\foo/bar`, trailing slashes, or embedded `..`), the rule text would contain a path that doesn't visually match what the agent sees in its filesystem operations.

In practice this is cosmetic — agents don't reason about path traversal, and `cwd` values from session-requests are clean absolute paths. But a one-line `canonicalize` before injection would eliminate any ambiguity:

```rust
let canonical_root = std::fs::canonicalize(agent_root)
    .unwrap_or_else(|_| std::path::PathBuf::from(agent_root))
    .to_string_lossy()
    .to_string();
```

Fallback to the raw value if canonicalize fails (directory doesn't exist yet). Not blocking — the plan works without it.

### What I Checked That Passed

- **Hash collision risk**: u64 space (18.4 quintillion values). Two different `cwd` strings colliding is astronomically unlikely. Pre-existing pattern used for `replica-context-{hash}.md`. Not a concern.
- **Non-agent sessions**: Regular Claude sessions (user-created) would get their `cwd` injected as the "agent root." This could be any directory, but: (a) the user chose it, (b) the rule still requires `repo-*` for repos, and (c) the additional zone just matches where the session is running — reasonable.
- **Codex `developer_instructions` sharing**: The generic placeholder `<YOUR OWN REPLICA ROOT — see Session Credentials below>` is correct. Codex sessions receive credentials via PTY injection (same as Claude — `needs_explicit_enter` returns true for codex at inject.rs:22). The credentials block includes `Root: <cwd>`. Agent can cross-reference.
- **`global_context_path()` dead function** (line 21): Dev-rust correctly says leave it. No clippy warning since it's `pub`. Not this plan's problem.
- **Intermediate vs final context file**: The per-agent file (`ac-context-{hash}.md`) is used either directly (fallback path, session.rs:118) or combined into `replica-context-{hash}.md` (when `config.json` has `context[]`). Both paths correctly propagate the per-agent GOLDEN RULE text to the session.
