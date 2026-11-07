//! Measuring tremor from a recording of pointer positions.
//!
//! The recording is resampled to a steady rate, split into a slow "intended"
//! trend and a fast residual, and the residual's spectrum is searched for a
//! dominant frequency in the 3–12 Hz band where Parkinsonian and essential
//! tremor usually sit.
//!
//! This is a tuning aid, not a medical measurement.

use crate::geometry::Vec2;
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, SQRT_2, TAU};

/// Rate (Hz) recordings are resampled to before analysis.
pub const ANALYSIS_HZ: f64 = 120.0;
/// Movement slower than this (Hz) counts as intended, not tremor.
pub const TREND_CUTOFF_HZ: f64 = 2.0;
/// Band searched for the dominant tremor frequency.
pub const TREMOR_BAND_HZ: (f64, f64) = (3.0, 12.0);
/// Band shown in the spectrum chart.
pub const SPECTRUM_BAND_HZ: (f64, f64) = (2.0, 15.0);

/// A pointer position at time `t` (seconds).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointerSample {
    pub t: f64,
    pub x: f64,
    pub y: f64,
}

impl PointerSample {
    pub fn new(t: f64, x: f64, y: f64) -> Self {
        Self { t, x, y }
    }

    pub fn pos(&self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}

/// Rough size of the tremor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Minimal,
    Mild,
    Moderate,
    Strong,
}

impl Severity {
    /// Classifies a typical swing (peak amplitude, px).
    pub fn from_amplitude(amplitude_px: f64) -> Self {
        match amplitude_px {
            a if a < 1.5 => Severity::Minimal,
            a if a < 4.0 => Severity::Mild,
            a if a < 9.0 => Severity::Moderate,
            _ => Severity::Strong,
        }
    }
}

/// The result of analysing a recording.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TremorProfile {
    /// Dominant tremor frequency, if there is a clear one.
    pub frequency_hz: Option<f64>,
    /// Typical swing either side of the intended position (px).
    pub amplitude_px: f64,
    /// Root-mean-square size of the tremor movement (px).
    pub rms_px: f64,
    /// Typical peak speed of the tremor itself (px/s).
    pub peak_speed_px_s: f64,
    /// How much of the fast movement sits at the dominant frequency (0–1).
    pub confidence: f64,
    pub severity: Severity,
    /// `[frequency Hz, relative power 0–1]` pairs over 2–15 Hz.
    pub spectrum: Vec<[f64; 2]>,
    /// Length of the analysed recording (s).
    pub duration_s: f64,
}

/// Resamples irregular samples onto a uniform grid at `hz` with linear
/// interpolation. Out-of-order and duplicate timestamps are skipped.
pub fn resample(samples: &[PointerSample], hz: f64) -> Vec<Vec2> {
    let clean: Vec<&PointerSample> = {
        let mut out: Vec<&PointerSample> = Vec::with_capacity(samples.len());
        for s in samples {
            if !(s.t.is_finite() && s.x.is_finite() && s.y.is_finite()) {
                continue;
            }
            if out.last().is_none_or(|last| s.t > last.t) {
                out.push(s);
            }
        }
        out
    };
    if clean.len() < 2 || hz <= 0.0 {
        return clean.iter().map(|s| s.pos()).collect();
    }
    let t0 = clean[0].t;
    let t1 = clean[clean.len() - 1].t;
    let n = ((t1 - t0) * hz).floor() as usize + 1;
    let mut out = Vec::with_capacity(n);
    let mut j = 0;
    for i in 0..n {
        let t = t0 + i as f64 / hz;
        while j + 2 < clean.len() && clean[j + 1].t < t {
            j += 1;
        }
        let (a, b) = (clean[j], clean[j + 1]);
        let u = ((t - a.t) / (b.t - a.t)).clamp(0.0, 1.0);
        out.push(a.pos().lerp(b.pos(), u));
    }
    out
}

/// Second-order Butterworth low-pass coefficients (b0, b1, b2, a1, a2).
fn butterworth_lowpass(fs: f64, fc: f64) -> [f64; 5] {
    let w0 = TAU * (fc / fs).min(0.49);
    let (sin, cos) = w0.sin_cos();
    let alpha = sin / SQRT_2; // Q = 1/sqrt(2)
    let a0 = 1.0 + alpha;
    let b0 = (1.0 - cos) / 2.0 / a0;
    [b0, 2.0 * b0, b0, -2.0 * cos / a0, (1.0 - alpha) / a0]
}

