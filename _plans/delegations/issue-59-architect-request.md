# Issue 59 Architect Request

## Repo
`C:\Users\maria\0_repos\agentscommander\.ac-new\wg-6-dev-team\repo-AgentsCommander`

## Branch
`feature/github-message-transport-gh59`

## Tracking Issue
`https://github.com/mblua/AgentsCommander/issues/59`

## Required Output
Create this plan file:

`repo-AgentsCommander/_plans/issue-59-github-message-transport.md`

Reply only with:
- the plan file path
- any blocker

## Requirement
- Long-form agent message content must always live in GitHub issue comments.
- `send --message` must become GitHub issue comment URL only.
- There is no valid raw-text messaging path anymore.
- `--message-file` must be removed for messaging.
- Receivers read and respond in GitHub and use `send` only to notify the reply comment URL.
- Add project-level task records under `_plans/tasks/` named `YYYYMMDD_HHmmss-slug.{json,md}`.
- The task record must include summary, status, activeWorkgroup, workgroupHistory, GitHub issue linkage, branch linkage, and messaging mode.
- One active task per workgroup. Helper and send validation should reject `0` or `>1` matches.
- Add helper scripts under `scripts/`: `task-new.ps1` and `gh-message.ps1`. Add optional `.sh` wrappers only if justified.
- All generated markdown, script output, and GitHub content must be English.
- Branch naming must be issue-based, for example `feature/<slug>-gh59`.

## What The Plan Must Contain
- Affected files and exact responsibilities per file.
- Proposed task JSON schema and markdown template.
- Proposed PowerShell helper interfaces and validation rules.
- Migration steps for `send`, mailbox delivery framing, and README/help text.
- Risks, sequencing, and anything that should be split.

## Constraints
- Do not implement code yet.
- Persist the plan inside `_plans/`.
- Work against the current branch content, not `main`.
