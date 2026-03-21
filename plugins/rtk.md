# RTK (Rust Token Killer) - Plugin Implementation Guide

## What is RTK?

RTK is a CLI proxy installed on this machine that compresses command outputs to reduce token consumption. It works by filtering and condensing verbose tool output before it reaches the LLM context window.

- **Repo:** https://github.com/rtk-ai/rtk
- RTK only compresses output from Bash tool calls, not native Claude Code tools (Read, Grep, Glob)
- On Windows it uses `--claude-md` mode (instructions in prompt) instead of a shell hook
- If RTK has a dedicated filter for a command, it compresses the output. If not, it passes through unchanged. This means RTK is always safe to use.

## Requirements

1. **RTK must be installed on the machine.** It is a global CLI tool, not a per-repo dependency.
2. **Add the RTK instruction block to the project's `CLAUDE.md`.** This is the only per-repo step needed.

## How to Implement

Add the following block at the end of your `CLAUDE.md`:

```markdown
<!-- rtk-instructions -->
## RTK (Token Optimizer)

`rtk` is a CLI proxy installed on this machine that compresses command outputs to reduce tokens.

**Rule:** ALWAYS prefix Bash commands with `rtk`. If RTK has a filter for that command, it compresses the output. If not, it passes through unchanged. It is always safe to use.

In command chains with &&, prefix each command:
rtk git add . && rtk git commit -m "msg" && rtk git push

Applies to: git, gh, cargo, npm, pnpm, npx, tsc, vitest, playwright, pytest, docker, kubectl, ls, grep, find, curl, and any other command.

Meta: `rtk gain` to view token savings statistics, `rtk discover` to find missed RTK usage opportunities.
<!-- /rtk-instructions -->
```

## Token Savings Overview

| Category | Commands | Typical Savings |
|----------|----------|-----------------|
| Tests | vitest, playwright, cargo test | 90-99% |
| Build | next, tsc, lint, prettier | 70-87% |
| Git | status, log, diff, add, commit | 59-80% |
| GitHub | gh pr, gh run, gh issue | 26-87% |
| Package Managers | pnpm, npm, npx | 70-90% |
| Files | ls, read, grep, find | 60-75% |
| Infrastructure | docker, kubectl | 85% |
| Network | curl, wget | 65-70% |

Overall average: **60-90% token reduction** on common development operations.

## Notes

- The condensed instruction block (~680 chars, ~200 tokens) is 85% smaller than the full version from `rtk init` (~4,757 chars, ~1,400 tokens).
- The HTML comments (`<!-- rtk-instructions -->`) serve as markers to easily locate and update the block across repos.
