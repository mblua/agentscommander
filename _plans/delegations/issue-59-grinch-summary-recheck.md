Issue: `mblua/AgentsCommander#59`
Branch: `feature/github-message-transport-gh59`

Please re-check the remaining summary-validation finding against the current working tree.

Current code appears to reject whitespace-only summaries in all three paths you referenced:
- `scripts/task-new.ps1:57-61` and `scripts/task-new.ps1:139-144`
- `scripts/gh-message.ps1:31-38`, `scripts/gh-message.ps1:163-180`
- `src-tauri/src/cli/task_resolution.rs:300-305`

Mailbox still requires non-empty `taskSummary` metadata at:
- `src-tauri/src/phone/mailbox.rs:1361-1366`

Question:
- Is there still a real bypass that lets a whitespace-only summary survive task creation/resolution and reach mailbox delivery?

Reply format:
- `PASS` if the previous finding is stale on the current branch
- otherwise `FAIL` with the concrete bypass path and file references
