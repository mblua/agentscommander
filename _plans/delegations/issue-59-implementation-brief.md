# Issue 59 Implementation Brief

## Repo
`C:\Users\maria\0_repos\agentscommander\.ac-new\wg-6-dev-team\repo-AgentsCommander`

## Branch
`feature/github-message-transport-gh59`

## Tracking Issue
`https://github.com/mblua/AgentsCommander/issues/59`

## Implementation Goal
Move human message transport from raw CLI payloads to GitHub issue comment URLs only.

## Required Outcomes

- `send --message` accepts only a GitHub issue comment URL.
- Raw text messaging paths are removed for human messages.
- `--message-file` is removed for messaging.
- `send --get-output` is removed for human message flows.
- `--command` is a pure non-message path and rejects `--command` together with `--message`.
- Legacy queued raw-text outbox files remain deserializable and reject with an explicit migration reason.
- Mailbox PTY delivery becomes a short English GitHub notification block.
- Task records are added under `_plans/tasks/YYYYMMDD_HHmmss-slug.{json,md}`.
- Helper scripts are added:
  - `scripts/task-new.ps1`
  - `scripts/gh-message.ps1`
- All generated Markdown, script output, and GitHub-facing templates remain in English.

## Consensus Constraints

1. Add a dedicated Rust helper module for task resolution and validation.
2. Task lookup resolves the sender workgroup from `--root` and scans sibling `repo-*` directories under that workgroup for `_plans/tasks/*.json`.
3. Malformed task JSON, filename/id mismatches, missing required fields, or inconsistent active-task ownership metadata are hard failures when those files could participate in the sender workgroup match set.
4. When `status == "active"`, there must be exactly one `workgroupHistory` row with `endedAt == null`, and that row must agree with both `activeWorkgroup` and `branch.name`.
5. `task-new.ps1` must use a workgroup-scoped lock, re-scan under that lock before writing, stage paired files under temporary names, and rename them into place only after both are ready.
6. `gh-message.ps1` must require `-Token` and `-BinaryPath` before posting when notify mode is requested. If those inputs are missing, fail before creating the GitHub comment.
7. Disable `send --outbox` for `send --message` in this phase so task resolution and outbox ownership stay aligned.

## Expected File Scope

- New `src-tauri/src/cli/task_resolution.rs`
- `src-tauri/src/cli/mod.rs`
- `src-tauri/src/cli/send.rs`
- `src-tauri/src/phone/types.rs`
- `src-tauri/src/phone/mailbox.rs`
- `src-tauri/src/pty/manager.rs`
- `src-tauri/src/cli/close_session.rs`
- `src-tauri/src/config/session_context.rs`
- `README.md`
- `CLAUDE.md`
- `ROLE_AC_BUILDER.md`
- New `scripts/task-new.ps1`
- New `scripts/gh-message.ps1`

## Delivery Notes

- Keep command delivery behavior intact outside the message-path restriction above.
- Do not leave a second raw human-message channel behind. This includes any exposed `phone_send_message` / `PhoneAPI.sendMessage` path.
- If a compatibility shim is needed for legacy queued raw-text outbox files, keep it explicit and reject with a migration reason instead of silently accepting or silently dropping them.

## Required Reply

Reply only with:
- changed files
- checks run
- blockers, if any
