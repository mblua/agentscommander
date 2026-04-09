# Plan: Per-Project Coding Agents Configuration

**Branch:** `feature/per-project-coding-agents`
**Status:** Draft
**Created:** 2026-04-09

---

## Problem Statement

Coding Agents (Claude Code, Codex, Gemini CLI, custom) are currently configured globally in `settings.json` and apply to ALL projects. Users need the ability to override which agents are available on a per-project basis — e.g., Project A uses only Claude Code while Project B uses Claude Code + Codex.

## Solution Overview

1. **Per-project settings file** stored at `<project>/.ac-new/project-settings.json`
2. **New context menu option** "Coding Agents" in ProjectPanel's right-click menu
3. **Modal UI** reusing the existing Coding Agents tab pattern from SettingsModal
4. **Visual badge** on project header when custom agents are configured
5. **Resolution logic**: project-level agents **fully replace** global agents (no merge)

---

## 1. Data Model Changes

### 1.1 New File: `<project>/.ac-new/project-settings.json`

```json
{
  "agents": [
    {
      "id": "agent_1712678400000_0",
      "label": "Claude Code",
      "command": "claude",
      "color": "#d97706",
      "gitPullBefore": false,
      "excludeGlobalClaudeMd": true
    }
  ]
}
```

**Design decisions:**
- Lives inside `.ac-new/` (already exists for every AC project, already gitignored via `wg-*/` pattern)
- Uses the same `AgentConfig` schema as global settings — identical fields
- File is optional: absence means "use global agents"
- Empty `agents: []` means "no agents available for this project" (valid state, distinct from absent file)
- camelCase keys (matching existing `config.json` pattern in `.ac-new/`)

### 1.2 Rust Struct: `ProjectSettings`

**File:** `src-tauri/src/config/project_settings.rs` (new)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettings {
    #[serde(default)]
    pub agents: Vec<AgentConfig>,
}
```

Reuses existing `AgentConfig` from `settings.rs`. No new types needed.

> **[DEV-RUST] Serde note:** `AgentConfig` in `settings.rs` already has `#[serde(rename_all = "camelCase")]` and `#[serde(default)]` on bool fields (`git_pull_before`, `exclude_global_claude_md`). This means project-settings.json will serialize/deserialize correctly with camelCase keys matching the frontend. The `#[serde(default)]` on `agents: Vec<AgentConfig>` ensures that a JSON file with `{}` (no `agents` key) deserializes to an empty vec — important for forward-compatibility if we add more fields to `ProjectSettings` later.

### 1.3 Frontend Type: `ProjectSettings`

**File:** `src/shared/types.ts` — add:

```typescript
export interface ProjectSettings {
  agents: AgentConfig[];
}
```

### 1.4 Extend `ProjectState` (frontend store)

**File:** `src/sidebar/stores/project.ts` — add field:

```typescript
interface ProjectState {
  path: string;
  folderName: string;
  workgroups: AcWorkgroup[];
  agents: AcAgentMatrix[];
  teams: AcTeam[];
  projectSettings: ProjectSettings | null;  // NEW — null = no override, use global
}
```

---

## 2. Backend Changes (Rust)

### 2.1 New Module: `src-tauri/src/config/project_settings.rs`

Functions:
- `project_settings_path(project_path: &str) -> PathBuf` — returns `<project>/.ac-new/project-settings.json`
- `load_project_settings(project_path: &str) -> Option<ProjectSettings>` — reads and parses; returns `None` if file missing or invalid
- `save_project_settings(project_path: &str, settings: &ProjectSettings) -> Result<(), String>` — writes JSON (pretty-printed)
- `delete_project_settings(project_path: &str) -> Result<(), String>` — removes the file (revert to global)

Register module in `src-tauri/src/config/mod.rs`.

