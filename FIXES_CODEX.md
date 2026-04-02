# FIXES_CODEX.md — ACRC Credential Injection Issues

**Fecha**: 2026-04-01  
**Branch**: `fix/acrc-codex-injection`  
**Afecta**: Sesiones Codex (Bug 1) y Claude (Bug 2)

---

## Bug 1: Codex nunca recibe credenciales tras emitir `%%ACRC%%`

### Observado

La sesion `mi-agents/codex-expert` imprimio `%%ACRC%%` como texto visible en la terminal. El marker aparece en el output del PTY. Sin embargo, el sistema nunca inyecto el bloque `# === Session Credentials ===`. El agente quedo esperando indefinidamente.

### Causa raiz: `strip_ansi_csi` no maneja secuencias OSC

**Archivo**: `src-tauri/src/pty/manager.rs`, lineas 57-80

La funcion `strip_ansi_csi` solo stripea dos tipos de secuencias ANSI:

- **CSI** (`ESC [` + params + byte final en `0x40..=0x7E`) — colores, cursor, SGR
- **Non-CSI de 2 bytes** (`ESC` + 1 byte que no sea `[`) — resets simples como `ESC c`, `ESC M`

**No maneja**:

- **OSC** (`ESC ]` ... terminado por `BEL` `\x07` o `ST` `ESC \`) — Codex usa OSC 133 para shell integration y OSC 8 para hyperlinks
- **DCS** (`ESC P` ... terminado por `ST` `ESC \`) — Device Control Strings

Cuando Codex imprime `%%ACRC%%`, el output real del PTY contiene secuencias OSC 133 (shell integration marks) envolviendo el texto:

```
ESC]133;C\x07%%ACRC%%ESC]133;D;0\x07
```

El branch `else` de `strip_ansi_csi` (linea 71) solo consume UN byte despues de `ESC` — el `]`. El payload OSC completo queda en el string "limpio":

```
133;C\x07%%ACRC%%133;D;0\x07
```

El check en linea 209 `strip_ansi_csi(line).trim() == "%%ACRC%%"` falla porque el string contiene basura OSC alrededor del marker.

### Causa secundaria: inyeccion via PTY stdin puede no funcionar en Codex

**Archivo**: `src-tauri/src/pty/inject.rs`, lineas 17-92

Incluso si la deteccion funcionara, la inyeccion usa `submit = false` (llamada desde `manager.rs:494-501`). Esto escribe bytes raw al PTY stdin sin enviar Enter. 

- **Claude Code** consume estos bytes activamente — su readline loop procesa todo lo que llega a stdin como input del usuario.
- **Codex** tiene un TUI diferente que puede no consumir stdin injections de la misma forma. Los bytes podrian quedar en el buffer del OS pipe sin ser leidos por la aplicacion.

### Fix propuesto (P0)

Agregar manejo de OSC y DCS a `strip_ansi_csi`:

```rust
fn strip_ansi_csi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next(); // consume '['
                    // CSI: consume hasta byte final 0x40..=0x7E
                    while let Some(&ch) = chars.peek() {
                        if (ch as u32) >= 0x40 && (ch as u32) <= 0x7E {
                            chars.next();
                            break;
                        }
                        chars.next();
                    }
                }
                Some(']') => {
                    chars.next(); // consume ']'
                    // OSC: consume hasta BEL (\x07) o ST (ESC \)
                    while let Some(&ch) = chars.peek() {
                        if ch == '\x07' {
                            chars.next();
                            break;
                        }
                        if ch == '\x1b' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                        chars.next();
                    }
                }
                Some('P') => {
                    chars.next(); // consume 'P'
                    // DCS: consume hasta ST (ESC \)
                    while let Some(&ch) = chars.peek() {
                        if ch == '\x1b' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                        chars.next();
                    }
                }
                Some(_) => {
                    chars.next(); // non-CSI 2-byte escape
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}
```

**Tambien investigar**: si Codex consume PTY stdin injections. Si no, se necesita un mecanismo alternativo de delivery (e.g., escribir credenciales a un archivo temporal que Codex pueda leer, o usar un Codex-specific CLI flag).

---

## Bug 2: Claude recibe inyecciones excesivas (4x sin trabajo real)

### Observado

La sesion `phi_phibridge-ui/ac_tech_lead` recibio 4 bloques `# === Session Credentials ===` consecutivos. Cada vez el agente respondio "listo, esperando instrucciones" pero nadie le envio trabajo. El patron se repitio 4 veces.

