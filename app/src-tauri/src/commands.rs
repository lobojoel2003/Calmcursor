//! Commands the frontend calls with `invoke`.

use crate::state::{AppState, Snapshot};
use calmcursor_core::calibration::{
    self, CalibrationRecording, CalibrationReport, CalibrationSummary,
};
use calmcursor_core::{ClickSettings, OneEuroParams, Settings};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

/// Pages Calmcursor may open in the browser.
const ALLOWED_URL_PREFIXES: &[&str] = &[
    "https://github.com/lobojoel2003/Calmcursor",
    "https://gery.casiez.net/1euro/",
    "https://www.gnu.org/licenses/",
    "x-apple.systempreferences:",
];

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> Snapshot {
    state.snapshot()
}

#[tauri::command]
pub fn set_enabled(app: AppHandle, state: State<'_, AppState>, enabled: bool) -> Snapshot {
    state.set_enabled(&app, enabled);
    state.snapshot()
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Snapshot {
    state.update_settings(&app, settings);
    state.snapshot()
}

#[tauri::command]
pub fn reset_settings(app: AppHandle, state: State<'_, AppState>) -> Snapshot {
    let enabled = state.settings().enabled;
    state.update_settings(
        &app,
        Settings {
            enabled,
            ..Settings::default()
        },
    );
    state.snapshot()
}

#[tauri::command]
pub fn retry_engine(app: AppHandle, state: State<'_, AppState>) -> Snapshot {
    state.restart_engine();
    state.notify(&app);
    state.snapshot()
}

#[tauri::command]
pub fn request_permission() -> bool {
    calmcursor_platform::request_permission()
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    if !ALLOWED_URL_PREFIXES.iter().any(|p| url.starts_with(p)) {
        return Err("this link is not allowed".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_trace(state: State<'_, AppState>, on: bool) {
    state.set_trace(on);
}

#[tauri::command]
pub fn set_demo(state: State<'_, AppState>, on: bool, tremor: Option<String>) -> Snapshot {
    state.set_demo(on, tremor.as_deref());
    state.snapshot()
}

#[tauri::command]
pub fn begin_calibration(app: AppHandle, state: State<'_, AppState>) {
    state.begin_calibration(&app);
}

#[tauri::command]
pub fn end_calibration(app: AppHandle, state: State<'_, AppState>) {
    state.end_calibration(&app);
}

#[tauri::command]
pub async fn analyze_calibration(
    recording: CalibrationRecording,
) -> Result<CalibrationReport, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    calibration::analyze(&recording, now).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn apply_calibration(
    app: AppHandle,
    state: State<'_, AppState>,
    filter: OneEuroParams,
    steadiness: f64,
    click: ClickSettings,
    summary: CalibrationSummary,
) -> Snapshot {
    state.apply_calibration(&app, filter, steadiness, click, summary);
    state.snapshot()
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}
