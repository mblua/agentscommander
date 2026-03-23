---
name: Monitor layout and Playwright usage
description: User's 5-monitor setup with positions, and which monitor to use for browser automation (DISPLAY5, the topmost)
type: user
---

User has 5 monitors. Layout (from PowerShell `[System.Windows.Forms.Screen]::AllScreens`):

| Display | Position | Resolution | Notes |
|---|---|---|---|
| DISPLAY1 (Primary) | X=0, Y=0 | 1707x1067 | Main/primary, likely laptop screen |
| DISPLAY5 | X=0, Y=-2880 | 2560x1440 | **Topmost monitor** — use this for browser automation |
| DISPLAY6 | X=-1080, Y=-1935 | 1080x1920 | Vertical/portrait monitor, to the left |
| DISPLAY7 | X=0, Y=-1440 | 2560x1440 | Middle monitor (between DISPLAY5 and primary) |
| DISPLAY12 | X=-1920, Y=0 | 1280x720 | Far left |

When moving Playwright browser to DISPLAY5 (topmost), use CDP:
1. Un-maximize first (`windowState: 'normal'`)
2. Move to `left: 100, top: -2800` (within DISPLAY5 bounds)
3. Then maximize (`windowState: 'maximized'`)
4. Set viewport to 2560x1400

User explicitly prefers DISPLAY5 (topmost, Y=-2880) for browser automation — NOT DISPLAY7 (Y=-1440).
