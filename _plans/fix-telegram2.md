# Plan: fix/telegram2 - Telegram Bridge Output Filtering

## Estado: VALIDADO contra codebase

---

## Problemas Observados

Comparando terminal vs Telegram, hay 4 categorias de ruido que pasan los filtros:

### Problema 1: `⎿ Running...` progress lines
Las lineas de progreso de ejecucion de tools de Claude Code se envian a Telegram.
```
⎿  Running…
⎿  Running… (3s)
⎿  Running… (4s)
⎿  Running… (7s)
```
**Causa**: No hay filtro para el patron `⎿` + `Running` con timer. Estas lineas cambian lentamente (cada ~3-5 segundos), asi que estabilizan a 800ms.

**Codepoint VALIDADO**: `⎿` = U+23BF (BOTTOM RIGHT CORNER), UTF-8: `E2 8E BF`

### Problema 2: Tool headers internos del TUI
```
●─Bash(rtk git pull)
```
**Causa**: El `●─` seguido de nombre de tool no esta filtrado. Es chrome del TUI de Claude Code.

**Codepoints VALIDADOS**: `●` = U+25CF (BLACK CIRCLE, `E2 97 8F`), `─` = U+2500 (BOX DRAWINGS LIGHT HORIZONTAL, `E2 94 80`)

**NOTA**: `●` (U+25CF) ya esta en `CLAUDE_SPINNERS` (bridge.rs:285). La secuencia `●─` es especifica de tool headers.

### Problema 3: Spinner text que estabiliza
```
· Topsy-turvying…
· Ttokens.rvying…
✶ Gallivanting…
```
**Causa**: `is_thinking_line()` (bridge.rs:378-410) requiere que la palabra sea `all(|c| c.is_alphabetic())` (linea 403). "Topsy-turvying" tiene guion (`-`), "Ttokens.rvying" tiene punto (`.`), asi que fallan el check.

**Codepoints VALIDADOS**: `·` = U+00B7 (MIDDLE DOT, `C2 B7`), `✶` = U+2736 (SIX POINTED BLACK STAR, `E2 9C B6`), `…` = U+2026 (HORIZONTAL ELLIPSIS, `E2 80 A6`)

### Problema 4: Spinner text concatenado al final de filas anchas
El screen vt100 es 220 columnas. Claude Code coloca spinners en la esquina derecha:
```
ok (up-to-date)                                          ✶ Gallivanting…
⎿  [rtk] /!\...                                          ✶ Gallivanting…
```
**Causa**: `is_thinking_line()` solo evalua si la linea ENTERA es spinner. Cuando el spinner esta al final de una fila con contenido real, no lo detecta. `strip_trailing_decoration` (bridge.rs:231-238) solo quita box-drawing chars, no spinners.

---

## Solucion Propuesta

### Cambio 1: Strip trailing spinners de filas anchas (Problema 4 - ROOT CAUSE)

**Archivo**: `bridge.rs` - nueva funcion `strip_trailing_spinner()`, definir despues de `strip_trailing_decoration()` (despues de linea 238)

Despues de `strip_trailing_decoration()`, aplicar una segunda pasada que busque patron de spinner al final de la fila:
- Detectar: `{spaces}{spinner_char} {texto}{…}` al final de una linea
- Spinner chars: los 7 de `CLAUDE_SPINNERS` (bridge.rs:285): ✻ ✶ * ✢ · ● ✽
- Remover ese sufijo y devolver solo el contenido real

**Puntos de insercion VALIDADOS**:
1. `update_from_screen()` linea 168 - despues de `strip_trailing_decoration`:
   ```rust
   let cleaned = strip_trailing_decoration(&row_text);
   let cleaned = strip_trailing_spinner(&cleaned);  // INSERT
   ```
2. USER_INPUT capture block linea 560 - misma posicion, para consistencia:
   ```rust
   let cleaned = strip_trailing_decoration(&row_text);
   let cleaned = strip_trailing_spinner(&cleaned);  // INSERT
   ```

