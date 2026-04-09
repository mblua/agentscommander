# Plan: Fix workgroup clone missing .git/index

**Branch:** `fix/workgroup-clone-missing-index`
**Primary file:** `src-tauri/src/commands/ac_discovery.rs`
**Secondary file:** `src-tauri/src/commands/entity_creation.rs`

---

## 1. Root Cause Analysis

### Observed symptoms
- `.git/` has HEAD, config, objects/, refs/, packed-refs, shallow — but **no index file**
- **Zero** working tree files on disk
- `git ls-tree HEAD` shows all 7,548 files correctly (object database intact)
- `git status` shows 7,548 staged deletions
- `git reflog` shows only `clone: from <url>` — no post-clone operations

### Actual root cause: Parent repo tracking child clones

When a project folder is itself a git repo, and AgentsCommander creates `.ac-new/wg-*/repo-*` inside it, the **parent repo tracks those cloned repo files**. Parent git operations (`checkout`, `reset`, `clean`) then corrupt the child clones — deleting working tree files and the index.

**Evidence:** The affected project (`phi_phibridge`) has NO `.gitignore` entry for `.ac-new/`, and `git status` at the project root shows 138 changes tracked inside `.ac-new/`. Any parent-level `git checkout` or `git reset` wipes the child repo's working tree and index because git treats those files as part of the parent.

### Why the clone itself succeeds
The `git clone --depth 1` in `git_clone_async()` works correctly. The repo is fully checked out immediately after clone. The corruption happens **later**, when the parent repo's git operations interfere with the child clone's files.

---

## 2. Primary Fix: Ensure `.ac-new/.gitignore` excludes workgroup directories

### Strategy
Every time AgentsCommander creates or opens a project's `.ac-new/` directory, ensure a `.gitignore` file exists inside it that excludes workgroup directories from parent repo tracking.

### Why `.ac-new/.gitignore` and not project root `.gitignore`?
- We must NOT modify the user's project root `.gitignore` — that's their file, committed to their repo
- Git respects `.gitignore` files in subdirectories — a `.gitignore` inside `.ac-new/` applies to that subtree
- This is self-contained: all AC artifacts stay within `.ac-new/`

### The gitignore content

```gitignore
# AgentsCommander: exclude workgroup cloned repos from parent git tracking.
# Without this, parent repo operations (checkout, reset) corrupt child clones.
wg-*/
```

### Entry points that need the check

There are **two Tauri commands** that create the `.ac-new/` directory, plus one that operates inside it:

| Command | File | Line | When called |
|---|---|---|---|
| `create_ac_project(path)` | `ac_discovery.rs` | 699 | User clicks "New Project" (creates `.ac-new/` from scratch) |
| `create_workgroup(project_path, team_name)` | `entity_creation.rs` | 420 | User creates a workgroup (`.ac-new/` already exists) |

Additionally, `discover_project()` and `discover_ac_agents()` are read-only discovery commands called on "Open Project" — they scan `.ac-new/` but don't create it. However, `discover_ac_agents()` runs on app startup for all configured repo_paths, making it a good opportunistic repair point.

### Implementation: helper function `ensure_ac_new_gitignore()`

Create a shared helper that both entry points call:

```rust
/// Ensure .ac-new/.gitignore exists and contains the wg-*/ exclusion pattern.
/// This prevents parent repo operations from corrupting cloned repos inside workgroups.
fn ensure_ac_new_gitignore(ac_new_dir: &Path) -> Result<(), String> {
    let gitignore_path = ac_new_dir.join(".gitignore");
    let required_pattern = "wg-*/";

    if gitignore_path.exists() {
        // Read existing content, check if pattern is present
        let content = std::fs::read_to_string(&gitignore_path)
            .map_err(|e| format!("Failed to read .ac-new/.gitignore: {}", e))?;

        if !content.lines().any(|line| line.trim() == required_pattern) {
            // Append the pattern
            let separator = if content.ends_with('\n') { "" } else { "\n" };
            let addition = format!(
                "{}# AgentsCommander: exclude workgroup cloned repos from parent git tracking.\n{}\n",
                separator, required_pattern
            );
            std::fs::write(&gitignore_path, format!("{}{}", content, addition))
                .map_err(|e| format!("Failed to update .ac-new/.gitignore: {}", e))?;
        }
    } else {
        // Create new .gitignore
        let content = format!(
            "# AgentsCommander: exclude workgroup cloned repos from parent git tracking.\n# Without this, parent repo operations (checkout, reset) corrupt child clones.\n{}\n",
            required_pattern
        );
        std::fs::write(&gitignore_path, content)
            .map_err(|e| format!("Failed to create .ac-new/.gitignore: {}", e))?;
    }

    Ok(())
}
```

### Insertion points

