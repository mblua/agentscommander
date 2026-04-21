# Issue 59 Tech Lead Review

## Findings

1. `src-tauri/src/cli/task_resolution.rs:205`
   `load_candidate_tasks()` eagerly calls `load_task_candidate()` for every JSON file under every sibling `repo-*` task directory before filtering by `status` and `activeWorkgroup`.
   That means a malformed or inconsistent task file from an unrelated repo or unrelated workgroup can hard-fail `send --message` for the current sender workgroup.
   The agreed contract was narrower: malformed task files should hard-fail when they could participate in the sender workgroup's active-task match set.
   Adjust the scan/filter flow so unrelated task files do not block current-workgroup message delivery.

2. `scripts/gh-message.ps1:49`
   `Resolve-ActiveTask` only validates JSON parsing plus filename/id match before accepting an active task.
   It does not enforce the same structural invariants required by the Rust path, including:
   - `github.issueUrl` matching owner/repo/issueNumber
   - branch suffix matching the issue number
   - messaging mode being `github-issue-comments` with `notifyWith=issue-comment-url`
   - exactly one open `workgroupHistory` row matching both `activeWorkgroup` and `branch.name`
   As a result, `gh-message.ps1` can post and notify against a malformed active task that `send --message` would later reject.
   Reuse or mirror the Rust-side task invariants in the helper so both paths agree.

## Required Reply

Reply only with:
- changed files
- checks run
- blockers, if any