> **[DEV-RUST] File I/O details and edge cases:**
>
> 1. **Path validation:** `project_path` originates from the frontend (user-controlled). Before constructing `.ac-new/project-settings.json`, validate that the project path is a real existing directory AND that `.ac-new/` exists inside it. This prevents the backend from creating arbitrary directories on the filesystem if the frontend sends a malicious or garbled path. Pattern:
>    ```rust
>    fn validated_settings_path(project_path: &str) -> Result<PathBuf, String> {
>        let base = Path::new(project_path);
>        if !base.is_dir() {
>            return Err(format!("Project path is not a directory: {}", project_path));
>        }
>        let ac_dir = base.join(".ac-new");
>        if !ac_dir.is_dir() {
>            return Err(format!("Not an AC project (no .ac-new/): {}", project_path));
>        }
>        Ok(ac_dir.join("project-settings.json"))
>    }
>    ```
>    Use this in all three functions instead of raw `project_settings_path()`.
>
> 2. **`load_project_settings` graceful fallback:** Must return `None` on *any* read/parse failure (not just missing file). This matches test case E7 and the pattern in `settings.rs:load_settings()` which returns `AppSettings::default()` on parse errors. Use:
>    ```rust
>    pub fn load_project_settings(project_path: &str) -> Option<ProjectSettings> {
>        let path = validated_settings_path(project_path).ok()?;
>        if !path.exists() { return None; }
>        let content = std::fs::read_to_string(&path).ok()?;
>        serde_json::from_str(&content).ok()
>    }
>    ```
>    Log a warning on parse failure (`serde_json::from_str` returns `Err`) so corrupted files are visible in logs but don't crash the UI.
>
> 3. **`delete_project_settings` idempotent:** `std::fs::remove_file` returns `Err` if the file doesn't exist. Handle `ErrorKind::NotFound` as success (idempotent delete). The frontend may call delete on a project that never had custom settings.
>
> 4. **`save_project_settings` uses `serde_json::to_string_pretty`:** Consistent with `save_settings()` in `settings.rs:244`. The `.ac-new/` directory already exists (validated above), so no `create_dir_all` needed — but keeping it as a safety net is fine.
>
> 5. **Encoding:** `serde_json` produces and expects UTF-8. `fs::write`/`fs::read_to_string` handle UTF-8 natively on all platforms. No BOM issues on Windows since we're writing JSON, not TOML.

### 2.2 New Tauri Commands: `src-tauri/src/commands/project_settings.rs`

```rust
#[tauri::command]
pub async fn get_project_settings(project_path: String) -> Result<Option<ProjectSettings>, String>
// Reads project-settings.json. Returns None if file doesn't exist.

#[tauri::command]
pub async fn update_project_settings(project_path: String, settings: ProjectSettings) -> Result<(), String>
// Writes project-settings.json. Creates .ac-new/ if needed.

#[tauri::command]
pub async fn delete_project_settings(project_path: String) -> Result<(), String>
// Deletes project-settings.json. Reverts project to global agents.

#[tauri::command]
pub async fn resolve_agents_for_project(
    project_path: String,
    settings: State<'_, SettingsState>,
) -> Result<Vec<AgentConfig>, String>
// Resolution logic:
//   1. Try load project-settings.json
//   2. If exists and has agents array → return those
//   3. Otherwise → return global settings.agents
```

Register commands in `lib.rs` invoke handler (NOT `main.rs`).

> **[DEV-RUST] Command registration specifics:**
>
> 1. **Registration is in `lib.rs`, not `main.rs`:** The `generate_handler![]` macro is in `src-tauri/src/lib.rs:570`. The plan says `main.rs` but the actual registration point is `lib.rs`. Add the 4 new commands after the existing `commands::entity_creation::*` block (line ~627).
>
> 2. **No Tauri capability/permission files exist:** The plan's section 5 mentions updating `src-tauri/capabilities/*.json` — those files don't exist in this project. Tauri 2's ACL is not configured; all commands registered in `generate_handler![]` are implicitly callable. So step A6 in the plan only needs `cargo check`, no permission files to update. **Remove the capabilities row from the "Tauri Permissions" table in section 5 to avoid confusion.**
>
> 3. **`resolve_agents_for_project` needs `State<'_, SettingsState>`:** This is already managed via `.manage()` in `lib.rs:241`. No new state registration needed — just add the `State<>` param to the command signature. Pattern matches `get_settings` in `commands/config.rs:24`.
>
> 4. **Error pattern:** All existing commands return `Result<T, String>` with `.map_err(|e| format!(...))`. Stay consistent — no `thiserror` in the commands layer even though CLAUDE.md mentions it for internal code. The commands are the boundary layer and Tauri requires `String` errors.
>
> 5. **Commands module registration:** Add `pub mod project_settings;` to `src-tauri/src/commands/mod.rs` (currently has: ac_discovery, agent_creator, config, entity_creation, phone, pty, repos, session, telegram, voice, window).

