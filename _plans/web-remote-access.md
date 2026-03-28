# Plan: Web Remote Access para AgentsCommander

## Context

AgentsCommander es una app de escritorio (Tauri + SolidJS + xterm.js) que maneja sesiones de terminal. El objetivo es que la app levante un web server embebido que sirva la misma UI (Sidebar + Terminal) a cualquier browser, permitiendo acceso remoto desde Chrome desktop o celular.

**Por que es viable:** El frontend ya es 100% web (SolidJS + xterm.js) y todo el IPC esta centralizado en `src/shared/ipc.ts`. Solo hay que crear un adapter WebSocket alternativo y un server HTTP en Rust.

---

## Secuencia de Implementacion

### Paso 1: Rust WebSocket Server + Auth

**Objetivo:** Axum server embebido que corra junto a Tauri, compartiendo el mismo estado.

**Deps nuevas en `src-tauri/Cargo.toml`:**
```toml
axum = "0.8"
tower-http = { version = "0.6", features = ["fs", "cors"] }
```

**Archivos a crear:**
- `src-tauri/src/web/mod.rs` — Router axum, `start_server()`, WS upgrade handler con validacion de token
- `src-tauri/src/web/commands.rs` — Dispatch de comandos WS: JSON `{id, cmd, args}` → match cmd → call inner function → response `{id, result}` o `{id, error}`
- `src-tauri/src/web/broadcast.rs` — `WsBroadcaster`: mantiene lista de `UnboundedSender<WsOutMsg>` por conexion, fan-out de eventos y PTY output. Usa `Vec::retain(|tx| tx.send(msg).is_ok())` para limpiar senders muertos automaticamente.
- `src-tauri/src/web/auth.rs` — Validacion de `WebAccessToken` (token dedicado para WS, separado del `MasterToken`)

**Archivos a modificar:**
- `src-tauri/src/lib.rs`:
  - `pub mod web;`
  - Generar `WebAccessToken` (segundo UUID) al startup, imprimirlo como `[web-token] <uuid>`
  - Construir `WsBroadcaster` ANTES de `Builder`, registrarlo con `.manage()`
  - Dentro de `setup()`: clonar `pty_mgr` Arc DESPUES de su creacion (linea ~188), luego llamar `web::start_server()` pasando los clones
  - **Restriccion de orden:** `PtyManager` se crea dentro de `setup()` (necesita `app.handle()` para GitWatcher). Clonar el Arc inmediatamente despues de crearlo, antes de pasarlo a `start_server()`
- `src-tauri/Cargo.toml` — agregar deps

**Patron de state sharing:**
Los Arcs que se crean ANTES de `Builder` (`session_mgr`, `settings`, `broadcaster`) se clonan directamente.
Los Arcs que se crean DENTRO de `setup()` (`pty_mgr`) se clonan inmediatamente despues de su creacion.
Todos se pasan a `web::start_server()` como parametros explicitos.
El `WsBroadcaster` tambien se registra via `.manage()` para que los command handlers accedan con `app.state::<WsBroadcaster>()`.

**Protocolo WS:**
- Text frames: JSON RPC `{id, cmd, args}` → `{id, result}` / `{id, error}`
- Text frames (server→client): eventos `{event, payload}`
- Binary frames (client→server): PTY write — `[36 bytes UUID ASCII][raw bytes]`
- Binary frames (server→client): PTY output — `[36 bytes UUID ASCII][raw bytes]`

**Auth:** Token dedicado `WebAccessToken` (no el MasterToken) en query param del WS upgrade: `ws://host:port/ws?token=<webToken>`. Static files (HTML/JS/CSS) NO requieren auth. Esto limita el blast radius: si se captura el web token por sniffing, NO otorga acceso CLI completo (send/list-peers).

---

### Paso 2: PTY Output Broadcasting + Screen Replay

**Objetivo:** Agregar broadcast WS al read loop del PTY sin romper el flujo Tauri existente. Mantener estado de pantalla para replay a clientes que se conectan tarde.

**Archivo a modificar:**
- `src-tauri/src/pty/manager.rs`
  - Agregar campo `WsBroadcaster` a `PtyManager`
  - Agregar `vt100::Parser` por sesion en un `HashMap<Uuid, vt100::Parser>` (el crate ya esta en Cargo.toml)
  - En el `std::thread::spawn` del read loop, despues de `app_handle.emit("pty_output", payload)`:
    1. `broadcaster.broadcast_pty_output(&session_id_str, &data)` — fan-out a WS clients
    2. Alimentar los bytes al `vt100::Parser` de esa sesion — mantener screen state
  - `broadcast_pty_output` usa `UnboundedSender::send()` (no-async, no bloquea el thread nativo)
  - Nuevo metodo: `get_screen_snapshot(session_id) -> Option<Vec<u8>>` — retorna el contenido visible del parser vt100 como bytes ANSI para replay

