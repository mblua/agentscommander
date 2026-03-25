use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::config::dark_factory;
use super::agent_registry::AgentRegistry;
use super::manager::can_communicate;
use super::types::{InboxMessage, OutboxMessage};

const POLL_INTERVAL: Duration = Duration::from_secs(3);

pub struct MailboxPoller {
    registry: Arc<AgentRegistry>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InboxEventPayload {
    recipient_name: String,
    recipient_path: String,
    message: InboxMessage,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UndeliverablePayload {
    from: String,
    to: String,
    reason: String,
    original_file: String,
}

impl MailboxPoller {
    pub fn new(registry: Arc<AgentRegistry>) -> Arc<Self> {
        Arc::new(Self { registry })
    }

    pub fn start(self: &Arc<Self>, app_handle: AppHandle) {
        let poller = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(POLL_INTERVAL).await;
                poller.poll(&app_handle).await;
            }
        });
    }

    async fn poll(&self, app: &AppHandle) {
        let manifest = self.registry.snapshot().await;
        let df_config = dark_factory::load_dark_factory();

        for agent in &manifest.agents {
            let outbox_dir = Path::new(&agent.path)
                .join(".agentscommander")
                .join("outbox");

            if !outbox_dir.is_dir() {
                continue;
            }

            let entries = match std::fs::read_dir(&outbox_dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let file_path = entry.path();
                if file_path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }

                self.process_outbox_file(
                    &file_path,
                    &agent.name,
                    &agent.path,
                    &df_config,
                    app,
                )
                .await;
            }
        }
    }

    async fn process_outbox_file(
        &self,
        file_path: &Path,
        from_name: &str,
        _from_path: &str,
        df_config: &dark_factory::DarkFactoryConfig,
        app: &AppHandle,
    ) {
        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Parse the outbox message
        let data = match std::fs::read_to_string(file_path) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("Failed to read outbox file {:?}: {}", file_path, e);
                return;
            }
        };

        let outbox_msg: OutboxMessage = match serde_json::from_str(&data) {
            Ok(m) => m,
            Err(e) => {
                log::warn!("Failed to parse outbox file {:?}: {}", file_path, e);
                reject_file(file_path, &format!("parse_error: {}", e));
                return;
            }
        };

        // Resolve destination
        let to_path = match self.registry.resolve_path(&outbox_msg.to).await {
            Some(p) => p,
            None => {
                log::warn!(
                    "Undeliverable message from '{}' to '{}': unknown destination",
                    from_name, outbox_msg.to
                );
                let _ = app.emit(
                    "agent_message_undeliverable",
                    UndeliverablePayload {
                        from: from_name.to_string(),
                        to: outbox_msg.to.clone(),
                        reason: "unknown_destination".to_string(),
                        original_file: file_name.clone(),
                    },
                );
                reject_file(file_path, "unknown_destination");
                return;
            }
        };

        // Validate routing
        if !outbox_msg.bypass_team_check
            && !can_communicate(from_name, &outbox_msg.to, df_config)
        {
            log::warn!(
                "Blocked message from '{}' to '{}': team routing denied",
                from_name, outbox_msg.to
            );
            let _ = app.emit(
                "agent_message_undeliverable",
                UndeliverablePayload {
                    from: from_name.to_string(),
                    to: outbox_msg.to.clone(),
                    reason: "team_routing_blocked".to_string(),
                    original_file: file_name.clone(),
                },
            );
            reject_file(file_path, "team_routing_blocked");
            return;
        }

        // Build inbox message
        let msg_id = uuid::Uuid::new_v4().to_string();
        let inbox_msg = InboxMessage {
            id: msg_id.clone(),
            from: from_name.to_string(),
            to: outbox_msg.to.clone(),
            body: outbox_msg.body,
            priority: outbox_msg.priority,
            timestamp: outbox_msg.timestamp,
            status: "unread".to_string(),
        };

        // Write to target's inbox
        let inbox_dir = Path::new(&to_path)
            .join(".agentscommander")
            .join("inbox");

        if let Err(e) = std::fs::create_dir_all(&inbox_dir) {
            log::warn!("Cannot create inbox dir at {:?}: {}", inbox_dir, e);
            return;
        }

        let from_safe = from_name.replace('/', "_").replace('\\', "_");
        let inbox_filename = format!("{}-from-{}.json", msg_id, from_safe);
        let inbox_path = inbox_dir.join(&inbox_filename);

        let inbox_json = match serde_json::to_string_pretty(&inbox_msg) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Failed to serialize inbox message: {}", e);
                return;
            }
        };

        if let Err(e) = std::fs::write(&inbox_path, &inbox_json) {
            log::warn!("Failed to write inbox message to {:?}: {}", inbox_path, e);
            return;
        }

        // Delete the source outbox file
        if let Err(e) = std::fs::remove_file(file_path) {
            log::warn!("Failed to remove processed outbox file {:?}: {}", file_path, e);
        }

        // Emit event to frontend
        let _ = app.emit(
            "agent_inbox_message",
            InboxEventPayload {
                recipient_name: outbox_msg.to,
                recipient_path: to_path,
                message: inbox_msg,
            },
        );

        log::info!(
            "Delivered message from '{}' to inbox at {:?}",
            from_name, inbox_path
        );
    }
}

/// Move a file to outbox/rejected/ with a reason sidecar.
fn reject_file(file_path: &Path, reason: &str) {
    let rejected_dir = match file_path.parent() {
        Some(p) => p.join("rejected"),
        None => return,
    };

    if std::fs::create_dir_all(&rejected_dir).is_err() {
        return;
    }

    if let Some(name) = file_path.file_name() {
        let dest = rejected_dir.join(name);
        let _ = std::fs::rename(file_path, &dest);

        // Write reason sidecar
        let reason_path = dest.with_extension("reason.txt");
        let _ = std::fs::write(
            reason_path,
            format!("Rejected at {}: {}", chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"), reason),
        );
    }
}
