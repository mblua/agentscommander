# Issue 59 Plan: GitHub Comment URL Message Transport

**Branch:** `feature/github-message-transport-gh59`  
**Status:** `reviewed`

## Requirement

Replace direct long-form payload delivery in `send` so agent-to-agent messages always live in GitHub issue comments and the CLI transports only a GitHub issue comment URL. Add project-level task records under repo `_plans/tasks/` that bind workgroup, GitHub issue, branch, and messaging mode together, then use those records to validate comment URLs and helper-script behavior.

This plan targets the current branch content and the active workgroup layout under:

`...\.ac-new\wg-*\repo-*`

## Architectural updates applied in this review

1. Task lookup is workgroup-scoped across sibling `repo-*` directories, not a single inferred repo root.
2. `task-new.ps1` requires a workgroup-scoped lock plus a second scan under that lock before writing files.
3. `gh-message.ps1` must require the notification prerequisites up front when notify mode is requested: `-Token` and `-BinaryPath`.
4. `send --command` stays only as a pure non-message remote action. `--command` and `--message` must be mutually exclusive.
5. Legacy queued raw-text outbox files must deserialize and then reject with an explicit migration reason.

## Delivery contract

1. Keep the outbox/mailbox file transport for routing and delivery confirmation.
2. Stop transporting human message text in `send`.
3. Make `send --message` accept only an HTML GitHub issue comment URL:
   `https://github.com/<owner>/<repo>/issues/<number>#issuecomment-<id>`
4. Remove `--message-file` from `send`.
5. Remove `--get-output` and the PTY marker-response path from `send`.
6. Keep `--command` only as a pure remote slash-command path. It must not be combinable with `--message`.
7. Resolve the sender workgroup from `--root`, scan sibling `repo-*` directories under that workgroup, and require exactly one active task for that workgroup when `--message` is used.
8. Validate that the comment URL belongs to the active task issue before writing any outbox file.

Accepted `send --message` format:

```text
https://github.com/<owner>/<repo>/issues/<number>#issuecomment-<comment_id>
```

Rejected:

- Raw text
- Empty strings
- Pull request review comment URLs
- Pull request conversation URLs under `/pull/`
- Issue URLs without `#issuecomment-...`

## Proposed task record layout

Each task is a paired record:

- `_plans/tasks/YYYYMMDD_HHmmss-slug.json`
- `_plans/tasks/YYYYMMDD_HHmmss-slug.md`

The filename stem is the task id.

### JSON schema

```json
{
  "schemaVersion": 1,
  "taskId": "20260416_170719-github-message-transport-gh59",
  "slug": "github-message-transport-gh59",
  "summary": "GitHub comment URL message transport and task records",
  "status": "active",
  "createdAt": "2026-04-16T17:07:19Z",
  "updatedAt": "2026-04-16T17:07:19Z",
  "activeWorkgroup": "wg-6-dev-team",
  "workgroupHistory": [
    {
      "workgroup": "wg-6-dev-team",
      "startedAt": "2026-04-16T17:07:19Z",
      "endedAt": null,
      "note": "Task created"
    }
  ],
  "issue": {
    "host": "github.com",
    "owner": "mblua",
    "repo": "AgentsCommander",
    "number": 59,
    "url": "https://github.com/mblua/AgentsCommander/issues/59",
    "title": "[dev-team] GitHub comment URL message transport and task records"
  },
  "branch": {
    "name": "feature/github-message-transport-gh59",
    "kind": "feature",
    "slug": "github-message-transport",
    "issueSuffix": "gh59"
  },
  "messaging": {
    "mode": "github-issue-comment-url",
    "commentUrlPattern": "https://github.com/mblua/AgentsCommander/issues/59#issuecomment-*"
  }
}
```

Validation rules:

- `taskId` must equal the filename stem.
- `slug` must be lowercase kebab-case.
- `status` allowed values: `active`, `blocked`, `handoff`, `done`, `archived`.
- Exactly one task with `status == "active"` may exist per `activeWorkgroup`.
- `issue.url` must match `owner/repo/number`.
- `branch.name` must match `^(feature|fix|bug)/[a-z0-9][a-z0-9-]*-gh[0-9]+$`.
- The numeric suffix in `branch.name` must equal `issue.number`.
- `messaging.mode` is fixed to `github-issue-comment-url` for this feature.

### Markdown companion

