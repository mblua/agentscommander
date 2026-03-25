use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::commands::repos::derive_repo_name;
use crate::config::dark_factory;
use crate::config::settings::AppSettings;
use super::types::{AgentEntry, AgentsManifest};

pub type AgentRegistryState = Arc<AgentRegistry>;

pub struct AgentRegistry {
    manifest: RwLock<AgentsManifest>,
}

impl AgentRegistry {
    /// Build the initial manifest by scanning repo_paths from settings.
    /// Also loads teams from DarkFactoryConfig to populate AgentEntry.teams.
    pub fn build_from_settings(settings: &AppSettings) -> Arc<Self> {
        let agents = scan_repos_for_agents(settings);
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let manifest = AgentsManifest {
            updated_at: now,
            agents,
        };

        // Flush before wrapping in RwLock — no lock needed
        let _ = flush_to_disk(&manifest);

        Arc::new(Self {
            manifest: RwLock::new(manifest),
        })
    }

    /// Called when a session is created. Sets session_id on the matching agent entry.
    pub async fn register_session(&self, repo_path: &str, session_id: uuid::Uuid) {
        let normalized = normalize_path(repo_path);
        let snapshot = {
            let mut manifest = self.manifest.write().await;
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

            if let Some(entry) = manifest.agents.iter_mut().find(|a| normalize_path(&a.path) == normalized) {
                entry.session_id = Some(session_id.to_string());
                entry.updated_at = now.clone();
            } else {
                // Agent not in registry (e.g. manual session with custom cwd).
                // Add it dynamically.
                let name = derive_repo_name(Path::new(repo_path))
                    .unwrap_or_else(|| repo_path.to_string());
                manifest.agents.push(AgentEntry {
                    name,
                    path: repo_path.to_string(),
                    teams: Vec::new(),
                    session_id: Some(session_id.to_string()),
                    updated_at: now.clone(),
                });
            }

            manifest.updated_at = now;
            manifest.clone()
        }; // write lock dropped here

        let _ = flush_to_disk(&snapshot);
    }

    /// Called when a session is destroyed. Clears session_id on the matching entry.
    pub async fn unregister_session(&self, session_id: uuid::Uuid) {
        let sid = session_id.to_string();
        let snapshot = {
            let mut manifest = self.manifest.write().await;
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

            if let Some(entry) = manifest.agents.iter_mut().find(|a| a.session_id.as_deref() == Some(&sid)) {
                entry.session_id = None;
                entry.updated_at = now.clone();
            }

            manifest.updated_at = now;
            manifest.clone()
        }; // write lock dropped here

        let _ = flush_to_disk(&snapshot);
    }

    /// Called when settings change (repo_paths modified). Rebuilds the manifest
    /// preserving active session_id assignments.
    pub async fn rebuild(&self, settings: &AppSettings) {
        let snapshot = {
            let mut manifest = self.manifest.write().await;
            let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

            // Preserve session_id assignments by path
            let active_sessions: std::collections::HashMap<String, String> = manifest
                .agents
                .iter()
                .filter_map(|a| {
                    a.session_id.as_ref().map(|sid| (normalize_path(&a.path), sid.clone()))
                })
                .collect();

            let mut agents = scan_repos_for_agents(settings);

            // Re-attach session_ids
            for agent in &mut agents {
                let norm = normalize_path(&agent.path);
                if let Some(sid) = active_sessions.get(&norm) {
                    agent.session_id = Some(sid.clone());
                }
            }

            manifest.agents = agents;
            manifest.updated_at = now;
            manifest.clone()
        }; // write lock dropped here

        let _ = flush_to_disk(&snapshot);
    }

    /// Returns the absolute path for a named agent.
    pub async fn resolve_path(&self, agent_name: &str) -> Option<String> {
        let manifest = self.manifest.read().await;
        manifest.agents.iter()
            .find(|a| a.name == agent_name)
            .map(|a| a.path.clone())
    }

    /// Reverse lookup: find agent name by absolute path.
    pub async fn resolve_name_for_path(&self, abs_path: &str) -> Option<String> {
        let normalized = normalize_path(abs_path);
        let manifest = self.manifest.read().await;
        manifest.agents.iter()
            .find(|a| normalize_path(&a.path) == normalized)
            .map(|a| a.name.clone())
    }

    /// Returns a snapshot of the full manifest.
    pub async fn snapshot(&self) -> AgentsManifest {
        self.manifest.read().await.clone()
    }
}

/// Scan repo_paths from settings to build the agent list.
/// Uses the same logic as search_repos but without query filtering.
fn scan_repos_for_agents(settings: &AppSettings) -> Vec<AgentEntry> {
    let df_config = dark_factory::load_dark_factory();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut agents = Vec::new();
    let mut seen_paths = std::collections::HashSet::new();

    for base_path in &settings.repo_paths {
        let base = Path::new(base_path);
        if !base.is_dir() {
            continue;
        }

        if base.join(".git").is_dir() {
            try_add_agent(base, &df_config, &now, &mut seen_paths, &mut agents);
            continue;
        }

        let entries = match std::fs::read_dir(base) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if name.starts_with('.') || name.to_uppercase().starts_with("DEPRECATED") {
                continue;
            }
            try_add_agent(&path, &df_config, &now, &mut seen_paths, &mut agents);
        }
    }

    agents.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    agents
}

fn try_add_agent(
    path: &Path,
    df_config: &dark_factory::DarkFactoryConfig,
    now: &str,
    seen_paths: &mut std::collections::HashSet<String>,
    agents: &mut Vec<AgentEntry>,
) {
    let path_str = path.to_string_lossy().to_string();
    if !seen_paths.insert(path_str.clone()) {
        return;
    }

    let name = match derive_repo_name(path) {
        Some(n) => n,
        None => return,
    };

    // Find team memberships for this path from DarkFactory config
    let teams: Vec<String> = df_config
        .teams
        .iter()
        .filter(|t| t.members.iter().any(|m| normalize_path(&m.path) == normalize_path(&path_str)))
        .map(|t| t.name.clone())
        .collect();

    agents.push(AgentEntry {
        name,
        path: path_str,
        teams,
        session_id: None,
        updated_at: now.to_string(),
    });
}

/// Write manifest to global config dir and to each known agent's .agentscommander/ dir.
fn flush_to_disk(manifest: &AgentsManifest) -> Result<(), String> {
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("Failed to serialize agents.json: {}", e))?;

    // Write global copy
    if let Some(dir) = crate::config::config_dir() {
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = std::fs::write(dir.join("agents.json"), &json) {
            log::warn!("Failed to write global agents.json: {}", e);
        }
    }

    // Write per-repo copy
    for agent in &manifest.agents {
        let agent_dir = Path::new(&agent.path).join(".agentscommander");
        if let Err(e) = std::fs::create_dir_all(&agent_dir) {
            log::warn!("Cannot create .agentscommander dir at {:?}: {}", agent_dir, e);
            continue;
        }
        if let Err(e) = std::fs::write(agent_dir.join("agents.json"), &json) {
            log::warn!("Cannot write agents.json at {:?}: {}", agent_dir, e);
        }
    }

    Ok(())
}

/// Normalize a path for comparison: lowercase on Windows, forward slashes.
fn normalize_path(p: &str) -> String {
    let s = p.replace('\\', "/");
    if cfg!(target_os = "windows") {
        s.to_lowercase()
    } else {
        s
    }
}
