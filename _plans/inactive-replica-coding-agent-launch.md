# Plan: Inactive SessionItem → Coding Agent Launch

**Branch:** `feature/inactive-replica-coding-agent-launch`
**Status:** Draft

## Problem

Inactive SessionItems (discovered replicas with no active PTY session) are completely non-interactive — no click handler, no context menu, grayed out with `cursor: default`. Users must navigate to ProjectPanel or AcDiscoveryPanel to launch sessions for these replicas.

## Goal

Enable right-click context menu on inactive SessionItems to launch a coding agent session. Right-click shows a context menu with "Launch Coding Agent" option that opens AgentPickerModal.

**Note:** The right-click context menu is **REQUIRED by spec**. Left-click behavior for inactive items is out of scope for this change.

## Scope

**This is a frontend-only change.** Only SolidJS/TypeScript and CSS files are modified. No Rust backend changes required — `SessionAPI.create` already supports all needed parameters.

## Current State

### Inactive sessions — `makeInactiveEntry()` in `src/sidebar/stores/sessions.ts:31`:
```typescript
{ id: `inactive-${normalizedPath}`, name, shell: "", shellArgs: [],
  workingDirectory: path, status: "idle", ... }
```

### SessionItem.tsx — inactive items fully disabled:
- **Line 264:** `onClick={isInactive() ? undefined : handleClick}`
- **Line 265:** `onContextMenu={isInactive() ? undefined : handleContextMenu}`
- **Line 344:** `<Show when={!isInactive()}>` hides all action buttons
- **CSS:** `.session-item.inactive-member { opacity: 0.5; cursor: default; }` + no hover

### Data available on inactive sessions:
- `workingDirectory` — replica path
- `name` — agent/repo name
- No `preferredAgentId`, no `shell`, no `shellArgs`

### Current API (this branch):
- `AgentPickerModal` props: `{ sessionName, onSelect, onClose }` — fetches global agents from `SettingsAPI.get()`
- `SessionAPI.create({ cwd, sessionName, agentId, ... })` — creates new session
- `SessionAPI.restart(id, { agentId })` — restarts existing session
- `restartSession(agentId?)` — existing helper in SessionItem (line 204)

---

## Implementation

### Phase A: Right-click context menu (REQUIRED by spec)

**File: `src/sidebar/components/SessionItem.tsx`**

1. **Wire onContextMenu** (line 265):
   ```typescript
   onContextMenu={handleContextMenu}
   ```
   Remove the `isInactive()` guard entirely — `handleContextMenu` (line 184) works generically (sets position, shows menu, adds dismiss listeners). No change to the handler itself.

2. **Branch context menu content** (lines 431-458). Wrap existing active-only options and add inactive option:
   ```tsx
   {showContextMenu() && (
     <Portal>
       <div class="session-context-menu" ref={contextMenuEl}
         style={{ left: `${contextMenuPos().x}px`, top: `${contextMenuPos().y}px` }}
         onClick={(e) => e.stopPropagation()}>
         <Show when={isInactive()} fallback={
           <>
             <button class="session-context-option context-option-danger" onClick={handleRestart}>
               Restart Session
             </button>
             <button class="session-context-option" onClick={handleCodingAgentRestart}>
               Coding Agent
             </button>
             <Show when={hasClaude()}>
               <div class="context-separator" />
               <button class="session-context-option" onClick={handleExcludeClaudeMd}>
                 Exclude global CLAUDE.md
               </button>
             </Show>
           </>
         }>
           <button class="session-context-option" onClick={() => {
             setShowContextMenu(false);
             cleanupContextMenu();
             setShowCodingAgentPicker(true);
           }}>
             Launch Coding Agent
           </button>
         </Show>
       </div>
     </Portal>
   )}
   ```
   The `<Show when={isInactive()}>` renders "Launch Coding Agent" for inactive items. The `fallback` contains the existing active menu options (Restart, Coding Agent, Exclude CLAUDE.md).

