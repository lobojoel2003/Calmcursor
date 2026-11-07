//! User settings, persisted as JSON.
//!
//! Every field has a default and unknown fields are ignored, so settings files
//! written by older or newer versions still load.

use crate::calibration::CalibrationSummary;
use crate::filter::OneEuroParams;
use serde::{Deserialize, Serialize};

/// Current settings schema version.
pub const SETTINGS_VERSION: u32 = 1;

/// How strongly to smooth the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterSettings {
    /// 0 (light) to 100 (strongest). Used unless `custom` is set.
    pub steadiness: f64,
    /// Exact filter parameters, set by calibration or advanced settings.
    pub custom: Option<OneEuroParams>,
}

impl FilterSettings {
    pub const DEFAULT_STEADINESS: f64 = 50.0;

    // Steadiness 0 and 100 map to these (log-interpolated in between).
    const LIGHT: (f64, f64) = (3.0, 0.02); // (min_cutoff Hz, beta)
    const STRONG: (f64, f64) = (0.3, 0.002);
    const D_CUTOFF: f64 = 1.0;

    /// Filter parameters for a steadiness value from 0 to 100.
    pub fn params_for_steadiness(steadiness: f64) -> OneEuroParams {
        let s = if steadiness.is_finite() {
            steadiness.clamp(0.0, 100.0) / 100.0
        } else {
            0.5
        };
        let log_lerp = |a: f64, b: f64| (a.ln() + (b.ln() - a.ln()) * s).exp();
        OneEuroParams::new(
            log_lerp(Self::LIGHT.0, Self::STRONG.0),
            log_lerp(Self::LIGHT.1, Self::STRONG.1),
            Self::D_CUTOFF,
        )
    }

    /// The steadiness value whose resting cutoff matches `min_cutoff`. Used to
    /// place the slider after calibration.
    pub fn steadiness_for_min_cutoff(min_cutoff: f64) -> f64 {
        let (lo, hi) = (Self::LIGHT.0.ln(), Self::STRONG.0.ln());
        let v = (min_cutoff.max(1e-6).ln() - lo) / (hi - lo) * 100.0;
        if v.is_finite() {
            v.clamp(0.0, 100.0)
        } else {
            Self::DEFAULT_STEADINESS
        }
    }

    /// The parameters actually used by the filter.
    pub fn params(&self) -> OneEuroParams {
        match self.custom {
            Some(p) => p.sanitized(),
            None => Self::params_for_steadiness(self.steadiness),
        }
    }
}

impl Default for FilterSettings {
    fn default() -> Self {
        Self {
            steadiness: Self::DEFAULT_STEADINESS,
            custom: None,
        }
    }
}

/// Helpers that make clicking easier.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClickSettings {
    /// "Click lock": hold the pointer still for a moment after a button is
    /// pressed, so a shaky press does not become a drag or slip off target.
    pub anchor_enabled: bool,
    /// Moving further than this (px) while pressed starts a drag at once.
    pub anchor_radius_px: f64,
    /// Longest time (ms) the pointer is held after a press.
    pub anchor_ms: u32,
    /// Ignore a second press of the same button this soon (ms) after a
    /// release. 0 turns it off. Keep it short so double-clicks still work.
    pub debounce_ms: u32,
}

impl ClickSettings {
    pub const RADIUS_RANGE: (f64, f64) = (2.0, 80.0);
    pub const ANCHOR_MS_RANGE: (u32, u32) = (50, 1000);
    pub const DEBOUNCE_MS_MAX: u32 = 150;

    pub fn sanitized(self) -> Self {
        let d = Self::default();
        Self {
            anchor_enabled: self.anchor_enabled,
            anchor_radius_px: if self.anchor_radius_px.is_finite() {
                self.anchor_radius_px
                    .clamp(Self::RADIUS_RANGE.0, Self::RADIUS_RANGE.1)
            } else {
                d.anchor_radius_px
            },
            anchor_ms: self
                .anchor_ms
                .clamp(Self::ANCHOR_MS_RANGE.0, Self::ANCHOR_MS_RANGE.1),
            debounce_ms: self.debounce_ms.min(Self::DEBOUNCE_MS_MAX),
        }
    }
}

impl Default for ClickSettings {
    fn default() -> Self {
        Self {
            anchor_enabled: true,
            anchor_radius_px: 10.0,
            anchor_ms: 250,
            debounce_ms: 0,
        }
    }
}