### 2.3 Extend Discovery (optional enhancement)

In `ac_discovery.rs`, the `AcDiscoveryResult` could include a `has_project_settings: bool` flag per project. However, since the frontend already loads project settings separately, this is **optional** and can be deferred. The badge can be driven by the frontend store instead.

> **[DEV-RUST] Recommendation: defer discovery integration.** Adding `has_project_settings` to `AcDiscoveryResult` would require modifying the `discover_project` command and its return type, which ripples into the frontend discovery store. Since the frontend can check project settings with a separate `get_project_settings` call (already planned), this adds complexity for no functional gain. The badge visibility can be derived from the `projectSettings` field in the frontend store. Defer to Phase E or beyond.

---

## 3. Frontend Changes

### 3.1 New IPC Wrappers: `src/shared/ipc.ts`

Add to existing API exports:

```typescript
export const ProjectSettingsAPI = {
  get: (projectPath: string) => 
    transport.invoke<ProjectSettings | null>("get_project_settings", { projectPath }),
  update: (projectPath: string, settings: ProjectSettings) => 
    transport.invoke<void>("update_project_settings", { projectPath, settings }),
  delete: (projectPath: string) => 
    transport.invoke<void>("delete_project_settings", { projectPath }),
  resolveAgents: (projectPath: string) => 
    transport.invoke<AgentConfig[]>("resolve_agents_for_project", { projectPath }),
};
```

### 3.2 Update Project Store: `src/sidebar/stores/project.ts`

- On `loadProject(path)` and `reloadProject(path)`, also call `ProjectSettingsAPI.get(path)` and store result in `projectSettings` field
- Add helper: `hasCustomAgents(path: string): boolean` — checks if `projectSettings !== null`
- Add helper: `getProjectSettings(path: string): ProjectSettings | null`

### 3.3 New Component: `ProjectAgentsModal.tsx`

**File:** `src/sidebar/components/ProjectAgentsModal.tsx`

A modal dialog that reuses the Coding Agents UI pattern from `SettingsModal.tsx` (lines ~319-448). Key differences from the global settings modal:

**Props:**
```typescript
{
  projectPath: string;
  projectName: string;
  initialSettings: ProjectSettings | null;
  onClose: () => void;
  onSaved: () => void;  // triggers project reload
}
```

**UI Structure:**
```
Modal Overlay
  Modal Container
    Header: "Coding Agents — {projectName}"
    
    Toggle: "Use custom agents for this project" (checkbox/switch)
      - OFF (default when no project-settings.json): shows message "Using global agents"
      - ON: shows agent editor (same as SettingsModal Coding Agents tab)
    
    When ON:
      [Copy from Global] button — copies current global agents as starting point
      
      Agent list (same card UI as SettingsModal):
        For each agent:
          - Label input
          - Command input  
          - Color picker + hex
          - gitPullBefore checkbox
          - excludeGlobalClaudeMd checkbox
          - Remove button
      
      Add buttons:
        - Preset buttons (Claude Code, Codex, Gemini CLI) — from AGENT_PRESETS
        - Custom Agent button
    
    Footer:
      [Cancel] [Save]
      If toggle ON → save calls ProjectSettingsAPI.update()
      If toggle OFF → save calls ProjectSettingsAPI.delete() (revert to global)
```

**Key behaviors:**
- "Copy from Global" loads `SettingsAPI.get().agents` into the local editor — one-time copy, not a link
- Reuses `AGENT_PRESETS`, `AGENT_PRESET_MAP`, `newAgentId()` from `src/shared/agent-presets.ts`
- Same validation logic as SettingsModal (check for `--continue` / `-c` flags in Claude commands)
- On save, calls `onSaved()` which triggers `projectStore.reloadProject(path)`

### 3.4 Update ProjectPanel: Context Menu + Badge

**File:** `src/sidebar/components/ProjectPanel.tsx`

#### 3.4.1 New Signal

```typescript
const [showProjectAgents, setShowProjectAgents] = createSignal(false);
```

#### 3.4.2 Context Menu — Add "Coding Agents" Option

Insert before the separator (between "New Workgroup" and the separator):

