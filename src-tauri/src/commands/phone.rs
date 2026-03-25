use std::sync::Arc;
use tauri::State;

use crate::config::dark_factory;
use crate::config::settings::SettingsState;
use crate::phone::agent_registry::AgentRegistryState;
use crate::phone::manager;
use crate::phone::types::{AgentEntry, AgentInfo, InboxMessage, PhoneMessage};
use crate::session::manager::SessionManager;

#[tauri::command]
pub async fn phone_send_message(
    from: String,
    to: String,
    body: String,
    team: String,
) -> Result<String, String> {
    let config = dark_factory::load_dark_factory();
    manager::send_message(&from, &to, &body, &team, &config)
}

#[tauri::command]
pub async fn phone_get_inbox(agent_name: String) -> Result<Vec<PhoneMessage>, String> {
    manager::get_inbox(&agent_name)
}

#[tauri::command]
pub async fn phone_list_agents() -> Result<Vec<AgentInfo>, String> {
    let config = dark_factory::load_dark_factory();
    Ok(manager::list_agents(&config))
}

#[tauri::command]
pub async fn phone_ack_messages(
    agent_name: String,
    message_ids: Vec<String>,
) -> Result<(), String> {
    manager::ack_messages(&agent_name, &message_ids)
}

/// List all known agents from the registry (detected repos + session state).
#[tauri::command]
pub async fn phone_list_active_agents(
    registry: State<'_, AgentRegistryState>,
) -> Result<Vec<AgentEntry>, String> {
    let manifest = registry.snapshot().await;
    Ok(manifest.agents)
}

/// Rebuild the agent registry (e.g. after settings change).
#[tauri::command]
pub async fn phone_refresh_registry(
    registry: State<'_, AgentRegistryState>,
    settings: State<'_, SettingsState>,
) -> Result<(), String> {
    let cfg = settings.read().await;
    registry.rebuild(&cfg).await;
    Ok(())
}

/// Scan inbox directories of all active sessions for unread messages.
#[tauri::command]
pub async fn phone_scan_inboxes(
    session_mgr: State<'_, Arc<tokio::sync::RwLock<SessionManager>>>,
) -> Result<Vec<InboxMessage>, String> {
    // Collect working directories, then drop lock before filesystem I/O
    let working_dirs: Vec<String> = {
        let mgr = session_mgr.read().await;
        mgr.list_sessions().await.into_iter().map(|s| s.working_directory).collect()
    };

    let mut all_messages = Vec::new();

    for dir in &working_dirs {
        let inbox_dir = std::path::Path::new(dir)
            .join(".agentscommander")
            .join("inbox");

        if !inbox_dir.is_dir() {
            continue;
        }

        let entries = match std::fs::read_dir(&inbox_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(msg) = serde_json::from_str::<InboxMessage>(&data) {
                    if msg.status == "unread" {
                        all_messages.push(msg);
                    }
                }
            }
        }
    }

    all_messages.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    Ok(all_messages)
}

/// Mark a specific inbox message as read.
/// Uses raw JSON patching to preserve unknown fields agents may add.
#[tauri::command]
pub async fn phone_ack_inbox_message(
    session_mgr: State<'_, Arc<tokio::sync::RwLock<SessionManager>>>,
    session_id: String,
    msg_id: String,
) -> Result<(), String> {
    // Collect working directory, then drop lock before filesystem I/O
    let working_dir = {
        let mgr = session_mgr.read().await;
        let sessions = mgr.list_sessions().await;
        sessions
            .iter()
            .find(|s| s.id == session_id)
            .map(|s| s.working_directory.clone())
            .ok_or_else(|| format!("Session {} not found", session_id))?
    };

    let inbox_dir = std::path::Path::new(&working_dir)
        .join(".agentscommander")
        .join("inbox");

    if !inbox_dir.is_dir() {
        return Ok(());
    }

    let entries = std::fs::read_dir(&inbox_dir)
        .map_err(|e| format!("Cannot read inbox dir: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(data) = std::fs::read_to_string(&path) {
            // Use raw JSON value to preserve unknown fields
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&data) {
                if value.get("id").and_then(|v| v.as_str()) == Some(&msg_id) {
                    value["status"] = serde_json::json!("read");
                    let updated = serde_json::to_string_pretty(&value)
                        .map_err(|e| format!("Serialize error: {}", e))?;
                    std::fs::write(&path, updated)
                        .map_err(|e| format!("Write error: {}", e))?;
                    return Ok(());
                }
            }
        }
    }

    Ok(())
}