/// Everything Calmcursor stores between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    /// Steadying on or off.
    pub enabled: bool,
    pub filter: FilterSettings,
    pub click: ClickSettings,
    /// How often (Hz) the smoothed pointer is updated.
    pub tick_hz: u32,
    pub launch_at_login: bool,
    /// Start hidden in the tray / menu bar.
    pub start_minimized: bool,
    /// Global shortcut that pauses and resumes steadying. Empty disables it.
    pub pause_shortcut: String,
    /// Summary of the last calibration, if any.
    pub calibration: Option<CalibrationSummary>,
}

impl Settings {
    pub const TICK_HZ_RANGE: (u32, u32) = (60, 1000);
    pub const DEFAULT_PAUSE_SHORTCUT: &'static str = "CommandOrControl+Alt+Shift+S";

    /// Returns a copy with every value inside its valid range.
    pub fn sanitized(mut self) -> Self {
        self.version = SETTINGS_VERSION;
        self.filter.steadiness = if self.filter.steadiness.is_finite() {
            self.filter.steadiness.clamp(0.0, 100.0)
        } else {
            FilterSettings::DEFAULT_STEADINESS
        };
        self.filter.custom = self.filter.custom.map(OneEuroParams::sanitized);
        self.click = self.click.sanitized();
        self.tick_hz = self
            .tick_hz
            .clamp(Self::TICK_HZ_RANGE.0, Self::TICK_HZ_RANGE.1);
        self.pause_shortcut = self.pause_shortcut.trim().to_string();
        self
    }

    /// Seconds between engine ticks.
    pub fn tick_interval(&self) -> f64 {
        1.0 / f64::from(
            self.tick_hz
                .clamp(Self::TICK_HZ_RANGE.0, Self::TICK_HZ_RANGE.1),
        )
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            enabled: true,
            filter: FilterSettings::default(),
            click: ClickSettings::default(),
            tick_hz: 250,
            launch_at_login: false,
            start_minimized: false,
            pause_shortcut: Self::DEFAULT_PAUSE_SHORTCUT.to_string(),
            calibration: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steadiness_mapping_is_monotonic() {
        let mut prev = FilterSettings::params_for_steadiness(0.0);
        assert!((prev.min_cutoff - 3.0).abs() < 1e-9);
        for s in 1..=100 {
            let p = FilterSettings::params_for_steadiness(s as f64);
            assert!(p.min_cutoff < prev.min_cutoff);
            assert!(p.beta < prev.beta);
            prev = p;
        }
        assert!((prev.min_cutoff - 0.3).abs() < 1e-9);
    }

    #[test]
    fn steadiness_inverse_roundtrips() {
        for s in [0.0, 12.5, 50.0, 77.0, 100.0] {
            let p = FilterSettings::params_for_steadiness(s);
            let back = FilterSettings::steadiness_for_min_cutoff(p.min_cutoff);
            assert!((back - s).abs() < 1e-6, "{s} -> {back}");
        }
        assert_eq!(FilterSettings::steadiness_for_min_cutoff(100.0), 0.0);
        assert_eq!(FilterSettings::steadiness_for_min_cutoff(0.001), 100.0);
    }

    #[test]
    fn custom_params_override_steadiness() {
        let f = FilterSettings {
            steadiness: 0.0,
            custom: Some(OneEuroParams::new(0.7, 0.004, 1.0)),
        };
        assert_eq!(f.params().min_cutoff, 0.7);
    }

    #[test]
    fn json_roundtrip_and_partial_files() {
        let s = Settings::default();
        let json = serde_json::to_string_pretty(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);

        // Old or hand-written files with missing and unknown fields still load.
        let partial: Settings =
            serde_json::from_str(r#"{"enabled": false, "click": {"anchor_ms": 400}, "future": 1}"#)
                .unwrap();
        assert!(!partial.enabled);
        assert_eq!(partial.click.anchor_ms, 400);
        assert_eq!(partial.click.anchor_radius_px, 10.0);
        assert_eq!(partial.tick_hz, 250);
    }

    #[test]
    fn sanitize_clamps_everything() {
        let mut s = Settings::default();
        s.filter.steadiness = 500.0;
        s.tick_hz = 5;
        s.click.anchor_radius_px = f64::INFINITY;
        s.click.anchor_ms = 0;
        s.click.debounce_ms = 10_000;
        s.pause_shortcut = "  F9 ".into();
        let s = s.sanitized();
        assert_eq!(s.filter.steadiness, 100.0);
        assert_eq!(s.tick_hz, 60);
        assert_eq!(s.click.anchor_radius_px, 10.0);
        assert_eq!(s.click.anchor_ms, 50);
        assert_eq!(s.click.debounce_ms, ClickSettings::DEBOUNCE_MS_MAX);
        assert_eq!(s.pause_shortcut, "F9");
    }
}