```md
# Task: GitHub comment URL message transport and task records

- Task ID: `20260416_170719-github-message-transport-gh59`
- Status: `active`
- Active Workgroup: `wg-6-dev-team`
- Messaging Mode: `github-issue-comment-url`
- Created: `2026-04-16T17:07:19Z`
- Updated: `2026-04-16T17:07:19Z`

## Summary

Replace direct message payload transport in `send` so long-form agent messages always live in GitHub issue comments and `send` only transports GitHub issue comment URLs.

## GitHub

- Issue: [#59](https://github.com/mblua/AgentsCommander/issues/59)
- Branch: `feature/github-message-transport-gh59`

## Workgroup History

| Workgroup | Started | Ended | Note |
| --- | --- | --- | --- |
| `wg-6-dev-team` | `2026-04-16T17:07:19Z` | `active` | Task created |

## Notes

- Comment notifications must target the issue above.
- Post content in GitHub first, then notify the receiver with the comment URL.
```

The JSON file is authoritative for automation. The Markdown file is human-facing and must remain English.

## Affected files and responsibilities

### 1. `src-tauri/src/config/mod.rs`

Current anchor: lines 1-8.

Change:

- Add `pub mod task_records;`.

### 2. New `src-tauri/src/config/task_records.rs`

Responsibilities:

- Define serde structs for task records.
- Extract the sender workgroup from `--root`.
- Discover direct sibling `repo-*` directories under the sender workgroup directory.
- Scan `repo-*/_plans/tasks/*.json` for active-task matches.
- Reject `0` or `>1` active tasks for the sender workgroup.
- Parse and validate GitHub issue comment URLs.
- Validate branch naming and issue linkage.

Required functions:

- `extract_workgroup_from_root(root: &str) -> Result<String, String>`
- `discover_repo_roots_for_workgroup(root: &str) -> Result<Vec<PathBuf>, String>`
- `find_active_task_for_workgroup(root: &str) -> Result<ResolvedTask, String>`
- `validate_issue_based_branch(branch_name: &str, issue_number: u64) -> Result<(), String>`
- `parse_comment_url(url: &str) -> Result<ParsedCommentUrl, String>`
- `validate_comment_url_for_task(url: &str, task: &TaskRecord) -> Result<(), String>`

### 3. `src-tauri/src/cli/send.rs`

Current anchors:

- argument definition: lines 8-67
- runtime flow: lines 90-292

Change:

- Keep `--token`, `--root`, `--to`, `--mode`, `--command`, `--agent`, and `--outbox`.
- Change `--message` help text to “GitHub issue comment URL”.
- Remove `--message-file`.
- Remove `--get-output`.
- Remove `--timeout`.
- Reject `--command` and `--message` together.
- If `--message` is present:
  - resolve the active task via `task_records`
  - validate the comment URL against that task
  - write task metadata into `OutboxMessage`
- If `--command` is present:
  - skip task lookup
  - write no message metadata
- Keep the delivered/rejected polling logic.
- Remove request-id generation and response waiting for the message path.

Expected behavior:

- `send --message <comment-url>` notifies another agent about a GitHub comment.
- `send --command clear` remains a pure remote action.
- `send --command clear --message <comment-url>` is rejected.

### 4. `src-tauri/src/cli/mod.rs`

Current anchors: lines 10-16 and 27-37.

Change:

- Update top-level CLI help text so it no longer documents raw message bodies or `--get-output` for human messaging.

### 5. `src-tauri/src/cli/close_session.rs`

Current anchor: lines 90-108.

Change:

- Update the `OutboxMessage` initializer for the new optional fields.
- Set `message_url: None`.
- Set `task_slug: None`.
- Set `task_record_path: None`.
- Set `issue_number: None`.
- Keep `request_id` for action-style responses.

### 6. `src-tauri/src/phone/types.rs`

Current anchor: lines 3-42.

Change:

- Keep `body: String` as a legacy compatibility field so old queued files still deserialize.
- Keep `get_output: bool` only if needed for backward deserialization. New `send` always writes `false`.
- Add:
  - `message_url: Option<String>`
  - `task_slug: Option<String>`
  - `task_record_path: Option<String>`
  - `issue_number: Option<u64>`
- Keep `request_id` for non-message actions such as `close-session`.

Do not replace `body` outright. New `send` writes `body = ""`.

### 7. `src-tauri/src/phone/mailbox.rs`

Current anchors:

- action dispatch and delivery selection: lines 361-395
- PTY injection path: lines 753-980
- close-session response write: lines 1154-1180
- token refresh notice: lines 1715-1723

