//! Everything the app keeps while running, and the actions that change it.

use calmcursor_core::calibration::CalibrationSummary;
use calmcursor_core::simulate::{Demo, TremorModel};
use calmcursor_core::version::BuildInfo;
use calmcursor_core::{ClickSettings, EngineStats, OneEuroParams, Settings};
use calmcursor_platform::{storage, PermissionStatus, PlatformError, Steadier};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use tauri::menu::CheckMenuItem;
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

/// Size of the area the demo hand moves in (engine units).
const DEMO_AREA: (f64, f64) = (330.0, 205.0);

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the frontend sees.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub settings: Settings,
    /// The filter parameters in effect (from the slider or custom values).
    pub effective_filter: OneEuroParams,
    pub running: bool,
    pub enabled: bool,
    pub backend: String,
    pub devices: Vec<String>,
    pub warning: Option<String>,
    pub error: Option<PlatformError>,
    pub permission: PermissionStatus,
    pub permission_url: Option<String>,
    pub stats: EngineStats,
    pub build: BuildInfo,
    pub os: &'static str,
    pub settings_path: Option<String>,
    pub calibrating: bool,
    pub demo: bool,
    pub shortcut_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TracePayload {
    pub source: &'static str,
    /// `[t, raw x, raw y, smoothed x, smoothed y, flags]`
    pub samples: Vec<[f64; 6]>,
}

#[derive(Default)]
pub struct AppState {
    settings: Mutex<Settings>,
    steadier: Mutex<Option<Steadier>>,
    start_error: Mutex<Option<PlatformError>>,
    demo: Mutex<Option<Demo>>,
    trace_on: AtomicBool,
    calibrating: AtomicBool,
    shortcut_error: Mutex<Option<String>>,
    pub tray_toggle: Mutex<Option<CheckMenuItem<Wry>>>,
}

impl AppState {
    pub fn load() -> Self {
        let state = Self::default();
        *lock(&state.settings) = storage::load();
        state
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    fn save(&self, settings: &Settings) {
        if let Err(e) = storage::save(settings) {
            log::error!("could not save settings: {e}");
        }
    }

    /// Starts steadying if it is not running.
    pub fn start_engine(&self) {
        let mut slot = lock(&self.steadier);
        if slot.is_some() {
            return;
        }
        let mut settings = self.settings();
        if self.calibrating.load(Ordering::Relaxed) {
            settings.enabled = false;
        }
        match Steadier::start(&settings) {
            Ok(s) => {
                s.set_trace(self.trace_on.load(Ordering::Relaxed));
                *slot = Some(s);
                *lock(&self.start_error) = None;
            }
            Err(e) => {
                log::warn!("steadying could not start: {e}");
                *lock(&self.start_error) = Some(e);
            }
        }
    }

    /// True when steadying is not running only because access is missing.
    pub fn waiting_for_permission(&self) -> bool {
        lock(&self.steadier).is_none()
            && matches!(
                *lock(&self.start_error),
                Some(PlatformError::PermissionDenied { .. })
            )
    }

    pub fn stop_engine(&self) {
        if let Some(s) = lock(&self.steadier).take() {
            s.stop();
        }
    }

    pub fn restart_engine(&self) {
        self.stop_engine();
        self.start_engine();
    }

    pub fn snapshot(&self) -> Snapshot {
        let settings = self.settings();
        let status = lock(&self.steadier).as_ref().map(Steadier::status);
        Snapshot {
            effective_filter: settings.filter.params(),
            running: status.as_ref().is_some_and(|s| s.running),
            enabled: settings.enabled,
            backend: status.as_ref().map_or_else(
                || calmcursor_platform::BACKEND.to_string(),
                |s| s.backend.clone(),
            ),
            devices: status
                .as_ref()
                .map(|s| s.devices.clone())
                .unwrap_or_default(),
            warning: status.as_ref().and_then(|s| s.warning.clone()),
            error: lock(&self.start_error).clone(),
            permission: calmcursor_platform::permission_status(),
            permission_url: calmcursor_platform::permission_help_url().map(str::to_string),
            stats: status.map(|s| s.stats).unwrap_or_default(),
            build: BuildInfo::current(),
            os: std::env::consts::OS,
            settings_path: storage::settings_path().map(|p| p.display().to_string()),
            calibrating: self.calibrating.load(Ordering::Relaxed),
            demo: lock(&self.demo).is_some(),
            shortcut_error: lock(&self.shortcut_error).clone(),
            settings,
        }
    }

    /// Tells the window and the tray that something changed.
    pub fn notify(&self, app: &AppHandle) {
        let snap = self.snapshot();
        if let Some(item) = lock(&self.tray_toggle).as_ref() {
            let _ = item.set_checked(snap.enabled);
        }
        if let Some(tray) = app.tray_by_id("main") {
            let _ = tray.set_tooltip(Some(if snap.enabled {
                "Calmcursor: steadying on"
            } else {
                "Calmcursor: paused"
            }));
        }
        let _ = app.emit("state-changed", snap);
    }

    pub fn set_enabled(&self, app: &AppHandle, enabled: bool) {
        let settings = {
            let mut s = lock(&self.settings);
            s.enabled = enabled;
            s.clone()
        };
        self.save(&settings);
        if !self.calibrating.load(Ordering::Relaxed) {
            if let Some(st) = lock(&self.steadier).as_ref() {
                st.set_enabled(enabled);
            }
        }
        self.notify(app);
    }

    pub fn toggle(&self, app: &AppHandle) {
        let on = !self.settings().enabled;
        self.set_enabled(app, on);
    }

    /// Replaces the settings, saves them and applies every change.
    pub fn update_settings(&self, app: &AppHandle, new: Settings) {
        let new = new.sanitized();
        let old = std::mem::replace(&mut *lock(&self.settings), new.clone());
        self.save(&new);

        let mut applied = new.clone();
        if self.calibrating.load(Ordering::Relaxed) {
            applied.enabled = false;
        }
        if let Some(st) = lock(&self.steadier).as_ref() {
            st.apply_settings(&applied);
        }
        if let Some(demo) = lock(&self.demo).as_mut() {
            demo.apply_settings(&Settings {
                enabled: true,
                ..new.clone()
            });
        }
        if old.launch_at_login != new.launch_at_login {
            self.apply_autostart(app, new.launch_at_login);
        }
        if old.pause_shortcut != new.pause_shortcut {
            self.register_shortcut(app);
        }
        self.notify(app);
    }

    pub fn apply_autostart(&self, app: &AppHandle, on: bool) {
        let manager = app.autolaunch();
        let result = if on {
            manager.enable()
        } else {
            manager.disable()
        };
        if let Err(e) = result {
            log::warn!("could not change launch at login: {e}");
        }
    }

    /// (Re-)registers the pause shortcut from the settings.
    pub fn register_shortcut(&self, app: &AppHandle) {
        let gs = app.global_shortcut();
        let _ = gs.unregister_all();
        let shortcut = self.settings().pause_shortcut;
        let error = if shortcut.is_empty() {
            None
        } else {
            gs.register(shortcut.as_str()).err().map(|e| {
                log::warn!("could not register shortcut {shortcut}: {e}");
                format!("“{shortcut}” could not be used: {e}")
            })
        };
        *lock(&self.shortcut_error) = error;
    }

    pub fn set_trace(&self, on: bool) {
        self.trace_on.store(on, Ordering::Relaxed);
        if let Some(st) = lock(&self.steadier).as_ref() {
            st.set_trace(on);
        }
    }

    pub fn set_demo(&self, on: bool, tremor: Option<&str>) {
        let mut demo = lock(&self.demo);
        if on {
            let model = tremor
                .and_then(TremorModel::by_name)
                .unwrap_or_else(TremorModel::moderate);
            let settings = Settings {
                enabled: true,
                ..self.settings()
            };
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_secs());
            let mut d = Demo::new(&settings, model, DEMO_AREA.0, DEMO_AREA.1, seed);
            d.advance(0.6);
            d.drain_trace();
            *demo = Some(d);
        } else {
            *demo = None;
        }
    }

