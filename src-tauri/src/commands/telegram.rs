use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::config::settings::SettingsState;
use crate::pty::manager::PtyManager;
use crate::session::manager::SessionManager;
use crate::telegram::bridge::SessionReaderKind;
use crate::telegram::manager::TelegramBridgeState;
use crate::telegram::types::BridgeInfo;

/// Derive which session-reader pipeline to spawn for a given session.
///
/// - `Ok(Some(kind))` — agent detected and resolver succeeded → caller spawns the reader.
/// - `Ok(None)` — no agent detected (or Gemini in commit 3 — handled in commit 4)
///   → caller falls back to PTY mode.
/// - `Err(message)` — agent detected but resolver returned None → caller logs +
///   emits `telegram_bridge_error` + early-returns with its contractual success
///   value (or `Err` for `telegram_attach`).
///
/// **Mutual exclusion (H4) is asserted in debug builds.** The detection at
/// `commands/session.rs:692-694` is mutually exclusive (Claude > Codex > Gemini),
/// so the assertion is invariant.
pub(crate) fn derive_reader(
    shell: &str,
    shell_args: &[String],
    cwd: &str,
    is_claude: bool,
    is_codex: bool,
    is_gemini: bool,
) -> Result<Option<SessionReaderKind>, String> {
    let kinds_set: u8 = u8::from(is_claude) + u8::from(is_codex) + u8::from(is_gemini);
    debug_assert!(
        kinds_set <= 1,
        "agent-kind flags must be mutually exclusive — fix detection at commands/session.rs:692-694 (is_claude={}, is_codex={}, is_gemini={})",
        is_claude, is_codex, is_gemini
    );

    let attach_time = chrono::Utc::now();

    if is_claude {
        return match crate::commands::session::resolve_claude_projects_dir(shell, shell_args, cwd)
        {
            Some(p) => Ok(Some(SessionReaderKind::Claude { project_dir: p })),
            None => Err("Cannot resolve Claude projects dir".to_string()),
        };
    }
    if is_codex {
        return match crate::commands::codex_resolver::resolve_codex_sessions_root(
            shell, shell_args, cwd,
        ) {
            Some(root) => Ok(Some(SessionReaderKind::Codex {
                search_root: root,
                cwd: cwd.to_string(),
                attach_time,
            })),
            None => Err(
                "Cannot resolve Codex sessions root (~/.codex/sessions/ missing)".to_string(),
            ),
        };
    }
    if is_gemini {
        // Gemini reader lands in commit 4 (#258). Until then, Gemini sessions
        // fall back to PTY mode just like undetected agents.
        return Ok(None);
    }
    Ok(None) // No agent detected — caller falls back to PTY mode.
}

#[tauri::command]
pub async fn telegram_attach(
    app: AppHandle,
    tg_mgr: State<'_, TelegramBridgeState>,
    pty_mgr: State<'_, Arc<Mutex<PtyManager>>>,
    settings: State<'_, SettingsState>,
    session_id: String,
    bot_id: String,
) -> Result<BridgeInfo, String> {
    let uuid = Uuid::parse_str(&session_id).map_err(|e| e.to_string())?;

    // Extract the fields the resolver needs and drop the SessionManager read guard
    // BEFORE invoking `derive_reader` — the resolver does blocking filesystem I/O
    // (`which::which` walks `%PATH%`, opens wrapper scripts) that can take hundreds
    // of milliseconds. Holding a `tokio::sync::RwLock` read guard across that would
    // starve concurrent writers (create_session, restart_session, switch_session).
    let (is_claude, is_codex, is_gemini, shell, shell_args, working_directory) = {
        let session_mgr = app.state::<Arc<tokio::sync::RwLock<SessionManager>>>();
        let mgr = session_mgr.read().await;
        let session = mgr.get_session(uuid).await.ok_or("Session not found")?;
        (
            session.is_claude,
            session.is_codex,
            session.is_gemini,
            session.shell.clone(),
            session.shell_args.clone(),
            session.working_directory.clone(),
        )
    };

    let reader = match derive_reader(
        &shell,
        &shell_args,
        &working_directory,
        is_claude,
        is_codex,
        is_gemini,
    ) {
        Ok(r) => r,
        Err(reason) => {
            let err_msg = format!(
                "Telegram bridge: {} for session {} (shell={:?}). Bridge inactive.",
                reason, uuid, shell
            );
            log::error!("{}", err_msg);
            let _ = app.emit(
                "telegram_bridge_error",
                serde_json::json!({
                    "sessionId": session_id,
                    "error": err_msg,
                }),
            );
            return Err(err_msg);
        }
    };

    let cfg = settings.read().await;
    let bot = cfg
        .telegram_bots
        .iter()
        .find(|b| b.id == bot_id)
        .ok_or_else(|| format!("Bot not found: {}", bot_id))?
        .clone();
    drop(cfg);

    let pty_arc = pty_mgr.inner().clone();
    let mut tg = tg_mgr.lock().await;
    let info = tg
        .attach(uuid, &bot, pty_arc, app.clone(), reader)
        .map_err(|e| e.to_string())?;

    let _ = app.emit("telegram_bridge_attached", info.clone());

    Ok(info)
}

#[tauri::command]
pub async fn telegram_detach(
    app: AppHandle,
    tg_mgr: State<'_, TelegramBridgeState>,
    session_id: String,
) -> Result<(), String> {
    let uuid = Uuid::parse_str(&session_id).map_err(|e| e.to_string())?;

    let mut tg = tg_mgr.lock().await;
    tg.detach(uuid).map_err(|e| e.to_string())?;

    let _ = app.emit(
        "telegram_bridge_detached",
        serde_json::json!({ "sessionId": session_id }),
    );

    Ok(())
}

#[tauri::command]
pub async fn telegram_list_bridges(
    tg_mgr: State<'_, TelegramBridgeState>,
) -> Result<Vec<BridgeInfo>, String> {
    let tg = tg_mgr.lock().await;
    Ok(tg.list_bridges())
}

#[tauri::command]
pub async fn telegram_get_bridge(
    tg_mgr: State<'_, TelegramBridgeState>,
    session_id: String,
) -> Result<Option<BridgeInfo>, String> {
    let uuid = Uuid::parse_str(&session_id).map_err(|e| e.to_string())?;
    let tg = tg_mgr.lock().await;
    Ok(tg.get_bridge(uuid))
}

/// Test bot connection: discovers chat_id from the latest message sent to the bot,
/// sends a confirmation message back, and returns the discovered chat_id.
/// The user just needs to send any message to the bot before clicking Test.
#[tauri::command]
pub async fn telegram_send_test(token: String) -> Result<i64, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    // Fetch recent updates to discover chat_id
    let updates = crate::telegram::api::get_updates(&client, &token, 0, 0)
        .await
        .map_err(|e| e.to_string())?;

    let chat_id = updates
        .last()
        .map(|u| u.chat_id)
        .ok_or_else(|| "No messages found. Send any message to your bot in Telegram first, then click Test again.".to_string())?;

    crate::telegram::api::send_message(&client, &token, chat_id, "agentscommander connected")
        .await
        .map_err(|e| e.to_string())?;

    Ok(chat_id)
}