#### A. `create_ac_project()` in `ac_discovery.rs` (line 699)

After creating `.ac-new/` directory (line 701), call:
```rust
ensure_ac_new_gitignore(&ac_new)?;
```

This is the "New Project" path — the most critical entry point since it's where `.ac-new/` is first created.

#### B. `create_workgroup()` in `entity_creation.rs` (line 420)

After validating `.ac-new/` exists (line 426-428), before cloning repos, call:
```rust
ensure_ac_new_gitignore(&base)?;
```

This catches the case where `.ac-new/` was created before the fix was deployed. Every new workgroup creation repairs the gitignore.

#### C. `discover_ac_agents()` in `ac_discovery.rs` (line 415) — opportunistic repair

Inside the loop that scans repo_paths (around line 448), after confirming `.ac-new/` exists:
```rust
if ac_new_dir.is_dir() {
    // Opportunistic: ensure gitignore exists for existing projects
    let _ = ensure_ac_new_gitignore(&ac_new_dir);  // Ignore errors — discovery is read-only
    // ... existing discovery logic
}
```

This heals ALL existing projects on next app startup, silently. The `let _ =` is intentional — discovery should not fail just because gitignore repair fails.

### Where to put the helper function

Since it's used by both `ac_discovery.rs` and `entity_creation.rs`, place it in a shared location. Options:

- **Option 1 (simplest):** Define it in `ac_discovery.rs` and make it `pub(crate)`, import from `entity_creation.rs`
- **Option 2:** Define it as a standalone function in a shared utils module

Recommendation: **Option 1** — the function is small and `ac_discovery.rs` is the natural home since it owns `.ac-new/` lifecycle.

---

## 3. Defensive Layer: Post-clone validation in `git_clone_async()`

This is retained from the original plan as a **secondary defense**. Even with the gitignore fix, defensive validation prevents silent failures from any cause.

### Changes to `git_clone_async()` (entity_creation.rs, lines 1205-1235)

#### 3a. Log stderr even on success

After a successful clone, log stderr for diagnostics:

```rust
let stderr = String::from_utf8_lossy(&output.stderr);
if !stderr.trim().is_empty() {
    let capped = &stderr[..stderr.len().min(512)];
    log::info!("[git_clone_async] stderr for {}: {}", target.display(), capped);
}
```

#### 3b. Post-clone index validation

After clone reports success, verify the index exists:

```rust
let index_path = target.join(".git").join("index");
if !index_path.exists() {
    log::warn!(
        "[git_clone_async] Clone succeeded but .git/index missing at {} — running recovery",
        target.display()
    );
    run_git_checkout_recovery(target).await?;

    // Final check
    if !index_path.exists() {
        return Err(format!(
            "git clone produced incomplete repo at {}: .git/index missing after recovery",
            target.display()
        ));
    }
}
```

#### 3c. Recovery function

```rust
async fn run_git_checkout_recovery(repo_path: &Path) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["checkout", "HEAD", "--", "."])
        .current_dir(repo_path);

    // NOTE: intentionally NOT using CREATE_NO_WINDOW here.
    // If the original checkout failed due to console handle issues,
    // using the same flag would cause recovery to fail too.

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to spawn git checkout recovery: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git checkout recovery failed: {}", stderr.trim()));
    }

    log::info!("[git_clone_async] Recovery checkout succeeded at {}", repo_path.display());
    Ok(())
}
```

---

## 4. Testing Strategy

### 4a. Primary fix verification

1. **New project on a git repo:** Create a new project via the UI on a folder that is a git repo. Verify:
   - `.ac-new/.gitignore` exists
   - Contains `wg-*/` pattern
   - `git status` at project root does NOT show `.ac-new/wg-*` entries after creating a workgroup

2. **Existing project without gitignore:** Open a project that already has `.ac-new/` but no `.gitignore`. Verify:
   - On app startup (via `discover_ac_agents`), `.ac-new/.gitignore` is created
   - Subsequent parent git operations do not corrupt child clones

3. **Existing project with gitignore but missing pattern:** Manually create `.ac-new/.gitignore` with other content. Verify the `wg-*/` pattern is appended without losing existing content.

4. **Idempotency:** Run the same operations multiple times. Verify the pattern is NOT duplicated in `.gitignore`.

### 4b. Defensive layer verification

1. **Normal clone:** Create a workgroup. Check logs for stderr output. Verify no recovery was triggered.
2. **Forced failure:** Temporarily delete `.git/index` after clone in code, verify recovery triggers and succeeds.

### 4c. Edge cases

| Scenario | Expected |
|---|---|
| Project folder is NOT a git repo | Gitignore still created (harmless, and protects if user later does `git init`) |
| `.ac-new/.gitignore` is read-only | Error logged but operation continues (graceful degradation) |
| Workgroup created before fix deployed | Healed on next app startup via `discover_ac_agents` |
| Multiple workgroups created in sequence | Gitignore check runs once per `create_workgroup` call, idempotent |

