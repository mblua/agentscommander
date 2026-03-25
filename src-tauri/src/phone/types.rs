use serde::{Deserialize, Serialize};

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

// --- Agent Registry types ---

/// One entry in agents.json — represents a known repo/agent
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEntry {
    /// Extended name: "parent/repo"
    pub name: String,
    /// Absolute path to repo root
    pub path: String,
    /// Teams from teams.json (empty if not in teams.json)
    pub teams: Vec<String>,
    /// UUID of active session, null if no session running
    #[serde(default)]
    pub session_id: Option<String>,
    /// ISO 8601 timestamp of last registry update for this entry
    pub updated_at: String,
}

/// The agents.json file format
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentsManifest {
    pub updated_at: String,
    pub agents: Vec<AgentEntry>,
}

/// Message written by an agent in its outbox/
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxMessage {
    pub to: String,
    pub body: String,
    #[serde(default = "default_priority")]
    pub priority: String,
    pub timestamp: String,
    #[serde(default)]
    pub bypass_team_check: bool,
}

fn default_priority() -> String {
    "normal".to_string()
}

/// Message written by agentscommander into an agent's inbox/
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxMessage {
    pub id: String,
    pub from: String,
    pub to: String,
    pub body: String,
    pub priority: String,
    pub timestamp: String,
    /// "unread" | "read"
    pub status: String,
}