```tsx
<div class="context-separator" />
<button
  class="session-context-option"
  onClick={() => { setShowCtxMenu(false); setShowProjectAgents(true); }}
>
  Coding Agents
</button>
```

Position: After "New Workgroup", before the existing separator + "Remove Project". This groups creation actions together and puts configuration in its own section.

#### 3.4.3 Modal Render

Below the existing modals (after NewWorkgroupModal), add:

```tsx
{showProjectAgents() && (
  <Portal>
    <ProjectAgentsModal
      projectPath={proj.path}
      projectName={proj.folderName}
      initialSettings={proj.projectSettings}
      onClose={() => setShowProjectAgents(false)}
      onSaved={() => {
        setShowProjectAgents(false);
        projectStore.reloadProject(proj.path);
      }}
    />
  </Portal>
)}
```

#### 3.4.4 Badge on Project Header

In the project header button (line ~342-351), add a badge after the title:

```tsx
<button class="project-header" ...>
  <span class="ac-discovery-chevron" ...>▾</span>
  <span class="project-title">Project: {proj.folderName}</span>
  {proj.projectSettings && (
    <span class="project-custom-agents-badge" title="Custom Coding Agents configured">
      ⚙ Custom Agents
    </span>
  )}
</button>
```

**CSS for badge** (add to project panel styles):
```css
.project-custom-agents-badge {
  font-size: 0.65em;
  padding: 1px 6px;
  border-radius: 3px;
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-secondary, rgba(255, 255, 255, 0.5));
  margin-left: 8px;
  white-space: nowrap;
  letter-spacing: 0.02em;
}
```

Subtle, non-intrusive — matches the industrial-dark aesthetic. No bright colors; uses opacity for hierarchy.

### 3.5 Update Agent Resolution Points

These components currently read agents from `SettingsAPI.get().agents` (global). They need to use the resolution logic instead:

#### 3.5.1 `AgentPickerModal.tsx` (line 23)

This modal is shown when picking an agent for a session. It needs to know which project the session belongs to.

**Change:** Accept optional `projectPath` prop. On mount:
```typescript
onMount(async () => {
  if (props.projectPath) {
    const resolved = await ProjectSettingsAPI.resolveAgents(props.projectPath);
    setAgents(resolved);
  } else {
    const settings = await SettingsAPI.get();
    setAgents(settings.agents);
  }
});
```

The caller must pass `projectPath` when the session/agent belongs to a project. Check all call sites and thread the project path through.

#### 3.5.2 `NewAgentModal.tsx` (line 39)

Same pattern — accept optional `projectPath`, resolve agents accordingly.

#### 3.5.3 `OpenAgentModal.tsx` (line 20)

Same pattern.

#### 3.5.4 `SessionItem.tsx` (line 143)

Currently checks `settingsStore.current?.agents` for agent availability. Should check resolved agents for the session's project instead. This may require the session to know which project it belongs to (trace the session → project relationship).

---

## 4. Implementation Sequence

### Phase A: Backend Foundation (no UI changes yet)

| Step | File | What |
|------|------|------|
| A1 | `src-tauri/src/config/project_settings.rs` | New module: `ProjectSettings` struct, load/save/delete functions |
| A2 | `src-tauri/src/config/mod.rs` | Register `project_settings` module |
| A3 | `src-tauri/src/commands/project_settings.rs` | New commands: get, update, delete, resolve |
| A4 | `src-tauri/src/commands/mod.rs` | Register module |
| A5 | `src-tauri/src/lib.rs` | Register commands in `generate_handler![]` (line ~570) |
| A6 | Verify | `cargo check` passes |

### Phase B: Frontend Types & IPC

| Step | File | What |
|------|------|------|
| B1 | `src/shared/types.ts` | Add `ProjectSettings` interface |
| B2 | `src/shared/ipc.ts` | Add `ProjectSettingsAPI` object |
| B3 | `src/sidebar/stores/project.ts` | Extend `ProjectState`, load settings on discovery |
| B4 | Verify | `npx tsc --noEmit` passes |

### Phase C: Modal UI

| Step | File | What |
|------|------|------|
| C1 | `src/sidebar/components/ProjectAgentsModal.tsx` | New modal component (reuse SettingsModal agent tab pattern) |
| C2 | CSS file for modal styles | Styles for the modal (or add to existing project panel CSS) |
| C3 | Verify | Modal renders correctly with test data |