Change:

- Reject legacy raw-text message payloads:
  - if `message_url` is missing and `body` is non-empty
  - reason text must explicitly tell operators to repost via a GitHub issue comment URL
- Reject any message that mixes `command` and message metadata.
- Stop injecting raw message text into PTYs.
- Inject a short GitHub notification block that points the receiver to the comment URL.
- Remove marker-based reply instructions.
- Remove any post-command follow-up message injection path.
- Keep command delivery behavior.
- Keep `close-session` action handling and response-file writing unchanged.
- Update every inline `send` example to include `--root`.

Proposed PTY framing:

```text
[GitHub message from wg-6-dev-team/tech-lead]
Task: github-message-transport-gh59
Issue: #59
Comment: https://github.com/mblua/AgentsCommander/issues/59#issuecomment-1234567890

Read and reply in GitHub. After posting your reply comment, notify the sender:
"<binary>" send --token <your_token> --root "<your_root>" --to "wg-6-dev-team/tech-lead" --message "<reply_comment_url>" --mode wake
```

Migration rule for legacy queued outbox files:

- Keep deserialization backward-compatible.
- Reject old raw-text files with a clear reason such as:
  `Legacy raw-text outbox message. Repost the content as a GitHub issue comment and send the comment URL instead.`

### 8. `src-tauri/src/pty/manager.rs`

Current anchors:

- file comments about marker scanning near lines 22 and 417
- watcher storage and registration near lines 39 and 398
- marker parser near lines 430-530

Change:

- Delete the response-marker machinery that only exists for `send --get-output`:
  - `ResponseWatcherMap`
  - `ResponseWatcher`
  - `register_response_watcher`
  - `scan_response_markers`
  - marker-related cleanup in `kill()`

Do not change:

- PTY output relay
- idle detection
- resize handling
- VT100 snapshotting
- WS broadcasting

### 9. `src-tauri/src/config/session_context.rs`

Current anchor: lines 414-431.

Change:

- Replace raw-text messaging examples with:
  - `gh-message.ps1` for posting long-form content
  - `send --message <comment-url>` for notification
  - `--root` in every example

### 10. `README.md`

Current anchors:

- CLI docs: lines 196-236
- project structure scripts list: lines 311-313

Change:

- Remove raw-text messaging examples.
- Remove `--message-file`.
- Remove `--get-output` from `send`.
- Document `_plans/tasks/`.
- Document `task-new.ps1`.
- Document `gh-message.ps1`.
- Document the `gh auth status` prerequisite.
- Explain that `send --message` now accepts only issue comment URLs.

### 11. `CLAUDE.md`

Current anchor: line 46.

Change:

- Update runtime messaging examples and coordinator protocol notes so agents send GitHub comment URLs, not raw replies.

### 12. `ROLE_AC_BUILDER.md`

Current anchor: line 416.

Change:

- Update the embedded `send` example to use URL-only messaging and include `--root`.

### 13. New `scripts/task-new.ps1`

Suggested interface:

```powershell
.\scripts\task-new.ps1 `
  -Issue 59 `
  -Slug "github-message-transport-gh59" `
  -Summary "GitHub comment URL message transport and task records" `
  -Workgroup "wg-6-dev-team" `
  -Branch "feature/github-message-transport-gh59"
```

Responsibilities:

- Create `_plans/tasks/` if missing.
- Normalize slug to lowercase kebab-case.
- Generate timestamp with `yyyyMMdd_HHmmss`.
- Resolve issue metadata with `gh issue view <Issue> --json title,url` unless explicit overrides are supplied.
- Validate branch naming and issue suffix.
- Acquire a workgroup-scoped lock before scanning and writing.
- Re-scan for active-task conflicts under that lock.
- Reject duplicate active task records for the same workgroup.
- Stage JSON and Markdown files under temp names, then rename them into place only after both payloads are ready.
- Write both files in English.
- Print created paths and task id in English.

Locking rule:

- Use a lock file under `_plans/tasks/.locks/<workgroup>.lock` or equivalent.
- The script must release the lock on success and failure.

### 14. New `scripts/gh-message.ps1`

Suggested interface:

```powershell
# Post a comment to the active task issue and notify a peer
.\scripts\gh-message.ps1 `
  -BinaryPath "C:\path\to\agentscommander.exe" `
  -Token <token> `
  -Root "C:\path\to\repo\.ac-new\wg-6-dev-team\__agent_dev-rust" `
  -To "wg-6-dev-team/tech-lead" `
  -BodyFile ".\tmp\message.md"

# Notify with an already-created comment URL
.\scripts\gh-message.ps1 `
  -BinaryPath "C:\path\to\agentscommander.exe" `
  -Token <token> `
  -Root "C:\path\to\repo\.ac-new\wg-6-dev-team\__agent_dev-rust" `
  -To "wg-6-dev-team/tech-lead" `
  -CommentUrl "https://github.com/mblua/AgentsCommander/issues/59#issuecomment-1234567890"
