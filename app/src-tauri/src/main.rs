//! Calmcursor desktop app.
//!
//! Hosts the steadying engine (from `calmcursor-platform`), shows the
//! settings window with the calibration game and live trace, and lives in
//! the tray / menu bar so steadying keeps running when the window is closed.

// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod state;
mod tray;

use state::{show_main_window, AppState};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::ShortcutState;

/// How often trace samples are sent to the window (about 30 fps).
const TRACE_INTERVAL: Duration = Duration::from_millis(33);

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Calmcursor {}", calmcursor_core::version::long_version());
    let start_hidden_arg = std::env::args().any(|a| a == "--minimized" || a == "--hidden");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app)
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        app.state::<AppState>().toggle(app);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            app.manage(AppState::load());
            let handle = app.handle().clone();
            let state = app.state::<AppState>();
            state.start_engine();
            if state.waiting_for_permission() && cfg!(target_os = "macos") {
                // Shows macOS's own "allow Accessibility" prompt.
                calmcursor_platform::request_permission();
            }
            state.register_shortcut(&handle);
            let settings = state.settings();
            if settings.launch_at_login {
                state.apply_autostart(&handle, true);
            }
            if let Err(e) = tray::create(&handle) {
                log::warn!("tray icon unavailable: {e}");
            }
            spawn_trace_pump(handle.clone());
            spawn_permission_watcher(handle.clone());
            if !(start_hidden_arg || settings.start_minimized) {
                show_main_window(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Keep steadying in the background; the tray brings it back.
                api.prevent_close();
                let _ = window.hide();
                let app = window.app_handle();
                let state = app.state::<AppState>();
                state.end_calibration(app);
                state.set_trace(false);
                state.set_demo(false, None);
                let _ = app.emit("window-hidden", ());
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::set_enabled,
            commands::save_settings,
            commands::reset_settings,
            commands::retry_engine,
            commands::request_permission,
            commands::open_url,
            commands::set_trace,
            commands::set_demo,
            commands::begin_calibration,
            commands::end_calibration,
            commands::analyze_calibration,
            commands::apply_calibration,
            commands::quit,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the Calmcursor app");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            app.state::<AppState>().shutdown();
            log::info!("Calmcursor exited; the pointer is back to normal");
        }
    });
}

/// Starts steadying by itself once the person grants access, so they never
/// have to come back and press "Try again".
fn spawn_permission_watcher(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("calmcursor-permission".into())
        .spawn(move || {
            let mut was_granted = false;
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let state = app.state::<AppState>();
                if !state.waiting_for_permission() {
                    was_granted = false;
                    continue;
                }
                let granted = calmcursor_platform::permission_status()
                    == calmcursor_platform::PermissionStatus::Granted;
                // Retry only when access has just been granted.
                if granted && !was_granted {
                    log::info!("access granted; starting steadying");
                    state.restart_engine();
                    state.notify(&app);
                }
                was_granted = granted;
            }
        })
        .expect("failed to spawn the permission watcher");
}

/// Sends trace samples to the window while it is showing the live trace.
fn spawn_trace_pump(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("calmcursor-trace".into())
        .spawn(move || {
            let mut last = Instant::now();
            loop {
                std::thread::sleep(TRACE_INTERVAL);
                let dt = last.elapsed().as_secs_f64();
                last = Instant::now();
                if let Some(payload) = app.state::<AppState>().collect_trace(dt) {
                    let _ = app.emit("trace", payload);
                }
            }
        })
        .expect("failed to spawn the trace thread");
}
