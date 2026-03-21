use tauri::State;

use crate::config::settings::{AppSettings, SettingsState};

#[tauri::command]
pub async fn get_settings(settings: State<'_, SettingsState>) -> Result<AppSettings, String> {
    let s = settings.read().await;
    Ok(s.clone())
}

#[tauri::command]
pub async fn update_settings(
    settings: State<'_, SettingsState>,
    new_settings: AppSettings,
) -> Result<(), String> {
    let mut s = settings.write().await;
    *s = new_settings;
    Ok(())
}
