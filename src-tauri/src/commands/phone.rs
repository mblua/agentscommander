use crate::phone::manager;
use crate::phone::types::{AgentInfo, PhoneMessage};

use crate::config::teams;

#[tauri::command]
pub async fn phone_get_inbox(agent_name: String) -> Result<Vec<PhoneMessage>, String> {
    manager::get_inbox(&agent_name)
}

#[tauri::command]
pub async fn phone_list_agents() -> Result<Vec<AgentInfo>, String> {
    let discovered = teams::discover_teams();
    Ok(manager::list_agents(&discovered))
}

#[tauri::command]
pub async fn phone_ack_messages(
    agent_name: String,
    message_ids: Vec<String>,
) -> Result<(), String> {
    manager::ack_messages(&agent_name, &message_ids)
}