### Phase D: Integration

| Step | File | What |
|------|------|------|
| D1 | `src/sidebar/components/ProjectPanel.tsx` | Add context menu option + modal trigger + badge |
| D2 | `src/sidebar/components/AgentPickerModal.tsx` | Accept `projectPath`, use resolution logic |
| D3 | `src/sidebar/components/NewAgentModal.tsx` | Same resolution update |
| D4 | `src/sidebar/components/OpenAgentModal.tsx` | Same resolution update |
| D5 | `src/sidebar/components/SessionItem.tsx` | Use resolved agents for display logic |
| D6 | Verify | Full flow works: set project agents → picker shows only those agents |

### Phase E: Validation & Edge Cases

| Step | What |
|------|------|
| E1 | Test: project with custom agents → only those appear in picker |
| E2 | Test: project without custom agents → global agents appear |
| E3 | Test: toggle custom agents off → file deleted, reverts to global |
| E4 | Test: "Copy from Global" populates correctly |
| E5 | Test: badge appears/disappears correctly |
| E6 | Test: empty agents array `[]` → no agents available (valid state) |
| E7 | Test: malformed/corrupted project-settings.json → graceful fallback to global |

---

## 5. Files Changed Summary

### New Files
| File | Purpose |
|------|---------|
| `src-tauri/src/config/project_settings.rs` | Rust: ProjectSettings struct, load/save/delete |
| `src-tauri/src/commands/project_settings.rs` | Rust: Tauri commands for project settings |
| `src/sidebar/components/ProjectAgentsModal.tsx` | Frontend: modal for editing project agents |

### Modified Files
| File | Change |
|------|--------|
| `src-tauri/src/config/mod.rs` | Register project_settings module |
| `src-tauri/src/commands/mod.rs` | Register project_settings commands module |
| `src-tauri/src/lib.rs` | Register 4 new commands in `generate_handler![]` |
| `src/shared/types.ts` | Add `ProjectSettings` interface |
| `src/shared/ipc.ts` | Add `ProjectSettingsAPI` |
| `src/sidebar/stores/project.ts` | Extend ProjectState, load project settings |
| `src/sidebar/components/ProjectPanel.tsx` | Context menu option + badge + modal trigger |
| `src/sidebar/components/AgentPickerModal.tsx` | Accept projectPath, use resolution |
| `src/sidebar/components/NewAgentModal.tsx` | Accept projectPath, use resolution |
| `src/sidebar/components/OpenAgentModal.tsx` | Accept projectPath, use resolution |
| `src/sidebar/components/SessionItem.tsx` | Use resolved agents for display |
| Sidebar CSS (project panel styles) | Badge styling |

### Tauri Permissions

> **[DEV-RUST] No capability files exist.** This project does not use Tauri 2's ACL/capabilities system. There are no `src-tauri/capabilities/*.json` files. Commands are implicitly allowed by being listed in `generate_handler![]` in `lib.rs`. No permission changes needed — just add the 4 commands to the handler macro.

---

## 6. Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Breaking existing agent picker behavior | Resolution command falls back to global — same behavior as today when no project settings exist |
| Stale project settings after reload | `reloadProject()` already re-fetches all data; just add project settings to that flow |
| File permissions on `.ac-new/` | Directory already exists and is writable (used by discovery) |
| Session-project association unclear | Sessions are created within project context; trace the project path from where the session is spawned |
| AgentConfig ID collisions between global and project | IDs use timestamp + counter (`newAgentId()`), collisions extremely unlikely; and they're independent namespaces anyway |

> **[DEV-RUST] Additional risks identified:**
>
> | Risk | Mitigation |
> |------|-----------|
> | Path traversal via `project_path` param | Validate `.ac-new/` exists within the path before any write (see validated_settings_path above) |
> | Concurrent write from multiple windows | Acceptable — same pattern as `save_settings()` in settings.rs. Both sidebar windows writing simultaneously is extremely unlikely since the modal blocks interaction |
> | Stale `SettingsState` in `resolve_agents_for_project` | The command reads from `State<SettingsState>` which is the in-memory live copy (updated by `update_settings`). No stale-file risk — it's always current |