fn biquad_pass(x: &[f64], c: &[f64; 5]) -> Vec<f64> {
    let [b0, b1, b2, a1, a2] = *c;
    let first = x.first().copied().unwrap_or(0.0);
    // Start in steady state at the first value (DC gain is 1).
    let (mut x1, mut x2, mut y1, mut y2) = (first, first, first, first);
    x.iter()
        .map(|&x0| {
            let y0 = b0 * x0 + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
            x2 = x1;
            x1 = x0;
            y2 = y1;
            y1 = y0;
            y0
        })
        .collect()
}

/// Zero-phase (forward-backward) Butterworth low-pass of a 1D signal, with
/// odd reflection padding to tame edge effects.
pub fn lowpass_zero_phase(x: &[f64], fs: f64, fc: f64) -> Vec<f64> {
    let n = x.len();
    if n < 3 {
        return x.to_vec();
    }
    let pad = ((fs / fc) as usize * 2).min(n - 1);
    let mut padded = Vec::with_capacity(n + 2 * pad);
    for i in (1..=pad).rev() {
        padded.push(2.0 * x[0] - x[i]);
    }
    padded.extend_from_slice(x);
    for i in 1..=pad {
        padded.push(2.0 * x[n - 1] - x[n - 1 - i]);
    }
    let c = butterworth_lowpass(fs, fc);
    let mut y = biquad_pass(&padded, &c);
    y.reverse();
    let mut y = biquad_pass(&y, &c);
    y.reverse();
    y[pad..pad + n].to_vec()
}

/// Zero-phase low-pass of a 2D signal.
pub fn lowpass_2d(signal: &[Vec2], fs: f64, fc: f64) -> Vec<Vec2> {
    let xs: Vec<f64> = signal.iter().map(|p| p.x).collect();
    let ys: Vec<f64> = signal.iter().map(|p| p.y).collect();
    let lx = lowpass_zero_phase(&xs, fs, fc);
    let ly = lowpass_zero_phase(&ys, fs, fc);
    lx.into_iter()
        .zip(ly)
        .map(|(x, y)| Vec2::new(x, y))
        .collect()
}

/// Splits a signal into its slow trend and the fast residual (tremor).
pub fn split_trend(signal: &[Vec2], fs: f64) -> (Vec<Vec2>, Vec<Vec2>) {
    let trend = lowpass_2d(signal, fs, TREND_CUTOFF_HZ);
    let residual = signal.iter().zip(&trend).map(|(&s, &t)| s - t).collect();
    (trend, residual)
}

/// The fast part of a signal (above [`TREND_CUTOFF_HZ`]).
pub fn high_pass(signal: &[Vec2], fs: f64) -> Vec<Vec2> {
    split_trend(signal, fs).1
}

/// Root-mean-square length of 2D vectors.
pub fn rms(v: &[Vec2]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    (v.iter().map(|p| p.length_squared()).sum::<f64>() / v.len() as f64).sqrt()
}

/// Root-mean-square distance between two equally long paths.
pub fn rms_distance(a: &[Vec2], b: &[Vec2]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    (a.iter()
        .zip(b)
        .map(|(&p, &q)| (p - q).length_squared())
        .sum::<f64>()
        / n as f64)
        .sqrt()
}

/// Power of the combined x/y signal at `freqs` (Hann window).
pub fn power_spectrum(residual: &[Vec2], fs: f64, freqs: &[f64]) -> Vec<f64> {
    let n = residual.len();
    if n < 4 {
        return vec![0.0; freqs.len()];
    }
    let window: Vec<f64> = (0..n)
        .map(|i| 0.5 - 0.5 * (TAU * i as f64 / (n - 1) as f64).cos())
        .collect();
    freqs
        .iter()
        .map(|&f| {
            let w = TAU * f / fs;
            let (mut xr, mut xi, mut yr, mut yi) = (0.0, 0.0, 0.0, 0.0);
            for (i, (p, win)) in residual.iter().zip(&window).enumerate() {
                let (s, c) = (w * i as f64).sin_cos();
                xr += win * p.x * c;
                xi += win * p.x * s;
                yr += win * p.y * c;
                yi += win * p.y * s;
            }
            xr * xr + xi * xi + yr * yr + yi * yi
        })
        .collect()
}

/// Analyses a recording of someone holding the pointer still (or moving it
/// slowly). Returns `None` if there are fewer than two seconds of data.
pub fn analyze(samples: &[PointerSample]) -> Option<TremorProfile> {
    let signal = resample(samples, ANALYSIS_HZ);
    analyze_resampled(&signal, ANALYSIS_HZ)
}