    /// Collects trace samples for the window. `dt` is the real time elapsed
    /// since the last call (drives the demo).
    pub fn collect_trace(&self, dt: f64) -> Option<TracePayload> {
        if !self.trace_on.load(Ordering::Relaxed) {
            return None;
        }
        let (source, samples) = if let Some(demo) = lock(&self.demo).as_mut() {
            demo.advance(dt);
            ("demo", demo.drain_trace())
        } else {
            let steadier = lock(&self.steadier);
            ("live", steadier.as_ref()?.drain_trace())
        };
        if samples.is_empty() {
            return None;
        }
        Some(TracePayload {
            source,
            samples: samples
                .iter()
                .map(|s| [s.t, s.raw.x, s.raw.y, s.out.x, s.out.y, f64::from(s.flags)])
                .collect(),
        })
    }

    /// Pauses steadying while the calibration game records natural movement.
    pub fn begin_calibration(&self, app: &AppHandle) {
        self.calibrating.store(true, Ordering::Relaxed);
        if let Some(st) = lock(&self.steadier).as_ref() {
            st.set_enabled(false);
        }
        self.notify(app);
    }

    pub fn end_calibration(&self, app: &AppHandle) {
        if !self.calibrating.swap(false, Ordering::Relaxed) {
            return;
        }
        let enabled = self.settings().enabled;
        if let Some(st) = lock(&self.steadier).as_ref() {
            st.set_enabled(enabled);
        }
        self.notify(app);
    }

    pub fn apply_calibration(
        &self,
        app: &AppHandle,
        filter: OneEuroParams,
        steadiness: f64,
        click: ClickSettings,
        summary: CalibrationSummary,
    ) {
        let mut s = self.settings();
        s.filter.custom = Some(filter);
        s.filter.steadiness = steadiness;
        s.click = click;
        s.calibration = Some(summary);
        self.update_settings(app, s);
    }

    pub fn shutdown(&self) {
        self.set_demo(false, None);
        self.stop_engine();
    }
}

/// Brings the main window to the front.
pub fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
