# RTK (Rust Token Killer) - Plugin Implementation Guide

## What is RTK?

RTK is a CLI proxy installed on this machine that compresses command outputs to reduce token consumption. It works by filtering and condensing verbose tool output before it reaches the LLM context window.

- **Repo:** https://github.com/rtk-ai/rtk
- RTK only compresses output from Bash tool calls, not native Claude Code tools (Read, Grep, Glob)
- If RTK has a dedicated filter for a command, it compresses the output. If not, it passes through unchanged. This means RTK is always safe to use.

## Installation Modes

RTK has two modes depending on the platform:

### Unix (macOS/Linux) - Hook mode
On Unix, RTK can install a shell hook in `settings.json` that automatically intercepts Bash tool calls. This is the preferred mode:

```bash
rtk init -g --auto-patch   # Installs hook in ~/.claude/settings.json + RTK.md
```

This adds a `PreToolUse` hook to `settings.json` that wraps all Bash commands with RTK automatically - no need for manual `rtk` prefixing.

### Windows - CLAUDE.md mode (current machine)
On Windows, RTK does NOT support hooks. The `rtk init -g` command falls back to `--claude-md` mode automatically. The mechanism is:

1. An instruction block in `CLAUDE.md` tells Claude to prefix commands with `rtk`
2. Claude reads the instruction and applies `rtk` manually to each Bash call
3. The `[rtk] /!\ No hook installed` warning is **expected on Windows** - it cannot be resolved because hooks are Unix-only

**The warning is cosmetic noise.** RTK works correctly on Windows via the CLAUDE.md instructions. The compression happens regardless of whether a hook or manual prefix is used.

## Per-Repo Setup (Windows)

Add the condensed instruction block at the end of the project's `CLAUDE.md`:

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

**Do NOT use `rtk init`** for per-repo setup - it injects the full verbose block (~1,400 tokens). The condensed block above is ~200 tokens and equally effective since the full command reference is already in the global `~/.claude/CLAUDE.md`.

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

- The condensed instruction block (~200 tokens) is 85% smaller than the full version from `rtk init` (~1,400 tokens)
- The HTML comments (`<!-- rtk-instructions -->`) serve as markers to easily locate and update the block across repos
- `rtk init --show` reports the current configuration status for the repo
- The `[rtk] /!\ No hook installed` warning on Windows is a known limitation and can be ignored
