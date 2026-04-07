use serde::{Deserialize, Serialize};

/// Message format in outbox files. Shared between CLI (send, close-session) and MailboxPoller.
/// All new fields are Option/default for backwards compatibility with existing outbox messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxMessage {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    pub from: String,
    pub to: String,
    pub body: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub get_output: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_agent: Option<String>,
    #[serde(default)]
    pub preferred_agent: String,
    #[serde(default)]
    pub priority: String,
    pub timestamp: String,
    /// Remote command to execute on agent's PTY (e.g., "clear", "compact")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Action type for non-message operations (e.g., "close-session")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Target agent name for action-based operations (e.g., close-session target)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Force mode for close-session (true = immediate kill, false = graceful)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
    /// Timeout in seconds for graceful shutdown before fallback to force-kill
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneMessage {
    pub id: String,
    pub from: String,
    pub to: String,
    pub team: String,
    pub content: String,
    pub timestamp: String,
    /// "pending", "delivered", "error"
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub participants: Vec<String>,
    pub created_at: String,
    pub messages: Vec<PhoneMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub name: String,
    pub path: String,
    pub teams: Vec<String>,
    pub is_coordinator_of: Vec<String>,
}