**Archivo a modificar:**
- `src-tauri/src/lib.rs` — pasar broadcaster a `PtyManager::new()`

**Emision dual de eventos via helper:**
En vez de tocar cada command handler individualmente, crear helper:
```rust
fn broadcast_all(app: &AppHandle, event: &str, payload: impl Serialize + Clone) {
    let _ = app.emit(event, payload.clone());
    if let Some(bc) = app.try_state::<WsBroadcaster>() {
        bc.broadcast_event(event, &payload);
    }
}
```
Reemplazar cada `app.emit(event, payload)` en los command handlers por `broadcast_all(&app, event, payload)`.
Eventos afectados: `session_created`, `session_destroyed`, `session_switched`, `session_renamed`, `session_idle`, `session_busy`, `session_git_branch`, `last_prompt`, `telegram_bridge_*`.

**Screen replay flow:**
1. WS client envia comando `subscribe_session {sessionId}`
2. Server llama `pty_mgr.get_screen_snapshot(session_id)`
3. Envia snapshot como binary frame inicial (mismo formato: UUID prefix + bytes)
4. A partir de ahi, el client recibe output incremental via broadcast

---

### Paso 3: Static File Serving

**Objetivo:** Servir el build de Vite (`dist/`) via el mismo server HTTP.

**Archivo a modificar:**
- `src-tauri/src/web/mod.rs` — agregar `tower_http::services::ServeDir` como fallback del Router:
  ```rust
  .fallback_service(ServeDir::new(&dist_path).append_index_html_on_directories(true))
  ```

**Resolucion de `dist_path`:**
1. Dev mode: `../dist` relativo al ejecutable (output de `npm run build`)
2. Produccion: `dist/` NO existe en disco (Tauri embebe assets en el binario)

**Limitacion MVP:** El web server solo funciona cuando `dist/` existe en disco. Para produccion hay 3 opciones futuras:
- `include_dir!` macro para embeber dist/ en el binario de axum
- Extraer assets de Tauri's embedded bytes a un temp dir al startup
- Documentar que el web server requiere `dist/` co-localizado con el exe instalado

Para el MVP, usar `npm run build` antes de `npm run tauri dev` genera el `dist/` necesario.

Resultado: `http://host:port/?window=sidebar` carga el sidebar, `http://host:port/?window=terminal` carga el terminal.

---

### Paso 4: Transport Abstraction (Frontend)

**Objetivo:** Que `ipc.ts` delegue a Tauri o WebSocket segun el entorno, sin cambiar NINGUN componente consumidor.

**Archivos a crear:**
- `src/shared/transport.ts` — Interface:
  ```typescript
  export interface Transport {
    invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
    listen<T>(event: string, callback: (payload: T) => void): Promise<() => void>;
    writePtyBinary?(sessionId: string, data: Uint8Array): void;
  }
  ```
  El return type de `listen()` es `Promise<() => void>` — compatible con `UnlistenFn` de Tauri y con `onCleanup()` de SolidJS.

- `src/shared/transport-tauri.ts` — Implementacion via `@tauri-apps/api` (lo que hace ipc.ts hoy)

- `src/shared/transport-ws.ts` — Implementacion via WebSocket:
  - Conexion: `ws://${location.host}/ws?token=<token>` (token de sessionStorage)
  - `invoke()` → envia JSON `{id, cmd, args}`, espera response con mismo `id` via `Map<number, {resolve, reject}>` pending-requests
  - `listen()` → registra callback en `Map<string, Set<callback>>` event registry, retorna `() => void` que remueve el callback
  - `writePtyBinary()` → envia binary frame (UUID + bytes)
  - PTY output (binary frame entrante) → parsea UUID prefix, convierte resto a `number[]` para mantener contrato `PtyOutputEvent.data`, dispatchea a listeners de `pty_output`
  - Reconexion con exponential backoff (1s, 2s, 4s, max 10s). Pending invocations se rejectan al desconectar.
  - Al reconectar, re-envia `subscribe_session` para el session activo (trigger screen replay)

**Archivo a modificar:**
- `src/shared/ipc.ts` — Deteccion de entorno al inicio:
  ```typescript
  const transport = '__TAURI_INTERNALS__' in window ? new TauriTransport() : new WsTransport();
  ```
  Todas las APIs delegan a `transport.invoke()` / `transport.listen()`. Firma publica NO cambia.

**Token flow en browser:**
- URL: `http://host:port/?window=sidebar&remoteToken=abc123`
- `src/main.tsx` lee `remoteToken` de URL, guarda en `sessionStorage`
- `WsTransport` lee de `sessionStorage` para abrir el WS

---

