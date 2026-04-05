//! Build-profile constants determined at compile time.
//!
//! BUILD_PROFILE is set by build.rs from the BUILD_PROFILE env var.
//! Values: "dev" (debug builds), "prod" (default release), "stage" (explicit).

/// Build profile string — "dev", "prod", or "stage".
pub const BUILD_PROFILE: &str = env!("BUILD_PROFILE");

/// Config directory name under $HOME.
/// PROD and STAGE share `.agentscommander-new`; DEV uses a separate one.
/// The old app uses `.agentscommander` — never touch that.
pub fn config_dir_name() -> &'static str {
    match BUILD_PROFILE {
        "dev" => ".agentscommander-new-dev",
        _ => ".agentscommander-new", // prod and stage share config
    }
}

/// Application title for the sidebar window.
pub fn app_title() -> &'static str {
    match BUILD_PROFILE {
        "dev" => "Agents Commander New",
        "stage" => "Agents Commander [STAGE]",
        _ => "Agents Commander",
    }
}

/// Title suffix appended to secondary windows (guide, etc.).
pub fn app_title_suffix() -> &'static str {
    match BUILD_PROFILE {
        "dev" => "Agents Commander New",
        "stage" => "Agents Commander [STAGE]",
        _ => "Agents Commander",
    }
}

/// Windows single-instance mutex name. Each profile gets a unique mutex
/// so PROD and STAGE can run simultaneously.
pub fn mutex_name() -> &'static str {
    match BUILD_PROFILE {
        "dev" => "Local\\AgentsCommander_SingleInstance_New_Dev\0",
        "stage" => "Local\\AgentsCommander_SingleInstance_Stage\0",
        _ => "Local\\AgentsCommander_SingleInstance\0",
    }
}

/// Executable name for use in documentation and agent instructions.
pub fn exe_name() -> &'static str {
    match BUILD_PROFILE {
        "dev" => "agentscommander-new.exe",
        "stage" => "agentscommander-stage.exe",
        _ => "agentscommander.exe",
    }
}

/// Product name as installed in LOCALAPPDATA (matches Tauri productName).
pub fn product_name() -> &'static str {
    match BUILD_PROFILE {
        "dev" => "Agents Commander New",
        "stage" => "Agents Commander Stage",
        _ => "Agents Commander",
    }
}

/// Default web server port. Each profile gets a distinct port
/// so PROD, STAGE, and the old app can run simultaneously.
/// Old app uses 9876 — never collide with that.
pub fn web_server_port() -> u16 {
    match BUILD_PROFILE {
        "dev" => 9876,
        "stage" => 9878,
        _ => 9877, // prod
    }
}

/// Whether this is the STAGE profile.
pub fn is_stage() -> bool {
    BUILD_PROFILE == "stage"
}