---

## 5. Implementation Sequence

1. **Create `ensure_ac_new_gitignore()` helper** in `ac_discovery.rs` as `pub(crate)`
2. **Call from `create_ac_project()`** — after `create_dir_all` (ac_discovery.rs:701)
3. **Call from `create_workgroup()`** — after `.ac-new` validation (entity_creation.rs:428)
4. **Call from `discover_ac_agents()`** — opportunistic repair in scan loop (ac_discovery.rs:~449)
5. **Add stderr logging** to `git_clone_async()` on success path (entity_creation.rs:1226)
6. **Add post-clone index validation** to `git_clone_async()` (entity_creation.rs:1232)
7. **Add `run_git_checkout_recovery()` helper** in `entity_creation.rs`
8. **Test**: Create workgroup on a git-tracked project, verify gitignore and clone integrity

### Files to modify
- `src-tauri/src/commands/ac_discovery.rs` — helper function + calls in `create_ac_project` and `discover_ac_agents`
- `src-tauri/src/commands/entity_creation.rs` — call in `create_workgroup` + post-clone validation + recovery helper

### Estimated scope
- Primary fix (gitignore): ~40 lines new code across 2 files
- Defensive layer (validation + recovery): ~35 lines in `entity_creation.rs`
- No frontend changes required
- No new Tauri commands needed

---

## Dev-Rust Review (v2)

Scope: Section 2 only (.gitignore fix). Sections 3+ are dropped.

### Exact insertion points (verified against current code)

**A. `create_ac_project()` — ac_discovery.rs:702**
```rust
// Current code:
700:    let ac_new = Path::new(&path).join(".ac-new");
701:    std::fs::create_dir_all(&ac_new)
702:        .map_err(|e| format!("Failed to create .ac-new directory: {}", e))?;
703:    Ok(())  // INSERT ensure_ac_new_gitignore(&ac_new)?; BEFORE this line
```
Insert `ensure_ac_new_gitignore(&ac_new)?;` at line 703, before `Ok(())`. The `?` propagation is correct here — if we can't create the gitignore, the project creation should fail (the directory was just created, so write permission should exist).

**B. `create_workgroup()` — entity_creation.rs:429**
```rust
// Current code:
425:    let base = Path::new(&project_path).join(".ac-new");
426:    if !base.is_dir() {
427:        return Err(format!(".ac-new directory not found in {}", project_path));
428:    }
429:                                         // INSERT HERE
430:    // Read team config
```
Insert `crate::commands::ac_discovery::ensure_ac_new_gitignore(&base)?;` at line 429. This is entity_creation.rs's first cross-module call to ac_discovery — there are no existing `use crate::commands::` imports in this file. Use the fully qualified path `crate::commands::ac_discovery::ensure_ac_new_gitignore` inline rather than adding an import, which is cleaner for a single call site.

**C. `discover_ac_agents()` — ac_discovery.rs:451**
```rust
// Current code:
448:            let ac_new_dir = repo_dir.join(".ac-new");
449:            if !ac_new_dir.is_dir() {
450:                continue;
451:            }
452:                                         // INSERT HERE
453:            let project_folder = repo_dir
```
Insert `let _ = ensure_ac_new_gitignore(&ac_new_dir);` at line 452. The `let _ =` is correct — discovery must not fail due to gitignore issues. This function scans ALL configured repo_paths on app startup, so it heals all existing projects.

### Helper function placement

Place `ensure_ac_new_gitignore()` as a `pub(crate) fn` in `ac_discovery.rs`, immediately before `create_ac_project()` (before line 697). This keeps it near its primary consumer and follows the file's existing layout of helper functions above the commands that use them.

### Code correctness — all good

- `lines()` handles both `\n` and `\r\n` (Rust stdlib) — correct for Windows-generated gitignores
- `line.trim()` catches trailing whitespace/CR — good
- `std::fs::write` is atomic on the content level (overwrites fully) — correct since we read+append+write
- The function is NOT async — correct, matches other small filesystem helpers in the codebase
- `wg-*/` (with trailing slash) is correct gitignore syntax for "directories only" matching

### One edge case to note

**discover_project()** (ac_discovery.rs:710-744) also scans `.ac-new/` directories. It's called when the user opens a specific project. The plan doesn't mention it, but it's another opportunity for repair. However, since `discover_ac_agents()` already covers startup repair across ALL projects, and `discover_project()` is read-only by design, I agree with skipping it. Not worth the extra call site.

### No concerns or disagreements

The plan is clean and minimal. The three insertion points are correct. The helper function handles creation, idempotent append, and the edge cases properly. Ready for implementation.