/// Like [`analyze`], for a signal already sampled at `fs`.
pub fn analyze_resampled(signal: &[Vec2], fs: f64) -> Option<TremorProfile> {
    if (signal.len() as f64) < fs * 2.0 {
        return None;
    }
    let duration_s = (signal.len() - 1) as f64 / fs;
    let (_, residual) = split_trend(signal, fs);
    let rms_px = rms(&residual);
    let amplitude_px = rms_px * SQRT_2;

    // Spectrum on a 0.05 Hz grid; the display copy uses a 0.25 Hz grid.
    let step = 0.05;
    let count = ((SPECTRUM_BAND_HZ.1 - SPECTRUM_BAND_HZ.0) / step).round() as usize + 1;
    let freqs: Vec<f64> = (0..count)
        .map(|i| SPECTRUM_BAND_HZ.0 + i as f64 * step)
        .collect();
    let power = power_spectrum(&residual, fs, &freqs);
    let total: f64 = power.iter().sum();

    let in_band = |f: f64| f >= TREMOR_BAND_HZ.0 && f <= TREMOR_BAND_HZ.1;
    let peak = freqs
        .iter()
        .zip(&power)
        .enumerate()
        .filter(|(_, (&f, _))| in_band(f))
        .max_by(|a, b| a.1 .1.total_cmp(b.1 .1))
        .map(|(i, _)| i);

    let (frequency_hz, confidence) = match peak {
        Some(i) if total > 0.0 => {
            // Parabolic interpolation for sub-bin accuracy.
            let f = if i > 0 && i + 1 < power.len() {
                let (a, b, c) = (power[i - 1], power[i], power[i + 1]);
                let denom = a - 2.0 * b + c;
                let shift = if denom.abs() > f64::EPSILON {
                    0.5 * (a - c) / denom
                } else {
                    0.0
                };
                freqs[i] + shift.clamp(-0.5, 0.5) * step
            } else {
                freqs[i]
            };
            let near: f64 = freqs
                .iter()
                .zip(&power)
                .filter(|(&g, _)| (g - f).abs() <= 0.75)
                .map(|(_, &p)| p)
                .sum();
            (f, near / total)
        }
        _ => (0.0, 0.0),
    };
    let detected = confidence >= 0.3 && amplitude_px >= 0.5;
    let frequency_hz = detected.then_some(frequency_hz);

    let max_power = power.iter().copied().fold(0.0, f64::max);
    let spectrum = freqs
        .iter()
        .zip(&power)
        .enumerate()
        .filter(|(i, _)| i % 5 == 0)
        .map(|(_, (&f, &p))| [f, if max_power > 0.0 { p / max_power } else { 0.0 }])
        .collect();

    let peak_speed_px_s = TAU * frequency_hz.unwrap_or(6.0) * amplitude_px;
    Some(TremorProfile {
        frequency_hz,
        amplitude_px,
        rms_px,
        peak_speed_px_s,
        confidence,
        severity: Severity::from_amplitude(amplitude_px),
        spectrum,
        duration_s,
    })
}

/// Estimates how far (seconds) `delayed` trails `reference`, searching shifts
/// from 0 to `max_lag` s.
pub fn estimate_lag(reference: &[Vec2], delayed: &[Vec2], fs: f64, max_lag: f64) -> f64 {
    let n = reference.len().min(delayed.len());
    let max_shift = ((max_lag * fs) as usize).min(n / 2);
    let mut best = (0usize, f64::INFINITY);
    for shift in 0..=max_shift {
        let err: f64 = (shift..n)
            .map(|i| (delayed[i] - reference[i - shift]).length_squared())
            .sum::<f64>()
            / (n - shift).max(1) as f64;
        if err < best.1 {
            best = (shift, err);
        }
    }
    best.0 as f64 / fs
}

/// Minimum-jerk easing from 0 to 1, used for natural-looking reaches.
pub fn min_jerk(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    u * u * u * (10.0 - 15.0 * u + 6.0 * u * u)
}