```

Responsibilities:

- Resolve the active task from `-Root`.
- Validate current branch matches the task record.
- Validate `gh auth status`.
- Require `-Token` and `-BinaryPath` before posting if `-To` is provided.
- Post the comment to the linked GitHub issue when `-Body` or `-BodyFile` is supplied.
- Read the returned `html_url`.
- Validate that URL against the active task issue.
- If `-To` is provided, call:
  - `"<BinaryPath>" send --token <Token> --root "<Root>" --to "<To>" --message "<html_url>" --mode wake`
- Print English status output, including the final comment URL.

Failure contract:

- If notify mode is requested and `-Token` or `-BinaryPath` is unavailable, fail before posting the GitHub comment.
- This avoids posting content that never gets notified to the target agent.

Implementation note:

- Prefer `gh api repos/<owner>/<repo>/issues/<number>/comments --method POST` because it returns JSON with `html_url` directly.

### 15. Optional `.sh` wrappers

Only add `task-new.sh` and `gh-message.sh` if the team explicitly wants Git Bash or WSL parity in this phase. If added, they should be thin Bash wrappers over the PowerShell scripts, matching the existing `scripts/kill-dev.sh` pattern.

## Validation rules

### Shared rules

- Workgroup must resolve from a `.../.ac-new/wg-*/__agent_*` root.
- Task lookup scans `repo-*/_plans/tasks/*.json` under the sender workgroup directory.
- `status == "active"` is the only task status eligible for message transport.
- Exactly one active task must match the sender workgroup.
- Current repo branch must equal `task.branch.name`.
- `issue.url` must match `owner/repo/number`.
- `branch.name` must match `^(feature|fix|bug)/[a-z0-9][a-z0-9-]*-gh[0-9]+$`.
- The numeric suffix in `branch.name` must equal `issue.number`.

### `send --message`

- URL must match the GitHub issue comment URL pattern.
- URL owner, repo, and issue number must match the active task record.
- Raw text, empty strings, and `--message-file` are invalid.
- If `--command` is absent, `--message` is required.
- `--command` and `--message` together are invalid.

### `task-new.ps1`

- Reject missing workgroup, slug, summary, or issue metadata.
- Reject branch names that do not end in `-gh<issueNumber>`.
- Reject duplicate active tasks for the same workgroup.
- Lock, re-scan, then write.
- Write both files in English.

### `gh-message.ps1`

- Reject missing active task.
- Reject branch mismatch.
- Reject missing `gh` auth.
- Reject empty body file or empty stdin.
- If `-To` is provided, require `-Token` and `-BinaryPath` before posting.
- If `-To` is provided, normal `send` routing rules still apply.

## Risks and sequencing

### Key risks

1. Non-workgroup agents currently can send messages. The new URL-notify flow should reject message transport outside workgroup replicas until a task-ownership rule exists for those roots.
2. Existing docs and injected instructions omit `--root` in some places. The migration must fix those examples in the same change set.
3. The helper flow depends on `gh`. Missing install, failed auth, or missing issue-comment permission must surface as explicit errors.
4. Leaving response markers in `pty/manager.rs` after removing `send --get-output` keeps dead transport logic on the PTY hot path.
5. Already-queued raw-text outbox files will exist during rollout. They must fail with an explicit migration reason, not an opaque parse or validation error.

### Recommended sequencing

1. Add task record format, locking rules, and `task-new.ps1`.
2. Add Rust task resolution and `send` validation.
3. Change `OutboxMessage` and mailbox PTY framing to URL-only notifications with explicit legacy rejection.
4. Remove the dead PTY response-marker path.
5. Add `gh-message.ps1`.
6. Update README, `CLAUDE.md`, `ROLE_AC_BUILDER.md`, and `session_context.rs`.

## Blockers

None. The main product decision this plan locks in is that `send --get-output` is removed rather than repurposed.
