; AgentsCommander NSIS installer hooks
; Adds/removes the install directory to/from the user PATH so
; agents can call `agentscommander.exe` without the full path.

!include "WinMessages.nsh"

; ── Post-install: add $INSTDIR to user PATH ──────────────────────
!macro NSIS_HOOK_POSTINSTALL
  ; Write a temp PowerShell script that handles dedup
  FileOpen $0 "$TEMP\ac_addpath.ps1" w
  FileWrite $0 `$$p = [Environment]::GetEnvironmentVariable("Path", "User")$\r$\n`
  FileWrite $0 `if (-not $$p) { $$p = "" }$\r$\n`
  FileWrite $0 `$$entries = $$p -split ";" | Where-Object { $$_ -ne "" }$\r$\n`
  FileWrite $0 `if ("$INSTDIR" -notin $$entries) {$\r$\n`
  FileWrite $0 `  $$entries += "$INSTDIR"$\r$\n`
  FileWrite $0 `  [Environment]::SetEnvironmentVariable("Path", ($$entries -join ";"), "User")$\r$\n`
  FileWrite $0 `}$\r$\n`
  FileClose $0
  nsExec::ExecToLog 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$TEMP\ac_addpath.ps1"'
  Delete "$TEMP\ac_addpath.ps1"
  ; Notify running processes of the change
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

; ── Pre-uninstall: remove $INSTDIR from user PATH ────────────────
!macro NSIS_HOOK_PREUNINSTALL
  FileOpen $0 "$TEMP\ac_rmpath.ps1" w
  FileWrite $0 `$$p = [Environment]::GetEnvironmentVariable("Path", "User")$\r$\n`
  FileWrite $0 `if ($$p) {$\r$\n`
  FileWrite $0 `  $$n = ($$p -split ";" | Where-Object { $$_ -ne "" -and $$_ -ne "$INSTDIR" }) -join ";"$\r$\n`
  FileWrite $0 `  [Environment]::SetEnvironmentVariable("Path", $$n, "User")$\r$\n`
  FileWrite $0 `}$\r$\n`
  FileClose $0
  nsExec::ExecToLog 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$TEMP\ac_rmpath.ps1"'
  Delete "$TEMP\ac_rmpath.ps1"
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend
