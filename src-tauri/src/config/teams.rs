use std::path::{Path, PathBuf};

/// A team discovered from `_team_*/config.json` in `.ac-new/` project directories.
#[derive(Debug, Clone)]
pub struct DiscoveredTeam {
    pub name: String,
    /// Agent display names in "project/agent" format (from resolve_agent_ref)
    pub agent_names: Vec<String>,
    /// Absolute paths to agent directories (resolved from team config refs)
    pub agent_paths: Vec<PathBuf>,
    /// Coordinator display name
    pub coordinator_name: Option<String>,
    /// Absolute path to coordinator directory
    pub coordinator_path: Option<PathBuf>,
}

/// Derive agent name (parent/folder) from a path, stripping `__agent_`/`_agent_` prefixes.
fn agent_name_from_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let components: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    if components.len() >= 2 {
        let parent = components[components.len() - 2];
        let last = components[components.len() - 1];
        let stripped = last
            .strip_prefix("__agent_")
            .or_else(|| last.strip_prefix("_agent_"))
            .unwrap_or(last);
        format!("{}/{}", parent, stripped)
    } else {
        normalized
    }
}

/// Resolve an agent ref (from team config) to a display name.
/// Handles relative refs like `_agent_foo` and absolute paths.
fn resolve_agent_ref(project_folder: &str, agent_ref: &str) -> String {
    let normalized = agent_ref.replace('\\', "/");
    let trimmed = normalized
        .trim_start_matches("../")
        .trim_start_matches("./");
    let agent_name = trimmed
        .split('/')
        .last()
        .unwrap_or(trimmed)
        .strip_prefix("_agent_")
        .unwrap_or(trimmed);
    format!("{}/{}", project_folder, agent_name)
}

/// Resolve an agent ref to an absolute path given the .ac-new directory.
fn resolve_agent_path(ac_new_dir: &Path, agent_ref: &str) -> Option<PathBuf> {
    let normalized = agent_ref.replace('\\', "/");
    let trimmed = normalized
        .trim_start_matches("../")
        .trim_start_matches("./");

    // Check if it's an absolute path
    if trimmed.contains(':') || trimmed.starts_with('/') {
        let p = PathBuf::from(trimmed);
        if p.is_dir() {
            return Some(p);
        }
        return None;
    }

    // Relative to .ac-new/
    let candidate = ac_new_dir.join(trimmed);
    if candidate.is_dir() {
        return Some(candidate);
    }

    // Try parent of .ac-new/ (project root)
    if let Some(project_root) = ac_new_dir.parent() {
        let candidate = project_root.join(trimmed);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }

    None
}

/// Check if an agent name matches a team member (by display name or path-derived name).
fn agent_matches_member(
    agent_name: &str,
    member_display_name: &str,
    member_path: Option<&PathBuf>,
) -> bool {
    if agent_name == member_display_name {
        return true;
    }
    if let Some(path) = member_path {
        let path_name = agent_name_from_path(&path.to_string_lossy());
        if agent_name == path_name {
            return true;
        }
    }
    false
}

/// Check if an agent is a member of a team.
fn is_team_member(agent_name: &str, team: &DiscoveredTeam) -> bool {
    for (i, display_name) in team.agent_names.iter().enumerate() {
        let path = team.agent_paths.get(i);
        if agent_matches_member(agent_name, display_name, path) {
            return true;
        }
    }
    false
}

/// Check if an agent is a coordinator of a team.
fn is_coordinator(agent_name: &str, team: &DiscoveredTeam) -> bool {
    if let Some(ref coord_name) = team.coordinator_name {
        if agent_matches_member(agent_name, coord_name, team.coordinator_path.as_ref()) {
            return true;
        }
    }
    false
}

/// Check if two agents can communicate based on discovery-based team routing rules.
///
/// Rules:
/// 1. Same team membership → allowed
/// 2. Both are coordinators (of any team) → allowed (cross-team coordinator chat)
/// 3. WG-scoped: agents in the same workgroup → allowed
/// 4. Otherwise → denied
pub fn can_communicate(from: &str, to: &str, teams: &[DiscoveredTeam]) -> bool {
    // Rule 1: Same team membership
    for team in teams {
        if is_team_member(from, team) && is_team_member(to, team) {
            return true;
        }
    }

    // Rule 2: WG-scoped (agents in the same workgroup can communicate)
    if from.starts_with("wg-") && to.starts_with("wg-") {
        let from_wg = from.split('/').next().unwrap_or("");
        let to_wg = to.split('/').next().unwrap_or("");
        if !from_wg.is_empty() && from_wg == to_wg {
            return true;
        }
    }

    // Rule 3: Coordinator-to-coordinator (any teams)
    let from_is_coordinator = teams.iter().any(|t| is_coordinator(from, t));
    let to_is_coordinator = teams.iter().any(|t| is_coordinator(to, t));
    if from_is_coordinator && to_is_coordinator {
        return true;
    }

    false
}

/// Discover all teams from all known project paths.
/// Scans settings.repo_paths (and immediate children) for `.ac-new/_team_*/config.json`.
pub fn discover_teams() -> Vec<DiscoveredTeam> {
    let settings = crate::config::settings::load_settings();
    let mut teams = Vec::new();

    for repo_path in &settings.repo_paths {
        let base = Path::new(repo_path);
        if !base.is_dir() {
            continue;
        }

        // Check base and immediate children (same pattern as ac_discovery)
        let mut dirs_to_check = vec![base.to_path_buf()];
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !name.starts_with('.') {
                        dirs_to_check.push(p);
                    }
                }
            }
        }

        for project_dir in dirs_to_check {
            discover_teams_in_project(&project_dir, &mut teams);
        }
    }

    teams
}

/// Discover teams in a single project directory.
fn discover_teams_in_project(project_dir: &Path, teams: &mut Vec<DiscoveredTeam>) {
    let ac_new = project_dir.join(".ac-new");
    if !ac_new.is_dir() {
        return;
    }

    let project_folder = project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let entries = match std::fs::read_dir(&ac_new) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let team_dir = entry.path();
        if !team_dir.is_dir() {
            continue;
        }

        let dir_name = match team_dir.file_name().and_then(|n| n.to_str()) {
            Some(n) if n.starts_with("_team_") => n,
            _ => continue,
        };

        let team_name = dir_name
            .strip_prefix("_team_")
            .unwrap_or(dir_name)
            .to_string();

        let config_path = team_dir.join("config.json");
        let parsed: serde_json::Value = match std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
        {
            Some(v) => v,
            None => continue,
        };

        // Resolve agents
        let agent_refs: Vec<String> = parsed
            .get("agents")
            .and_then(|a| a.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let agent_names: Vec<String> = agent_refs
            .iter()
            .map(|r| resolve_agent_ref(&project_folder, r))
            .collect();

        let agent_paths: Vec<PathBuf> = agent_refs
            .iter()
            .filter_map(|r| resolve_agent_path(&ac_new, r))
            .collect();

        // Resolve coordinator
        let coordinator_ref = parsed
            .get("coordinator")
            .and_then(|c| c.as_str())
            .map(String::from);

        let coordinator_name = coordinator_ref
            .as_ref()
            .map(|r| resolve_agent_ref(&project_folder, r));

        let coordinator_path = coordinator_ref
            .as_ref()
            .and_then(|r| resolve_agent_path(&ac_new, r));

        teams.push(DiscoveredTeam {
            name: team_name,
            agent_names,
            agent_paths,
            coordinator_name,
            coordinator_path,
        });
    }
}

/// Find all team member paths from discovered teams (for repo path resolution).
pub fn all_member_paths(teams: &[DiscoveredTeam]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for team in teams {
        for p in &team.agent_paths {
            if !paths.contains(p) {
                paths.push(p.clone());
            }
        }
    }
    paths
}