3. **Branch `onSelect` in AgentPickerModal** (line 419-429). The context menu's "Launch Coding Agent" opens AgentPickerModal via `setShowCodingAgentPicker(true)`. The onSelect handler must branch for inactive items.

   Currently:
   ```typescript
   onSelect={async (agent) => {
     setShowCodingAgentPicker(false);
     await restartSession(agent.id);
   }}
   ```
   Change to (matches ProjectPanel pattern at `ProjectPanel.tsx:143-155`):
   ```typescript
   onSelect={async (agent) => {
     setShowCodingAgentPicker(false);
     if (isInactive()) {
       try {
         const newSession = await SessionAPI.create({
           cwd: props.session.workingDirectory,
           sessionName: props.session.name,
           agentId: agent.id,
         });
         await SessionAPI.switch(newSession.id);
         if (isTauri) {
           const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
           const detachedLabel = `terminal-${newSession.id.replace(/-/g, "")}`;
           const detachedWin = await WebviewWindow.getByLabel(detachedLabel);
           if (!detachedWin) {
             await WindowAPI.ensureTerminal();
           }
         }
       } catch (e) {
         console.error("Failed to launch session:", e);
       }
     } else {
       await restartSession(agent.id);
     }
   }}
   ```
   **Why create + switch + ensureTerminal:** `SessionManager::create_session()` only auto-activates the first session. Without `switch()`, the new session appears in the list but isn't selected. Without `ensureTerminal()`, the terminal window may not exist. This matches the established ProjectPanel launch pattern.

   **Why try/catch:** `create_session` can fail (PTY spawn failure, context file validation error, cwd doesn't exist). The active branch routes through `restartSession()` which has internal try/catch. The inactive branch needs its own.

4. **No structural move needed.** The `showCodingAgentPicker` portal (line 419) is already OUTSIDE the `<Show when={!isInactive()}>` block (which ends at line 410), so it renders for both active and inactive items.

5. **`isTauri` import** — already available at `SessionItem.tsx:5` (imports from `../../shared/platform`). No new import needed.

### Phase B: CSS — Interactive Inactive Items

**File: `src/sidebar/styles/sidebar.css`**

```css
/* Before: */
.session-item.inactive-member { opacity: 0.5; cursor: default; }
.session-item.inactive-member:hover { background: transparent; }

/* After: */
.session-item.inactive-member { opacity: 0.5; cursor: pointer; }
.session-item.inactive-member:hover { opacity: 0.7; background: var(--bg-hover, rgba(255,255,255,0.03)); }
```

---

## Out of Scope

- **Left-click launch:** Left-click on inactive items remains disabled. Could be added as a follow-up to open AgentPickerModal directly (one fewer step than right-click → menu → pick). Note: CSS changes make items look interactive, so users may try left-click first — consider adding in a follow-up.
- **Git branch info:** Inactive sessions lack `gitBranchSource`/`gitBranchPrefix`. Created session won't auto-checkout. Can be added later.
- **PreferredAgentId:** `makeInactiveEntry` only gets name+path. Auto-launch without picker would require propagating `preferredAgentId` through the session store — separate change.
- **Per-project agent resolution:** `AgentPickerModal` currently fetches global agents only. Project-aware agent resolution is on a separate branch (`feature/per-project-coding-agents`).

## Files Changed

| File | Change |
|------|--------|
| `src/sidebar/components/SessionItem.tsx` | Wire contextmenu for inactive, branch onSelect for create+switch+ensureTerminal vs restart, branch context menu content |
| `src/sidebar/styles/sidebar.css` | Interactive cursor and hover for `.inactive-member` |

## Verification

1. `npx tsc --noEmit` — clean compile
2. Visual: inactive items show pointer cursor and hover effect
3. Right-click inactive → "Launch Coding Agent" menu → AgentPickerModal → select → new session created, activated, terminal shown
4. Right-click active → existing menu (Restart, Coding Agent, Exclude CLAUDE.md) unchanged
5. Left-click inactive → no action (unchanged from current behavior)
6. Error case: right-click inactive with invalid cwd → no crash, error logged to console

---

## Grinch Review

**VERDICT: FAIL — 1 bug, 1 concern.**

### Finding 1 (BUG): Created session is not activated — user gets no feedback

**What:** Phase A Step 3 `onSelect` calls `SessionAPI.create()` but never calls `SessionAPI.switch()` or `WindowAPI.ensureTerminal()`. The session is created in the backend but NOT activated.

**Why it matters:** Traced the full lifecycle:
- `SessionManager::create_session()` (`session/manager.rs:61-64`): only auto-activates if `active_session.is_none()` — i.e., no sessions exist. If the user already has active sessions (the common case), the new session is NOT activated.
- `session_created` event handler in sidebar `App.tsx:120-125`: only sets active if `sessionsStore.sessions.length === 1` — same: only the first session.
- Terminal `App.tsx:91-99`: only activates if `!terminalStore.activeSessionId` — same.

So: user clicks inactive item → picks agent → session created → user still looking at whatever was active before. The new session appears in the list but is not selected, not focused, terminal doesn't show it. From the user's perspective, nothing happened.

**Fix:** The inactive branch must match the ProjectPanel pattern (`ProjectPanel.tsx:143-155`, `1302-1315`):

```typescript
if (isInactive()) {
  const newSession = await SessionAPI.create({
    cwd: props.session.workingDirectory,
    sessionName: props.session.name,
    agentId: agent.id,
  });
  await SessionAPI.switch(newSession.id);
  if (isTauri) {
    const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
    const detachedLabel = `terminal-${newSession.id.replace(/-/g, "")}`;
    const detachedWin = await WebviewWindow.getByLabel(detachedLabel);
    if (!detachedWin) {
      await WindowAPI.ensureTerminal();
    }
  }
}
```

This requires adding `isTauri` import (already available: `SessionItem.tsx:5` imports from `../../shared/platform`).

### Finding 2 (CONCERN): No error handling on create

**What:** The inactive branch `await SessionAPI.create({...})` has no try/catch. The active branch routes through `restartSession()` (line 204-212) which has internal try/catch. The inactive branch doesn't.

**Why it matters:** `create_session` can fail — PTY spawn failure, context file validation error (session.rs:127-144 aborts with error dialog + session cleanup), cwd doesn't exist. An uncaught rejection from `SessionAPI.create()` inside the `onSelect` async callback becomes an unhandled promise rejection (the modal calls `props.onSelect()` without awaiting — AgentPickerModal.tsx:40,67).

**Fix:** Wrap in try/catch matching existing pattern:

```typescript
if (isInactive()) {
  try {
    const newSession = await SessionAPI.create({...});
    await SessionAPI.switch(newSession.id);
    // ... ensureTerminal
  } catch (e) {
    console.error("Failed to launch session:", e);
  }
}
```

### Resolved from tech-lead's earlier flags

- **`extractProjectPath`/`projectPath` phantom APIs:** Not present in current plan version. Line 152 explicitly defers to separate branch. ✓
- **Right-click optional:** Now mandatory Phase B with full implementation. ✓

### What passed

- Phase B context menu branching via `<Show when={isInactive()}>` / `fallback` — clean separation, no active-only options reachable for inactive items. ✓
- `handleContextMenu` reuse for inactive — handler is purely positional, no session-specific logic. ✓
- CSS changes — `cursor: pointer` + hover effect are correct and minimal. ✓
- Inactive entry auto-removal: `sessions.ts` `createMemo` re-derives on `state.sessions` change; `activePathSet` will include the new session's path, so the `makeInactiveEntry` call is skipped. No duplicate entries. ✓
- Portal placement: `showCodingAgentPicker` portal (line 419) is outside `<Show when={!isInactive()}>` block (ends line 410). No structural change needed. ✓
- Line number references in plan match actual code. ✓
