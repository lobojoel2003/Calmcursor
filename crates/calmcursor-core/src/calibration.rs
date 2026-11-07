//! Turning the 30-second calibration game into personal settings.
//!
//! The game has three short rounds, recorded with steadying paused:
//!
//! 1. **Rest**: hold the pointer inside a circle. Shows the tremor at rest.
//! 2. **Follow**: follow a slowly gliding dot. Shows the tremor while moving
//!    and how fast the person moves.
//! 3. **Tap**: click a few targets. Shows how far the pointer slips during a
//!    press and whether presses bounce.
//!
//! [`analyze`] measures the tremor, then tries a grid of filter settings on
//! the person's own recordings. It keeps the ones that remove enough of the
//! tremor and, from those, picks the one that follows deliberate movement
//! most closely (the least lag).

use crate::filter::{OneEuro2D, OneEuroParams};
use crate::geometry::Vec2;
use crate::settings::{ClickSettings, FilterSettings};
use crate::tremor::{self, PointerSample, Severity, TremorProfile, ANALYSIS_HZ};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Seconds dropped from the start of each round while the person settles in.
const SETTLE_S: f64 = 0.5;
const MIN_CUTOFFS: [f64; 9] = [0.2, 0.3, 0.45, 0.65, 0.9, 1.3, 1.9, 2.7, 3.5];
const BETAS: [f64; 9] = [
    0.0005, 0.001, 0.0018, 0.003, 0.005, 0.008, 0.013, 0.02, 0.03,
];

/// A pointer sample from the follow round, with the dot's position.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FollowSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    /// Where the dot was.
    pub tx: f64,
    pub ty: f64,
}

/// One target click from the tap round.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TapRecord {
    pub down_t: f64,
    pub down_x: f64,
    pub down_y: f64,
    pub up_t: f64,
    pub up_x: f64,
    pub up_y: f64,
}

impl TapRecord {
    pub fn drift(&self) -> f64 {
        Vec2::new(self.down_x, self.down_y).distance(Vec2::new(self.up_x, self.up_y))
    }

    pub fn duration(&self) -> f64 {
        (self.up_t - self.down_t).max(0.0)
    }
}

/// A raw button transition from the tap round (for bounce detection).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PressRecord {
    pub t: f64,
    pub down: bool,
}

/// Everything recorded during the calibration game. Times are seconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalibrationRecording {
    pub hold: Vec<PointerSample>,
    pub follow: Vec<FollowSample>,
    pub taps: Vec<TapRecord>,
    pub presses: Vec<PressRecord>,
}

/// Why a recording could not be analysed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalibrationError {
    /// A round was too short (fewer than two seconds of movement).
    NotEnoughData { round: &'static str },
}

impl fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotEnoughData { round } => write!(
                f,
                "not enough pointer movement was recorded in the {round} round; please try again"
            ),
        }
    }
}

impl std::error::Error for CalibrationError {}

/// Statistics from the tap round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TapStats {
    pub count: usize,
    pub median_drift_px: f64,
    pub max_drift_px: f64,
    pub median_press_ms: f64,
    /// Presses that followed a release within 150 ms.
    pub bounces: usize,
}

/// A raw path next to the smoothed one, for the results screen.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Replay {
    pub raw: Vec<[f64; 2]>,
    pub smoothed: Vec<[f64; 2]>,
    /// The dot's path (follow round only).
    pub target: Vec<[f64; 2]>,
}

/// The short version kept in settings ("last calibrated …").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibrationSummary {
    /// Seconds since the Unix epoch.
    pub date_unix: u64,
    pub frequency_hz: Option<f64>,
    pub amplitude_px: f64,
    pub severity: Severity,
    pub reduction_percent: f64,
    pub lag_ms: f64,
}

/// Recommended settings and everything needed to explain them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibrationReport {
    pub profile: TremorProfile,
    pub filter: OneEuroParams,
    /// The steadiness slider position closest to `filter`.
    pub steadiness: f64,
    pub click: ClickSettings,
    /// Share of the resting tremor removed with these settings (0–100).
    pub reduction_percent: f64,
    /// Share of the tremor removed while moving (0–100).
    pub moving_reduction_percent: f64,
    /// How far the smoothed pointer trails a deliberate move (ms).
    pub lag_ms: f64,
    pub taps: TapStats,
    pub hold_replay: Replay,
    pub follow_replay: Replay,
    pub summary: CalibrationSummary,
}

/// Runs the One Euro filter over a uniformly sampled path.
pub fn run_filter(signal: &[Vec2], fs: f64, params: OneEuroParams) -> Vec<Vec2> {
    let mut f = OneEuro2D::new(params);
    signal
        .iter()
        .enumerate()
        .map(|(i, &p)| f.filter(i as f64 / fs, p))
        .collect()
}

