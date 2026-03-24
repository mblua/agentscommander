# Agents Commander — Runtime Resource Analysis

> Snapshot from production instance, 2026-03-24. Version 0.4.8, Windows 11 Pro.

---

## 1. Overview

Agents Commander is a multi-process application. A single running instance spawns a significant process tree due to its architecture: Tauri core, WebView2 for UI rendering, and PTY-managed shell sessions — each of which may run heavyweight processes like Claude Code.

**Total observed footprint: ~4.2 GB RAM across ~59 processes.**

The application itself (Tauri + WebView2) is lightweight. The dominant resource consumer is the user's shell sessions — specifically, AI coding agents (Claude Code) running inside them.

---

## 2. Process Tree Structure

```
agentscommander.exe (Tauri core)
├── msedgewebview2 (browser process — main)
│   ├── msedgewebview2 (GPU process)
│   ├── msedgewebview2 (utility)
│   ├── msedgewebview2 (renderer — sidebar window)
│   ├── msedgewebview2 (renderer — terminal window)
│   └── msedgewebview2 (renderer — additional views)
│
├── [per terminal session] cmd → conhost (PTY pair)
│   └── <user shell or agent>
│       └── (agent subprocess tree, if any)
│
└── ... repeated for each active session
```

Each terminal session creates a **cmd + conhost** pair via ConPTY. Whatever the user runs inside that session (a shell, Claude Code, Codex, etc.) becomes a subtree under that pair.

---

## 3. Resource Distribution by Component

| Component | Processes | RAM | % of Total | Notes |
|---|---|---|---|---|
| Tauri Core | 1 | 32 MB | 1% | Rust binary — very lean |
| WebView2 (UI) | 7 | ~611 MB | 15% | Chromium-based, expected for 2 windows |
| Claude Code instances | 6 | ~2,482 MB | 59% | Dominant consumer |
| Node.js (Claude internals) | ~13 | ~651 MB | 15% | MCP servers + runtime per Claude instance |
| Codex | 1 | 41 MB | 1% | Lightweight in comparison |
| cmd + conhost (PTY) | ~27 | ~239 MB | 6% | OS-level terminal hosting |
| bash + powershell | 4 | ~175 MB | 4% | Shell processes |
| **TOTAL** | **~59** | **~4.2 GB** | **100%** | |

### Visual breakdown

```
Claude Code sessions (6x)   ██████████████████████████████  59%  (~2.5 GB)
Node.js (Claude internals)  ████████                        15%  (~651 MB)
WebView2 (UI rendering)     ████████                        15%  (~611 MB)
PTY shells + OS processes   █████                           10%  (~446 MB)
Tauri core                  ▌                                1%  (~32 MB)
```

---

## 4. Per-Session Cost

Each terminal session with an AI agent has a predictable resource footprint:

| Agent running in session | RAM per session | Process count |
|---|---|---|
| Claude Code (idle/light context) | ~380-430 MB | ~6 (claude + 2 node + cmd chain) |
| Claude Code (heavy context) | ~650-750 MB | ~6 |
| Codex | ~70-100 MB | ~3 (codex + node + cmd) |
| Plain shell (PowerShell/bash) | ~15-25 MB | ~2 (cmd + conhost) |

**Claude Code internal breakdown:**

```
claude process         280-653 MB   (varies with conversation context size)
├── node (MCP server)   40-50 MB   (always present)
│   └── node (worker)   40-94 MB   (always present)
└── cmd (launcher)       ~9 MB
```

The two Node.js processes per Claude instance are part of Claude Code's internal architecture — they handle MCP server communication and are not user-configurable.

---

## 5. Application vs. User Workload

A critical distinction for understanding resource consumption:

| Layer | RAM | Description |
|---|---|---|
| **App shell** | ~643 MB | Tauri core (32 MB) + WebView2 (611 MB) — fixed cost |
| **User workload** | ~3,570 MB | Everything running inside terminal sessions — scales with session count |

The app shell is a **fixed cost** regardless of how many sessions are open. The user workload **scales linearly** with the number of active sessions and what's running in them.

**Scaling estimate:**

| Open sessions | Agent type | Expected total RAM |
|---|---|---|
| 1 session | Claude Code | ~1.1 GB |
| 3 sessions | Claude Code | ~2.0 GB |
| 6 sessions | Claude Code | ~3.6 GB |
| 6 sessions | Mixed (Claude + Codex + shells) | ~2.5 GB |
| 10 sessions | Claude Code | ~5.5 GB |

---

## 6. Key Takeaways

1. **Agents Commander itself is lightweight.** The Tauri core is 32 MB. Even including WebView2, the app shell is under 650 MB — comparable to a single Electron app.

2. **Claude Code is the dominant consumer.** Each instance uses 380-750 MB depending on context size, plus ~90-140 MB in Node.js subprocesses. With 6 sessions, that's ~3.1 GB just in Claude processes.

3. **The biggest lever for reducing memory is closing unused Claude Code sessions.** Switching from 6 to 3 active sessions would free ~1.5 GB.

4. **WebView2 is shared.** Unlike Electron (which bundles its own Chromium), Tauri uses the system's Edge WebView2 runtime. The 7 WebView2 processes handle both windows and are relatively efficient for a Chromium-based renderer.

5. **ConPTY overhead is minimal.** Each cmd + conhost pair is ~17 MB — negligible compared to the agents running inside them.

---

## 7. Example: Snapshot Detail

Captured 2026-03-24 at ~16:00, production instance running from `AppData\Local\Agents Commander\`.

```
[PID 47976] agentscommander         32 MB    66 threads   (Tauri core)
  [PID  8572] msedgewebview2       131 MB    49 threads   (browser main)
    [PID 42148] msedgewebview2      11 MB     7 threads   (utility)
    [PID 31888] msedgewebview2     204 MB    48 threads   (renderer)
    [PID 34240] msedgewebview2      38 MB    19 threads   (renderer)
    [PID  7436] msedgewebview2      19 MB     9 threads   (renderer)
    [PID  8328] msedgewebview2      88 MB    36 threads   (renderer)
    [PID 37260] msedgewebview2     121 MB    36 threads   (renderer)
  [PID 41704] cmd + conhost          17 MB   (session 1)
    [PID 38448] claude              653 MB   (heavy context)
      └── node → node               88 MB
  [PID 23732] cmd + conhost          17 MB   (session 2)
    [PID 34864] claude              288 MB
      └── node → node               91 MB
  [PID 44008] cmd + conhost          17 MB   (session 3)
    [PID 23740] claude              281 MB
      └── node → node               89 MB
  [PID 22184] cmd + conhost          17 MB   (session 4)
    [PID 44384] claude              288 MB
      └── node → node               96 MB
  [PID 35180] cmd + conhost          17 MB   (session 5)
    [PID  5484] claude              376 MB
      └── node → node               89 MB
  [PID 20156] cmd + conhost          18 MB   (session 6)
    [PID 49328] claude              596 MB
      └── node → node              171 MB
  [PID 49908] cmd + conhost          18 MB   (session 7)
    [PID 36924] node                 28 MB
      [PID 37692] codex              41 MB
```