### Paso 5: Window API Guards

**Objetivo:** Que el frontend no crashee en browser por llamadas a APIs de Tauri que no existen.

**Archivo a crear:**
- `src/shared/platform.ts`:
  ```typescript
  export const isTauri = '__TAURI_INTERNALS__' in window;
  export const isBrowser = !isTauri;
  ```

**Archivos a modificar (guards `if (isTauri)`):**

| Archivo | Que guardar |
|---|---|
| `src/sidebar/components/Titlebar.tsx` | Botones minimize/close → `<Show when={isTauri}>` |
| `src/terminal/components/Titlebar.tsx` | Botones minimize/maximize/close → `<Show when={isTauri}>` |
| `src/sidebar/App.tsx` | `getCurrentWindow().setAlwaysOnTop()`, `getCurrentWindow().setFocus()` |
| `src/terminal/App.tsx` | `getCurrentWindow().close()` en detached mode |
| `src/sidebar/components/SessionItem.tsx` | `WebviewWindow.getByLabel()`, ocultar boton detach/explorer en browser |
| `src/shared/zoom.ts` | `initZoom()` → bail early, usar CSS zoom en browser |
| `src/shared/window-geometry.ts` | `initWindowGeometry()` → no-op en browser |
| `src/shared/window-layout.ts` | `applyWindowLayout()` → no-op en browser |

**Navegacion en browser:** Dos tabs separadas (sidebar y terminal), identico al modelo actual de dos ventanas. Link "Open Terminal" en el sidebar para abrir segunda tab.

---

### Paso 6: Configuracion

**Archivo a modificar:**
- `src-tauri/src/config/settings.rs` — Agregar campos:
  ```rust
  pub web_server_enabled: bool,     // default: false
  pub web_server_port: u16,         // default: 9876
  pub web_server_bind: String,      // default: "127.0.0.1" (local only por seguridad)
  ```

- `src/shared/types.ts` — Agregar campos TS matching

- `src-tauri/src/lib.rs` — Leer settings en setup(), arrancar server solo si `web_server_enabled`

- `src/sidebar/components/SettingsModal.tsx` — Seccion "Remote Access":
  - Toggle enable/disable
  - Input puerto
  - Dropdown bind address (Local only / All interfaces)
  - Display URL remota + boton copiar
  - Warning: "Requiere reinicio. Solo habilitar en redes confiadas."

**Nuevo comando Tauri:**
- `get_remote_url() → Option<String>` — retorna URL completa con web token si el server esta activo

---

### Paso 7: Mobile CSS (polish)

**Archivos a modificar:**
- `src/sidebar/styles/*.css` — Media queries `@media (max-width: 600px)`:
  - Sidebar full width
  - Botones 44px minimo (touch targets Apple HIG)
  - Ocultar botones irrelevantes en browser (detach, explorer)
- `src/terminal/styles/*.css` — Titlebar 44px, font-size responsive
- `src/terminal/components/TerminalView.tsx` — Font size 12px en mobile (`window.innerWidth < 600`)

---

## Archivos criticos (referencia)

| Archivo | Rol |
|---|---|
| `src-tauri/src/lib.rs` | Setup Tauri + spawn web server + state wiring |
| `src-tauri/src/pty/manager.rs` | Read loop + broadcast + vt100 screen state |
| `src-tauri/src/commands/session.rs` | Reemplazar app.emit() por broadcast_all() |
| `src/shared/ipc.ts` | Punto unico de IPC a refactorizar |
| `src/shared/types.ts` | Tipos TS compartidos |
| `src-tauri/src/config/settings.rs` | Config donde agregar campos web server |

## Verificacion

**Setup previo:** Habilitar web server en settings, reiniciar app. Confirmar en consola:
```
[web-server] Listening on http://127.0.0.1:9876
[web-token] <uuid>
```

1. **Tauri local sigue funcionando** — `npm run tauri dev`, verificar sidebar + terminal + PTY flow sin regresiones
2. **Browser sidebar** — Abrir `http://localhost:9876/?window=sidebar&remoteToken=<web-token>`, verificar lista de sesiones, crear sesion
3. **Browser terminal** — Abrir `http://localhost:9876/?window=terminal&remoteToken=<web-token>`, verificar xterm.js renderiza, typing funciona, output llega
4. **Screen replay** — Conectar browser a sesion que ya tiene output, verificar que se muestra el contenido existente (no pantalla en blanco)
5. **Cross-sync** — Crear sesion desde browser, verificar que aparece en Tauri sidebar y viceversa
6. **Reconnect** — Cerrar y reabrir tab del browser, verificar reconexion automatica + screen replay
7. **Mobile** — Desde Chrome mobile en misma LAN (bind `0.0.0.0`), verificar layout responsive y touch input