/// Half a cosine from 0 to 1, a gentle ease in and out.
pub fn ease_cos(u: f64) -> f64 {
    0.5 - 0.5 * (PI * u.clamp(0.0, 1.0)).cos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulate::{Rng, ShakyHand, TremorModel};

    fn sine_recording(freq: f64, amp: f64, secs: f64, hz: f64) -> Vec<PointerSample> {
        (0..(secs * hz) as usize)
            .map(|i| {
                let t = i as f64 / hz;
                PointerSample::new(t, 400.0 + amp * (TAU * freq * t).sin(), 300.0)
            })
            .collect()
    }

    #[test]
    fn resample_interpolates_and_skips_bad_samples() {
        let s = vec![
            PointerSample::new(0.0, 0.0, 0.0),
            PointerSample::new(0.0, 99.0, 99.0), // duplicate time
            PointerSample::new(f64::NAN, 1.0, 1.0),
            PointerSample::new(1.0, 10.0, 20.0),
        ];
        let r = resample(&s, 10.0);
        assert_eq!(r.len(), 11);
        assert!((r[5].x - 5.0).abs() < 1e-9 && (r[5].y - 10.0).abs() < 1e-9);
        assert_eq!(r[10], Vec2::new(10.0, 20.0));
    }

    #[test]
    fn lowpass_keeps_slow_and_removes_fast() {
        let fs = 120.0;
        let x: Vec<f64> = (0..1200)
            .map(|i| {
                let t = i as f64 / fs;
                100.0 * (TAU * 0.3 * t).sin() + 5.0 * (TAU * 6.0 * t).sin()
            })
            .collect();
        let y = lowpass_zero_phase(&x, fs, 2.0);
        for (i, v) in y.iter().enumerate().take(1080).skip(120) {
            let t = i as f64 / fs;
            assert!((v - 100.0 * (TAU * 0.3 * t).sin()).abs() < 0.5, "at {i}");
        }
    }

    #[test]
    fn detects_frequency_and_amplitude_of_a_sine() {
        let p = analyze(&sine_recording(5.0, 6.0, 10.0, 60.0)).unwrap();
        let f = p.frequency_hz.unwrap();
        assert!((f - 5.0).abs() < 0.1, "frequency {f}");
        assert!(
            (p.amplitude_px - 6.0).abs() < 0.6,
            "amplitude {}",
            p.amplitude_px
        );
        assert!(p.confidence > 0.8);
        assert_eq!(p.severity, Severity::Moderate);
        assert!(!p.spectrum.is_empty());
    }

    #[test]
    fn detects_simulated_tremor_while_moving() {
        let mut hand = ShakyHand::new(TremorModel::new(4.6, 7.0), 3);
        let samples: Vec<PointerSample> = (0..900)
            .map(|i| {
                let t = i as f64 / 75.0;
                let intended = Vec2::new(400.0 + 200.0 * (TAU * 0.25 * t).sin(), 300.0);
                let p = intended + hand.offset(t);
                PointerSample::new(t, p.x, p.y)
            })
            .collect();
        let p = analyze(&samples).unwrap();
        let f = p.frequency_hz.expect("tremor detected");
        assert!((f - 4.6).abs() < 0.35, "frequency {f}");
        assert!(
            p.amplitude_px > 4.0 && p.amplitude_px < 10.0,
            "{}",
            p.amplitude_px
        );
    }

    #[test]
    fn no_dominant_frequency_in_noise() {
        let mut rng = Rng::new(7);
        let samples: Vec<PointerSample> = (0..1200)
            .map(|i| PointerSample::new(i as f64 / 120.0, rng.normal() * 2.0, rng.normal() * 2.0))
            .collect();
        let p = analyze(&samples).unwrap();
        assert!(
            p.frequency_hz.is_none(),
            "{:?} conf {}",
            p.frequency_hz,
            p.confidence
        );
    }

    #[test]
    fn still_hand_is_minimal() {
        let samples: Vec<PointerSample> = (0..600)
            .map(|i| PointerSample::new(i as f64 / 60.0, 300.0, 200.0))
            .collect();
        let p = analyze(&samples).unwrap();
        assert_eq!(p.amplitude_px, 0.0);
        assert_eq!(p.frequency_hz, None);
        assert_eq!(p.severity, Severity::Minimal);
    }

    #[test]
    fn too_short_returns_none() {
        assert!(analyze(&sine_recording(5.0, 5.0, 1.0, 60.0)).is_none());
    }

    #[test]
    fn lag_estimate_finds_shift() {
        let fs = 120.0;
        let a: Vec<Vec2> = (0..600)
            .map(|i| Vec2::new((TAU * 0.5 * i as f64 / fs).sin() * 100.0, 0.0))
            .collect();
        let mut b = vec![a[0]; 12];
        b.extend_from_slice(&a[..588]);
        let lag = estimate_lag(&a, &b, fs, 0.4);
        assert!((lag - 0.1).abs() < 1e-9, "lag {lag}");
    }

    #[test]
    fn easing_curves() {
        assert_eq!(min_jerk(0.0), 0.0);
        assert_eq!(min_jerk(1.0), 1.0);
        assert!((min_jerk(0.5) - 0.5).abs() < 1e-12);
        assert!((ease_cos(0.5) - 0.5).abs() < 1e-12);
    }
}
