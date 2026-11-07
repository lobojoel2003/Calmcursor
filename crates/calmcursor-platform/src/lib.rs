//! # calmcursor-platform
//!
//! Connects the platform-independent [`calmcursor_core::Engine`] to the
//! operating system's pointer:
//!
//! | OS      | How hardware movement is intercepted           | How the smoothed pointer is applied     |
//! |---------|------------------------------------------------|-----------------------------------------|
//! | Windows | `WH_MOUSE_LL` low-level mouse hook             | `SetCursorPos`                          |
//! | macOS   | Quartz event tap (`CGEventTapCreate`)          | event rewriting + `CGEventPost`         |
//! | Linux   | exclusive `evdev` grab of each mouse           | a `uinput` virtual pointer              |
//!
//! The Linux backend works below the display server, so it covers X11 and
//! every Wayland compositor alike.
//!
//! Start it with [`Steadier::start`]; stop it by calling [`Steadier::stop`]
//! or dropping it. If the process dies, the OS releases the hooks and the
//! pointer behaves normally again.

mod runner;
mod simulated;
pub mod storage;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod unsupported;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as backend;
#[cfg(target_os = "macos")]
use macos as backend;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
use unsupported as backend;
#[cfg(windows)]
use windows as backend;

use calmcursor_core::simulate::TremorModel;
use calmcursor_core::{EngineStats, Settings, TraceSample};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub use runner::Shared;

impl PlatformError {
    pub(crate) fn os(message: impl Into<String>) -> Self {
        PlatformError::Os {
            message: message.into(),
        }
    }
}

/// A short description of the active backend, for the About screen.
pub const BACKEND: &str = backend::NAME;

/// Why steadying could not start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlatformError {
    /// The OS needs the user to grant access first. `help` says how.
    #[error("permission needed: {message}")]
    PermissionDenied { message: String, help: String },
    /// No pointing device could be found (Linux).
    #[error("no mouse or trackball was found")]
    NoDevices { help: String },
    /// This operating system is not supported.
    #[error("this operating system is not supported")]
    Unsupported,
    /// Any other OS error.
    #[error("{message}")]
    Os { message: String },
}

/// Whether the OS will let Calmcursor steady the pointer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PermissionStatus {
    Granted,
    /// Access is missing. `help` explains how to grant it.
    Needed {
        message: String,
        help: String,
    },
    /// Cannot tell without trying.
    Unknown,
}

/// A live snapshot of the steadier, for the app's status display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub running: bool,
    pub enabled: bool,
    pub backend: String,
    /// Devices being steadied (Linux lists them by name).
    pub devices: Vec<String>,
    /// The last non-fatal problem, if any.
    pub warning: Option<String>,
    pub stats: EngineStats,
}

/// Name reported by the simulated backend.
pub const SIMULATED_BACKEND: &str = simulated::NAME;

#[derive(Debug)]
enum Running {
    Native(backend::Backend),
    Simulated(simulated::Backend),
}

impl Running {
    fn stop(self) {
        match self {
            Running::Native(b) => b.stop(),
            Running::Simulated(b) => b.stop(),
        }
    }
}

/// A running pointer steadier.
#[derive(Debug)]
pub struct Steadier {
    shared: Arc<Shared>,
    backend: Option<Running>,
    backend_name: &'static str,
}

impl Steadier {
    /// Installs the OS hooks and starts steadying with `settings`.
    ///
    /// If the environment variable `CALMCURSOR_BACKEND` is `simulated`, a
    /// synthetic shaky hand is used instead of the real pointer (see
    /// [`Steadier::start_simulated`]).
    pub fn start(settings: &Settings) -> Result<Self, PlatformError> {
        if std::env::var("CALMCURSOR_BACKEND").is_ok_and(|v| v.eq_ignore_ascii_case("simulated")) {
            return Ok(Self::start_simulated(settings, TremorModel::moderate()));
        }
        let shared = Shared::new(settings);
        let backend = backend::Backend::start(shared.clone())?;
        log::info!("steadier started ({BACKEND})");
        Ok(Self {
            shared,
            backend: Some(Running::Native(backend)),
            backend_name: BACKEND,
        })
    }

    /// Starts a steadier fed by a simulated shaky hand instead of the real
    /// pointer. The real pointer is never touched. Used for demos, UI
    /// development and tests on machines without input devices.
    pub fn start_simulated(settings: &Settings, model: TremorModel) -> Self {
        let shared = Shared::new(settings);
        let backend = simulated::Backend::start(shared.clone(), model);
        log::info!("steadier started ({SIMULATED_BACKEND})");
        Self {
            shared,
            backend: Some(Running::Simulated(backend)),
            backend_name: SIMULATED_BACKEND,
        }
    }

    /// The backend in use.
    pub fn backend_name(&self) -> &'static str {
        self.backend_name
    }

    /// Applies new settings at once.
    pub fn apply_settings(&self, settings: &Settings) {
        self.shared.apply_settings(settings);
    }

    /// Pauses or resumes steadying without removing the hooks.
    pub fn set_enabled(&self, enabled: bool) {
        self.shared.set_enabled(enabled);
    }

    pub fn is_enabled(&self) -> bool {
        self.shared.with_engine(|e| e.is_enabled())
    }

    /// Starts or stops recording the live trace.
    pub fn set_trace(&self, on: bool) {
        self.shared.with_engine(|e| e.set_trace_enabled(on));
    }

    /// Returns the trace samples recorded since the last call.
    pub fn drain_trace(&self) -> Vec<TraceSample> {
        self.shared.with_engine(|e| e.drain_trace())
    }

    pub fn status(&self) -> Status {
        let (enabled, stats) = self.shared.with_engine(|e| (e.is_enabled(), e.stats()));
        Status {
            running: self.shared.is_running(),
            enabled,
            backend: self.backend_name.to_string(),
            devices: self.shared.devices(),
            warning: self.shared.warning(),
            stats,
        }
    }

    /// Removes the hooks and returns the pointer to normal.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(backend) = self.backend.take() {
            self.shared.stop();
            backend.stop();
            log::info!("steadier stopped");
        }
    }
}

impl Drop for Steadier {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Checks whether the OS will allow steadying, without changing anything.
pub fn permission_status() -> PermissionStatus {
    backend::permission_status()
}

/// Asks the OS to show its permission prompt, where there is one (macOS).
/// Returns true if access is already granted.
pub fn request_permission() -> bool {
    backend::request_permission()
}

/// A settings page or document that helps grant access, if the OS has one.
pub fn permission_help_url() -> Option<&'static str> {
    backend::PERMISSION_URL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_serialise_with_a_kind_tag() {
        let e = PlatformError::os("boom");
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"kind":"os","message":"boom"}"#
        );
        let p = PermissionStatus::Needed {
            message: "m".into(),
            help: "h".into(),
        };
        assert!(serde_json::to_string(&p)
            .unwrap()
            .contains(r#""state":"needed""#));
        assert_eq!(
            serde_json::to_string(&PlatformError::Unsupported).unwrap(),
            r#"{"kind":"unsupported"}"#
        );
    }
}
