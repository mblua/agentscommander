# Linux QA Environment — Plan & Learnings

## Estado: Funcional
Branch: `chores/linux-qa-environment`

---

## Qué se logró

1. **Webtop container** corriendo con Ubuntu 24.04 XFCE accesible en `http://localhost:3000`
2. **Build completo** de agentscommander en Linux: `.deb`, `.rpm`, `.AppImage` — sin errores
3. **Instalación del .deb** v0.4.5 desde GitHub Releases
4. **Session bash funcionando** dentro de agentscommander en Linux
5. **Claude Code v2.1.81** autenticado y operacional dentro de una sesión
6. **Telegram Web** abierto y listo para testing del bridge

---

## Fix requerido: soporte Linux en settings

**Problema**: El default shell es `powershell.exe` con args `["-NoLogo"]` y el cwd fallback es `"C:\\"` — todo hardcodeado para Windows.

**Fix aplicado (runtime)**: Crear `~/.agentscommander/settings.json` en el container:
```json
{
  "defaultShell": "/bin/bash",
  "defaultShellArgs": [],
  "repoPaths": ["/config"],
  "agents": [],
  "telegramBots": [],
  "sidebarAlwaysOnTop": false,
  "raiseTerminalOnClick": true
}
```

**Fix pendiente (código)**: Modificar `src-tauri/src/config/settings.rs` para auto-detectar el OS:
- `settings.rs:46-47` — default shell debería ser `powershell.exe` en Windows, `/bin/bash` en Linux/macOS
- `session.rs:74` — cwd fallback `"C:\\"` debería ser `"/"` en Linux
- Estos son los únicos dos archivos que necesitan cambio para cross-platform

**El PTY code (`pty/manager.rs:75`) ya tiene `cfg!(windows)`** y maneja Linux correctamente.

---

## Arquitectura del ambiente

```
Host Windows
  └── Docker Desktop
       └── Container: agentscommander-linux-qa
            ├── Image: lscr.io/linuxserver/webtop:ubuntu-xfce
            ├── OS: Ubuntu 24.04 LTS (Noble Numbat)
            ├── Port: 3000 → XFCE desktop via browser
            ├── Volume: agentscommander-qa-config → /config
            ├── User: abc (PUID/PGID 1000)
            ├── Rust 1.94 + Node 20.20 + npm 10.8
            ├── Tauri deps: libwebkit2gtk-4.1-dev, librsvg2-dev, patchelf, libayatana-appindicator3-dev
            ├── Repo clone: /config/agentscommander/
            ├── Installed .deb: /usr/bin/agentscommander (v0.4.5)
            └── Config: /config/.agentscommander/settings.json (Linux-adapted)
```

---

## Troubleshooting documentado

### libappindicator3 vs libayatana-appindicator3
Ubuntu 24.04 reemplazó `libappindicator3-dev` por `libayatana-appindicator3-dev`. No instalar ambos — conflictan. El CI en `release.yml` usa `libappindicator3-dev` porque corre en Ubuntu 22.04.

### MSYS path conversion (Git Bash en Windows)
`docker exec` desde Git Bash convierte `/tmp/foo` a `C:/Users/.../Temp/foo`. Workaround:
```bash
MSYS2_ARG_CONV_EXCL="*" docker exec container_name ...
```

### PackageKit Permission Denied
Warning cosmético en apt — ignorar. No afecta instalación.

### DRI3 / EGL warnings al lanzar la app
```
libEGL warning: DRI3 error: Could not get DRI3 device
```
Normal en containers sin GPU. La app funciona con software rendering.

### Interacción con webtop via Playwright
Los clicks de Playwright sobre el canvas streameado **no coinciden** con las coordenadas reales del desktop remoto. Usar `xdotool` dentro del container para interacciones precisas:
```bash
MSYS2_ARG_CONV_EXCL="*" docker exec agentscommander-linux-qa bash -c \
  'DISPLAY=:1 xdotool mousemove X Y click 1'
```

### Sessions residuales tras cambio de shell
Si se cambia el shell en settings.json, borrar `sessions.json` antes de relanzar:
```bash
rm -f /config/.agentscommander/sessions.json
```

---

## Próximos pasos

- [ ] Implementar auto-detección de OS en `settings.rs` (default shell + cwd)
- [ ] Testear el Telegram bridge: Agents Commander → Claude Code → Telegram bot
- [ ] Documentar el flujo completo de testing E2E en Linux
- [ ] Evaluar si el `release.yml` necesita update para Ubuntu 24.04

---

## Comandos útiles

```bash
# Levantar container (si no existe)
docker run -d --name agentscommander-linux-qa --security-opt seccomp=unconfined \
  -e PUID=1000 -e PGID=1000 -e TZ=America/Argentina/Buenos_Aires \
  --shm-size="1gb" -p 3000:3000 -v agentscommander-qa-config:/config \
  --restart unless-stopped lscr.io/linuxserver/webtop:ubuntu-xfce

# Lanzar app
MSYS2_ARG_CONV_EXCL="*" docker exec agentscommander-linux-qa \
  bash -c "DISPLAY=:1 su abc -c 'agentscommander &'"

# Click "+ New Session" en sidebar (coordenadas del container display)
MSYS2_ARG_CONV_EXCL="*" docker exec agentscommander-linux-qa \
  bash -c 'DISPLAY=:1 xdotool mousemove 1280 1000 click 1'

# Rebuild desde source dentro del container
MSYS2_ARG_CONV_EXCL="*" docker exec agentscommander-linux-qa \
  su - abc -c "source /config/.cargo/env && cd /config/agentscommander && npm run tauri build"
```

---

## Monitor para Playwright

Usar DISPLAY5 (monitor superior, Y=-2880, 2560x1440). Ver `.claude/memory/user_monitor_layout.md` para el procedimiento CDP.
