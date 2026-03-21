use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionButton {
    pub id: String,
    pub label: String,
    pub command: String,
    pub args: Vec<String>,
    pub color: String,
    pub working_directory: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub default_shell: String,
    pub default_shell_args: Vec<String>,
    pub action_buttons: Vec<ActionButton>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_shell: "powershell.exe".to_string(),
            default_shell_args: vec!["-NoLogo".to_string()],
            action_buttons: vec![
                ActionButton {
                    id: "claude".to_string(),
                    label: "Claude".to_string(),
                    command: "claude".to_string(),
                    args: vec![],
                    color: "#d97706".to_string(),
                    working_directory: "~".to_string(),
                },
                ActionButton {
                    id: "codex".to_string(),
                    label: "Codex".to_string(),
                    command: "codex".to_string(),
                    args: vec![],
                    color: "#10b981".to_string(),
                    working_directory: "~".to_string(),
                },
            ],
        }
    }
}

pub type SettingsState = Arc<RwLock<AppSettings>>;