/// Smooths an irregularly sampled recording with `params`, sample by sample,
/// exactly as the live engine would.
pub fn preview(samples: &[PointerSample], params: OneEuroParams) -> Vec<Vec2> {
    let mut f = OneEuro2D::new(params);
    samples.iter().map(|s| f.filter(s.t, s.pos())).collect()
}

fn percentile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let idx = ((v.len() - 1) as f64 * q.clamp(0.0, 1.0)).round() as usize;
    v[idx]
}

fn skip_settle(v: &[Vec2]) -> &[Vec2] {
    let skip = (SETTLE_S * ANALYSIS_HZ) as usize;
    if v.len() > skip * 3 {
        &v[skip..]
    } else {
        v
    }
}

fn to_pairs(v: &[Vec2], step: usize) -> Vec<[f64; 2]> {
    v.iter().step_by(step.max(1)).map(|p| [p.x, p.y]).collect()
}

struct Candidate {
    params: OneEuroParams,
    hold_jitter: f64,
    follow_jitter: f64,
    tracking_error: f64,
}

/// Analyses a calibration recording. `date_unix` is stored in the summary.
pub fn analyze(
    rec: &CalibrationRecording,
    date_unix: u64,
) -> Result<CalibrationReport, CalibrationError> {
    let fs = ANALYSIS_HZ;
    let hold_full = tremor::resample(&rec.hold, fs);
    let hold = skip_settle(&hold_full);
    let profile = tremor::analyze_resampled(hold, fs)
        .ok_or(CalibrationError::NotEnoughData { round: "rest" })?;

    let follow_pointer: Vec<PointerSample> = rec
        .follow
        .iter()
        .map(|s| PointerSample::new(s.t, s.x, s.y))
        .collect();
    let follow_target: Vec<PointerSample> = rec
        .follow
        .iter()
        .map(|s| PointerSample::new(s.t, s.tx, s.ty))
        .collect();
    let follow_full = tremor::resample(&follow_pointer, fs);
    let target_full = tremor::resample(&follow_target, fs);
    let have_follow = follow_full.len() as f64 >= fs * 2.0;

    let follow = skip_settle(&follow_full);
    let (follow_trend, follow_residual) = tremor::split_trend(follow, fs);
    let hold_raw_jitter = tremor::rms(&tremor::high_pass(hold, fs));
    let follow_raw_jitter = tremor::rms(&follow_residual);

    let hold_goal = (0.15 * hold_raw_jitter).max(0.35);
    let follow_goal = (0.45 * follow_raw_jitter).max(0.5);
    let settle = (SETTLE_S * fs) as usize;

    let evaluate = |params: OneEuroParams| -> Candidate {
        let h = run_filter(hold, fs, params);
        let hold_jitter = tremor::rms(&tremor::high_pass(&h[settle.min(h.len())..], fs));
        let (follow_jitter, tracking_error) = if have_follow {
            let f = run_filter(follow, fs, params);
            let s = settle.min(f.len());
            (
                tremor::rms(&tremor::high_pass(&f[s..], fs)),
                tremor::rms_distance(&f[s..], &follow_trend[s..]),
            )
        } else {
            (0.0, 0.0)
        };
        Candidate {
            params,
            hold_jitter,
            follow_jitter,
            tracking_error,
        }
    };

    let candidates: Vec<Candidate> = MIN_CUTOFFS
        .iter()
        .flat_map(|&mc| BETAS.iter().map(move |&b| OneEuroParams::new(mc, b, 1.0)))
        .map(evaluate)
        .collect();

    let feasible = |c: &&Candidate| c.hold_jitter <= hold_goal && c.follow_jitter <= follow_goal;
    let best = candidates
        .iter()
        .filter(feasible)
        .min_by(|a, b| {
            a.tracking_error
                .total_cmp(&b.tracking_error)
                .then(b.params.min_cutoff.total_cmp(&a.params.min_cutoff))
        })
        .or_else(|| {
            // Nothing meets the goal: take the steadiest overall.
            candidates.iter().min_by(|a, b| {
                let score =
                    |c: &Candidate| c.hold_jitter / hold_goal + c.follow_jitter / follow_goal;
                score(a).total_cmp(&score(b))
            })
        })
        .expect("candidate grid is never empty");
    let filter = best.params;

    let reduction = |before: f64, after: f64| {
        if before > 1e-9 {
            ((1.0 - after / before) * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        }
    };
    let reduction_percent = reduction(hold_raw_jitter, best.hold_jitter);
    let moving_reduction_percent = reduction(follow_raw_jitter, best.follow_jitter);

    let follow_smoothed_full = run_filter(&follow_full, fs, filter);
    let lag_ms = if have_follow {
        let s = settle.min(follow_smoothed_full.len());
        let smoothed = &follow_smoothed_full[s..];
        let trend = tremor::lowpass_2d(&follow_full, fs, tremor::TREND_CUTOFF_HZ);
        tremor::estimate_lag(&trend[s..], smoothed, fs, 0.4) * 1000.0
    } else {
        0.0
    };

    // Click lock and bounce filter from the tap round.
    let drifts: Vec<f64> = rec.taps.iter().map(TapRecord::drift).collect();
    let durations: Vec<f64> = rec.taps.iter().map(|t| t.duration() * 1000.0).collect();
    let mut presses = rec.presses.clone();
    presses.sort_by(|a, b| a.t.total_cmp(&b.t));
    let bounce_gaps: Vec<f64> = presses
        .windows(2)
        .filter(|w| !w[0].down && w[1].down && w[1].t - w[0].t < 0.15)
        .map(|w| w[1].t - w[0].t)
        .collect();
    let taps = TapStats {
        count: rec.taps.len(),
        median_drift_px: percentile(&drifts, 0.5),
        max_drift_px: drifts.iter().copied().fold(0.0, f64::max),
        median_press_ms: percentile(&durations, 0.5),
        bounces: bounce_gaps.len(),
    };
    let defaults = ClickSettings::default();
    let radius_from_taps = if drifts.is_empty() {
        defaults.anchor_radius_px
    } else {
        percentile(&drifts, 0.9) * 1.3 + 3.0
    };
    let click = ClickSettings {
        anchor_enabled: true,
        anchor_radius_px: radius_from_taps
            .max(profile.amplitude_px * 1.5)
            .clamp(6.0, 40.0)
            .round(),
        anchor_ms: if durations.is_empty() {
            defaults.anchor_ms
        } else {
            ((percentile(&durations, 0.9) + 120.0).clamp(180.0, 600.0) / 10.0).round() as u32 * 10
        },
        debounce_ms: bounce_gaps
            .iter()
            .copied()
            .filter(|g| *g < 0.1)
            .fold(None, |acc: Option<f64>, g| {
                Some(acc.map_or(g, |a| a.max(g)))
            })
            .map_or(0, |g| {
                ((g * 1000.0 + 25.0).clamp(40.0, 100.0) / 5.0).round() as u32 * 5
            }),
    }
    .sanitized();

    let hold_smoothed = run_filter(&hold_full, fs, filter);
    let summary = CalibrationSummary {
        date_unix,
        frequency_hz: profile.frequency_hz,
        amplitude_px: profile.amplitude_px,
        severity: profile.severity,
        reduction_percent,
        lag_ms,
    };

    Ok(CalibrationReport {
        steadiness: FilterSettings::steadiness_for_min_cutoff(filter.min_cutoff),
        filter,
        click,
        reduction_percent,
        moving_reduction_percent,
        lag_ms,
        taps,
        hold_replay: Replay {
            raw: to_pairs(&hold_full, 2),
            smoothed: to_pairs(&hold_smoothed, 2),
            target: Vec::new(),
        },
        follow_replay: Replay {
            raw: to_pairs(&follow_full, 2),
            smoothed: to_pairs(&follow_smoothed_full, 2),
            target: to_pairs(&target_full, 2),
        },
        summary,
        profile,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulate::{ShakyHand, TremorModel};
    use std::f64::consts::TAU;

    /// Simulates the whole game for a given tremor.
    fn record(model: TremorModel, seed: u64) -> CalibrationRecording {
        let mut hand = ShakyHand::new(model, seed);
        let hz = 60.0;
        let mut rec = CalibrationRecording::default();
        let mut t = 0.0;
        for _ in 0..(10.0 * hz) as usize {
            let p = Vec2::new(500.0, 350.0) + hand.offset(t);
            rec.hold.push(PointerSample::new(t, p.x, p.y));
            t += 1.0 / hz;
        }
        let start = t;
        for _ in 0..(12.0 * hz) as usize {
            let u = t - start;
            let target = Vec2::new(
                500.0 + 260.0 * (TAU * u / 6.0).sin(),
                350.0 + 120.0 * (2.0 * TAU * u / 6.0).sin(),
            );
            // The person trails the dot a little.
            let lagged = Vec2::new(
                500.0 + 260.0 * (TAU * (u - 0.15) / 6.0).sin(),
                350.0 + 120.0 * (2.0 * TAU * (u - 0.15) / 6.0).sin(),
            );
            let p = lagged + hand.offset(t);
            rec.follow.push(FollowSample {
                t,
                x: p.x,
                y: p.y,
                tx: target.x,
                ty: target.y,
            });
            t += 1.0 / hz;
        }
        for i in 0..6 {
            let down_t = t + i as f64;
            let down = Vec2::new(300.0, 300.0) + hand.offset(down_t);
            let up = down + hand.offset(down_t + 0.12) * 0.8;
            rec.taps.push(TapRecord {
                down_t,
                down_x: down.x,
                down_y: down.y,
                up_t: down_t + 0.12,
                up_x: up.x,
                up_y: up.y,
            });
            rec.presses.push(PressRecord {
                t: down_t,
                down: true,
            });
            rec.presses.push(PressRecord {
                t: down_t + 0.12,
                down: false,
            });
        }
        rec
    }

    #[test]
    fn strong_tremor_gets_strong_smoothing() {
        let report = analyze(&record(TremorModel::strong(), 1), 0).unwrap();
        let f = report.profile.frequency_hz.expect("tremor found");
        assert!((f - 4.8).abs() < 0.5, "frequency {f}");
        assert!(
            report.reduction_percent > 70.0,
            "reduction {}",
            report.reduction_percent
        );
        assert!(report.steadiness > 50.0, "steadiness {}", report.steadiness);
        assert!(report.click.anchor_enabled);
        assert!(report.click.anchor_radius_px >= 10.0);
        assert_eq!(report.taps.count, 6);
        assert_eq!(report.taps.bounces, 0);
        assert_eq!(report.click.debounce_ms, 0);
        assert!(
            report.lag_ms > 0.0 && report.lag_ms < 400.0,
            "lag {}",
            report.lag_ms
        );
        assert!(!report.follow_replay.raw.is_empty());
        assert_eq!(
            report.follow_replay.raw.len(),
            report.follow_replay.smoothed.len()
        );
    }

    #[test]
    fn milder_tremor_gets_lighter_settings() {
        let strong = analyze(&record(TremorModel::strong(), 2), 0).unwrap();
        let mild = analyze(&record(TremorModel::mild(), 2), 0).unwrap();
        assert!(
            mild.filter.min_cutoff > strong.filter.min_cutoff,
            "mild {:?} vs strong {:?}",
            mild.filter,
            strong.filter
        );
        assert!(mild.lag_ms <= strong.lag_ms + 1.0);
    }

    #[test]
    fn steady_hand_gets_light_settings() {
        let mut model = TremorModel::new(6.0, 0.0);
        model.noise_px = 0.0;
        let report = analyze(&record(model, 3), 0).unwrap();
        assert_eq!(report.profile.severity, Severity::Minimal);
        assert!(report.steadiness < 40.0, "steadiness {}", report.steadiness);
    }

    #[test]
    fn bounces_enable_the_bounce_filter() {
        let mut rec = record(TremorModel::moderate(), 4);
        let t = rec.presses.last().unwrap().t;
        rec.presses.push(PressRecord {
            t: t + 0.05,
            down: true,
        });
        rec.presses.push(PressRecord {
            t: t + 0.09,
            down: false,
        });
        let report = analyze(&rec, 0).unwrap();
        assert_eq!(report.taps.bounces, 1);
        assert!(report.click.debounce_ms >= 40 && report.click.debounce_ms <= 100);
    }

    #[test]
    fn missing_follow_and_taps_still_work() {
        let mut rec = record(TremorModel::moderate(), 5);
        rec.follow.clear();
        rec.taps.clear();
        rec.presses.clear();
        let report = analyze(&rec, 1_700_000_000).unwrap();
        assert_eq!(report.lag_ms, 0.0);
        assert_eq!(
            report.click.anchor_radius_px,
            report.click.anchor_radius_px.round()
        );
        assert_eq!(report.summary.date_unix, 1_700_000_000);
    }

    #[test]
    fn empty_recording_is_an_error() {
        let err = analyze(&CalibrationRecording::default(), 0).unwrap_err();
        assert_eq!(err, CalibrationError::NotEnoughData { round: "rest" });
        assert!(err.to_string().contains("rest"));
    }

    #[test]
    fn preview_matches_engine_filter() {
        let samples: Vec<PointerSample> = (0..100)
            .map(|i| PointerSample::new(i as f64 / 60.0, i as f64, 0.0))
            .collect();
        let out = preview(&samples, OneEuroParams::default());
        assert_eq!(out.len(), 100);
        assert_eq!(out[0], Vec2::new(0.0, 0.0));
        assert!(out[99].x < 99.0);
    }

    #[test]
    fn recording_deserialises_from_partial_json() {
        let rec: CalibrationRecording =
            serde_json::from_str(r#"{"hold":[{"t":0,"x":1,"y":2}]}"#).unwrap();
        assert_eq!(rec.hold.len(), 1);
        assert!(rec.follow.is_empty());
    }
}