**Beneficio colateral**: Al limpiar spinners ANTES del tracking, las filas estabilizan mas rapido porque solo cambia el contenido real.

### Cambio 2: Filtrar progress indicators y tool headers (Problemas 1 y 2)

**Archivo**: `bridge.rs` - `ClaudeCodeFilter::keep_line()` (linea 289+)

Agregar filtros SELECTIVOS (no filtrar todo `⎿`, porque tambien contiene output real de tools):
```rust
// Tool execution progress: ⎿  Running…, ⎿  Running… (3s)
if trimmed.starts_with("\u{23BF}") && trimmed.contains("Running") {
    return false;
}

// Tool headers: ●─Bash(...), ●─Read(...), etc.
if trimmed.starts_with("\u{25CF}\u{2500}") {  // ●─
    return false;
}
```

**IMPORTANTE** (correccion vs plan original): NO filtrar todas las lineas `⎿` - solo las que contienen "Running". Lineas como `⎿  [rtk] /!\ No hook installed...` son output real de tools que el usuario quiere ver (o que se filtraran por otros medios si son ruido).

### Cambio 3: Relajar `is_thinking_line()` (Problema 3)

**Archivo**: `bridge.rs` - `is_thinking_line()` (linea 378)

La validacion actual (linea 403) es demasiado estricta:
```rust
// ACTUAL: solo alpha pura
word_part.chars().all(|c| c.is_alphabetic())
```

**Solucion recomendada**: Si la linea empieza con un spinner char y termina con `…` o `...`, filtrarla sin importar el contenido intermedio. Los spinners de Claude Code SIEMPRE tienen esta forma: `{spinner} {texto}{…}`.

```rust
// REEMPLAZO: cualquier linea corta que empiece con spinner y termine con ellipsis
if check.ends_with('\u{2026}') || check.ends_with("...") {
    // Lines starting with spinner char + ending with ellipsis are always
    // Claude Code thinking/processing animations
    let word_part = check.trim_end_matches('\u{2026}').trim_end_matches("...");
    if !word_part.is_empty() && word_part.len() < 50 {
        return true;
    }
}
```

**Justificacion**: Claude Code no produce contenido real en formato `{spinner} {palabra}…`. El limite de 50 chars previene false positives con lineas de contenido real que accidentalmente terminen en `…`.

### Cambio 4: Limpiar USER_INPUT capture (mejora menor)

**Archivo**: `bridge.rs` - bloque USER_INPUT (linea 551+)

El texto capturado puede incluir espacios intermedios del screen de 220 cols. Aplicar trim al user_input extraido:
```rust
let user_input = user_input.trim();
```

---

## Orden de Implementacion

1. **Cambio 1** (strip trailing spinners) - Root cause de Problema 4, mejora Problema 3
2. **Cambio 2** (filtrar progress `⎿` + tool headers `●─`) - Limpia las lineas de Running
3. **Cambio 3** (relajar is_thinking_line) - Catch-all para spinners restantes
4. **Cambio 4** (limpiar USER_INPUT) - Mejora menor
5. **cargo check** - Verificar compilacion
6. **Test manual** - Attach Telegram, tipear en terminal, verificar que solo llegan respuestas del modelo

## Criterio de Exito

En Telegram solo deben aparecer:
- Input del usuario (prefijado con `❯`) cuando tipea desde la terminal
- Input del usuario (sin prefijo) cuando llega desde Telegram
- Respuestas del modelo (lineas que empiezan con `●`)
- Output de tools reales (contenido de archivos, resultados de comandos) SIN chrome del TUI

NO deben aparecer:
- `⎿ Running…` ni variantes con timer
- `●─Bash(...)` tool headers
- Spinner text: `· Topsy-turvying…`, `✶ Gallivanting…`, etc.
- Spinner concatenado a contenido real
