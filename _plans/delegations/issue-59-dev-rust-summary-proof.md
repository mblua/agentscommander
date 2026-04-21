Issue: `mblua/AgentsCommander#59`
Branch: `feature/github-message-transport-gh59`

Please validate the remaining summary-only finding with an explicit negative test.

What to test:
- `scripts/task-new.ps1` with `-Summary '   '` must fail before writing task files
- if practical, also confirm the active-task resolution path rejects a task record whose `summary` is whitespace-only before mailbox delivery

Return format:
- exact command(s) run
- observed result
- `PASS` if whitespace-only summaries are rejected before delivery
- otherwise `FAIL` with the concrete bypass path