### Causa raiz: feedback loop comportamental

El ciclo es:

1. Llega un mensaje via `deliver_wake` (`mailbox.rs:358-381`) — el agente se despierta
2. El agente emite `%%ACRC%%` para obtener credenciales (como lo indica su system prompt)
3. El PTY reader detecta el marker, pasa el debounce `acrc_pending`, inyecta credenciales
4. El bloque de credenciales termina con `\r` (`manager.rs:486`) — esto causa que Claude Code trate el bloque como un nuevo turno de conversacion
5. Claude responde "listo" y va a idle → `waiting_for_input = true`
6. El siguiente mensaje en cola llega via `deliver_wake` → se repite desde paso 1

El debounce `acrc_pending` (`manager.rs:210-223`) solo previene inyecciones **concurrentes** para la misma sesion (mientras el async task esta en vuelo). Una vez que la inyeccion completa y se remueve el ID del `HashSet`, una nueva deteccion es permitida inmediatamente.

### Detalle: el `\r` como amplificador

**Archivo**: `src-tauri/src/pty/manager.rs`, linea 486

```rust
let cred_block = format!(
    concat!(
        "\n",
        "# === Session Credentials ===\n",
        "# Token: {token}\n",
        "# Root: {root}\n",
        "# === End Credentials ===\n",
        "\r",   // <-- este \r causa que Claude trate el bloque como input
    ),
    ...
);
```

La inyeccion usa `submit = false` (no envia Enter adicional), pero el propio texto contiene un `\r` literal al final. En Claude Code, esto se interpreta como submit del contenido del buffer de input.

### Fix propuesto (P2)

Rate-limit ACRC por sesion: cooldown de ~10 segundos despues de una inyeccion exitosa.

```rust
// En PtyManager o como estructura separada
pub type AcrcCooldownMap = Arc<Mutex<HashMap<Uuid, std::time::Instant>>>;

// En el read loop, despues de detectar el marker:
let now = std::time::Instant::now();
let in_cooldown = acrc_cooldowns.lock()
    .map(|map| map.get(&id)
        .map(|last| now.duration_since(*last) < Duration::from_secs(10))
        .unwrap_or(false))
    .unwrap_or(false);

if has_standalone_marker && !in_cooldown {
    // ... inyectar y registrar timestamp
    acrc_cooldowns.lock().map(|mut map| map.insert(id, now));
}
```

---

## Tabla de prioridades

| Prioridad | Fix | Archivo | Complejidad |
|---|---|---|---|
| **P0** | Agregar stripping de OSC/DCS a `strip_ansi_csi` | `manager.rs:57-80` | Baja |
| **P1** | Investigar si Codex consume PTY stdin injections | `inject.rs` | Media (requiere testing) |
| **P2** | Rate-limit ACRC por sesion (cooldown 10s) | `manager.rs:210-223` | Baja |

---

## Archivos clave

- `src-tauri/src/pty/manager.rs` — Read loop (181-263), `strip_ansi_csi` (57-80), `acrc_pending` (210-223), `inject_credentials` (465-512)
- `src-tauri/src/pty/inject.rs` — `inject_text_into_session`, `needs_explicit_enter`, semantica de `submit` flag (17-92)
- `src-tauri/src/commands/session.rs` — Session creation, Claude vs Codex divergencia (56-156)
- `src-tauri/src/config/session_context.rs` — Context delivery: Claude via `--append-system-prompt-file`, Codex via `~/.codex/config.toml` (33-173)
- `src-tauri/src/phone/mailbox.rs` — `deliver_wake` (358-381), `inject_into_pty` (507-655)
- `_logbooks/fix__acrc-false-positive-detection.md` — Historial del fix previo de false positives
