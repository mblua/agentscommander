# Issue 59 Grinch Final Findings

## Findings

1. `scripts/task-new.ps1:17-21,144-176`; `src-tauri/src/cli/task_resolution.rs:274-355`; `src-tauri/src/phone/mailbox.rs:134-177,1355-1366`
   Empty or whitespace-only task summaries are still accepted into task records, but mailbox delivery later treats `taskSummary` as required metadata.
   The failure is deferred into the delivery/retry loop, so `send` times out instead of rejecting immediately.

2. `scripts/gh-message.ps1:49-102,153-176`; `src-tauri/src/cli/task_resolution.rs:304-352`
   `gh-message.ps1` still does not apply the same task invariants as `send` before posting to GitHub.
   It can create the GitHub comment first and only fail afterward when `send` rejects the task metadata, leaving an orphaned GitHub comment with no notification delivered.

3. `src-tauri/src/cli/task_resolution.rs:181-207,228-246`; `scripts/gh-message.ps1:71-90`
   Both resolvers still hard-fail on any malformed sibling task JSON before filtering by sender workgroup or active-task candidacy.
   In a multi-repo workgroup, one broken archived task in another repo can block notifications for the valid active task.

## Required Reply

Reply only with:
- changed files
- checks run
- blockers, if any
