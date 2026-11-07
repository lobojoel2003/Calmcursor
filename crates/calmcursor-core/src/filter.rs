//! The One Euro filter, an adaptive low-pass filter for noisy pointer input.
//!
//! Reference: Géry Casiez, Nicolas Roussel and Daniel Vogel, "1€ Filter: A
//! Simple Speed-based Low-pass Filter for Noisy Input in Interactive Systems",
//! CHI 2012. <https://gery.casiez.net/1euro/>
//!
//! The filter's cutoff frequency rises with the (smoothed) speed of the
//! pointer:
//!
//! ```text
//! cutoff = min_cutoff + beta * |speed|
//! ```
//!
//! When the hand is nearly still, the cutoff sits at `min_cutoff` (well below
//! typical tremor frequencies of 4–12 Hz), so the shaking is filtered out.
//! During a deliberate move the speed rises, the cutoff opens up and the
//! pointer keeps up with the hand.
//!
//! Tremor itself is fast but small, and the speed estimate is low-pass
//! filtered at `d_cutoff` (1 Hz by default), so tremor barely raises the
//! cutoff. That property is what makes One Euro a good fit for tremor.
//!
//! [`OneEuro2D`] filters both axes together and drives one shared cutoff from
//! the 2D speed, so diagonal moves are treated the same as straight ones.

use crate::geometry::Vec2;
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

/// Smallest time step the filter accepts, in seconds. Guards against
/// duplicate timestamps.
const MIN_DT: f64 = 1e-4;

/// Tuning parameters for the One Euro filter.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OneEuroParams {
    /// Cutoff frequency (Hz) when the pointer is still. Lower is steadier
    /// but adds lag at the start of a movement.
    pub min_cutoff: f64,
    /// How quickly the cutoff opens with speed (Hz per px/s). Higher is more
    /// responsive during fast moves.
    pub beta: f64,
    /// Cutoff (Hz) used to smooth the speed estimate.
    pub d_cutoff: f64,
}

impl OneEuroParams {
    pub const MIN_CUTOFF_RANGE: (f64, f64) = (0.05, 20.0);
    pub const BETA_RANGE: (f64, f64) = (0.0, 1.0);
    pub const D_CUTOFF_RANGE: (f64, f64) = (0.1, 20.0);

    pub const fn new(min_cutoff: f64, beta: f64, d_cutoff: f64) -> Self {
        Self {
            min_cutoff,
            beta,
            d_cutoff,
        }
    }

    /// Returns a copy with every field finite and within its valid range.
    pub fn sanitized(self) -> Self {
        fn fix(v: f64, (lo, hi): (f64, f64), default: f64) -> f64 {
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                default
            }
        }
        let d = Self::default();
        Self {
            min_cutoff: fix(self.min_cutoff, Self::MIN_CUTOFF_RANGE, d.min_cutoff),
            beta: fix(self.beta, Self::BETA_RANGE, d.beta),
            d_cutoff: fix(self.d_cutoff, Self::D_CUTOFF_RANGE, d.d_cutoff),
        }
    }

    /// The cutoff the filter uses at a given speed (px/s).
    pub fn cutoff_at_speed(&self, speed: f64) -> f64 {
        self.min_cutoff + self.beta * speed.abs()
    }
}

impl Default for OneEuroParams {
    /// Balanced settings: equivalent to steadiness 50.
    fn default() -> Self {
        crate::settings::FilterSettings::params_for_steadiness(
            crate::settings::FilterSettings::DEFAULT_STEADINESS,
        )
    }
}

/// Smoothing factor of a first-order low-pass filter.
#[inline]
pub fn smoothing_factor(cutoff_hz: f64, dt: f64) -> f64 {
    let tau = 1.0 / (TAU * cutoff_hz.max(1e-6));
    1.0 / (1.0 + tau / dt)
}

#[derive(Clone, Copy, Debug)]
struct State {
    t: f64,
    /// Last raw input, used for the speed estimate.
    raw: Vec2,
    /// Last filtered output.
    value: Vec2,
    velocity: Vec2,
}

/// A two-axis One Euro filter with a shared, speed-driven cutoff.
#[derive(Clone, Debug)]
pub struct OneEuro2D {
    params: OneEuroParams,
    state: Option<State>,
}

impl OneEuro2D {
    pub fn new(params: OneEuroParams) -> Self {
        Self {
            params: params.sanitized(),
            state: None,
        }
    }

    pub fn params(&self) -> OneEuroParams {
        self.params
    }

    /// Changes the tuning without resetting the filter state.
    pub fn set_params(&mut self, params: OneEuroParams) {
        self.params = params.sanitized();
    }

    /// The last output value, if the filter has been fed.
    pub fn value(&self) -> Option<Vec2> {
        self.state.map(|s| s.value)
    }

    /// The smoothed speed estimate in units per second.
    pub fn speed(&self) -> f64 {
        self.state.map_or(0.0, |s| s.velocity.length())
    }

    /// Time of the last sample.
    pub fn last_time(&self) -> Option<f64> {
        self.state.map(|s| s.t)
    }

    /// Forgets all history. The next sample passes straight through.
    pub fn reset(&mut self) {
        self.state = None;
    }

    /// Restarts the filter at rest at `value`, as if it had settled there by
    /// time `t`. Later samples glide away from `value` instead of jumping.
    pub fn reset_to(&mut self, t: f64, value: Vec2) {
        self.state = Some(State {
            t,
            raw: value,
            value,
            velocity: Vec2::ZERO,
        });
    }

    /// Feeds one sample taken at time `t` (seconds) and returns the filtered value.
    pub fn filter(&mut self, t: f64, x: Vec2) -> Vec2 {
        if !x.is_finite() || !t.is_finite() {
            return self.value().unwrap_or(Vec2::ZERO);
        }
        let Some(prev) = self.state else {
            self.state = Some(State {
                t,
                raw: x,
                value: x,
                velocity: Vec2::ZERO,
            });
            return x;
        };

        let dt = (t - prev.t).max(MIN_DT);
        let p = self.params;

        // Speed of the raw input (as in the reference implementation), itself
        // low-pass filtered so tremor barely moves it.
        let raw_velocity = (x - prev.raw) / dt;
        let a_d = smoothing_factor(p.d_cutoff, dt);
        let velocity = prev.velocity.lerp(raw_velocity, a_d);

        // Speed-dependent cutoff, shared by both axes.
        let cutoff = p.cutoff_at_speed(velocity.length());
        let a = smoothing_factor(cutoff, dt);
        let value = prev.value.lerp(x, a);

        self.state = Some(State {
            t: t.max(prev.t),
            raw: x,
            value,
            velocity,
        });
        value
    }
}

/// Amplitude response of a first-order low-pass filter at `freq_hz`.
pub fn first_order_gain(cutoff_hz: f64, freq_hz: f64) -> f64 {
    1.0 / (1.0 + (freq_hz / cutoff_hz.max(1e-9)).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> OneEuroParams {
        OneEuroParams::new(1.0, 0.007, 1.0)
    }

    #[test]
    fn first_sample_passes_through() {
        let mut f = OneEuro2D::new(params());
        assert_eq!(f.filter(0.0, Vec2::new(10.0, 20.0)), Vec2::new(10.0, 20.0));
    }

    #[test]
    fn constant_input_is_unchanged() {
        let mut f = OneEuro2D::new(params());
        let p = Vec2::new(5.0, -3.0);
        for i in 0..100 {
            let out = f.filter(i as f64 / 250.0, p);
            assert!(out.distance(p) < 1e-12);
        }
    }

    #[test]
    fn converges_after_step() {
        let mut f = OneEuro2D::new(params());
        f.filter(0.0, Vec2::ZERO);
        let target = Vec2::new(100.0, 0.0);
        let mut out = Vec2::ZERO;
        let mut prev_x = 0.0;
        for i in 1..=1000 {
            out = f.filter(i as f64 / 250.0, target);
            // Monotonic: never overshoots, never goes backwards.
            assert!(out.x >= prev_x - 1e-9);
            assert!(out.x <= 100.0 + 1e-9);
            prev_x = out.x;
        }
        assert!(out.distance(target) < 0.01, "did not converge: {out:?}");
    }

    #[test]
    fn attenuates_tremor_frequency() {
        // 6 Hz, 8 px tremor around a still point.
        let mut f = OneEuro2D::new(OneEuroParams::new(0.5, 0.005, 1.0));
        let mut peak: f64 = 0.0;
        for i in 0..2500 {
            let t = i as f64 / 250.0;
            let x = Vec2::new(8.0 * (TAU * 6.0 * t).sin(), 0.0);
            let out = f.filter(t, x);
            if t > 2.0 {
                peak = peak.max(out.x.abs());
            }
        }
        assert!(peak < 1.5, "tremor peak after filter = {peak}");
    }

    #[test]
    fn fast_moves_open_the_filter() {
        // A deliberate move at 1000 px/s should lag much less than a
        // fixed low-pass at min_cutoff would.
        let p = OneEuroParams::new(0.5, 0.01, 1.0);
        let mut adaptive = OneEuro2D::new(p);
        let mut fixed = OneEuro2D::new(OneEuroParams::new(0.5, 0.0, 1.0));
        let mut a_out = Vec2::ZERO;
        let mut f_out = Vec2::ZERO;
        for i in 0..=250 {
            let t = i as f64 / 250.0;
            let x = Vec2::new(1000.0 * t, 0.0);
            a_out = adaptive.filter(t, x);
            f_out = fixed.filter(t, x);
        }
        let a_lag = 1000.0 - a_out.x;
        let f_lag = 1000.0 - f_out.x;
        assert!(a_lag < f_lag * 0.5, "adaptive lag {a_lag} vs fixed {f_lag}");
    }

    #[test]
    fn reset_to_glides_from_anchor() {
        let mut f = OneEuro2D::new(params());
        f.reset_to(1.0, Vec2::new(10.0, 10.0));
        let out = f.filter(1.004, Vec2::new(20.0, 10.0));
        assert!(out.x > 10.0 && out.x < 12.0, "{out:?}");
    }

    #[test]
    fn non_finite_input_is_ignored() {
        let mut f = OneEuro2D::new(params());
        f.filter(0.0, Vec2::new(1.0, 1.0));
        let out = f.filter(0.01, Vec2::new(f64::NAN, 0.0));
        assert_eq!(out, Vec2::new(1.0, 1.0));
        assert_eq!(f.value(), Some(Vec2::new(1.0, 1.0)));
    }

    #[test]
    fn duplicate_timestamps_are_safe() {
        let mut f = OneEuro2D::new(params());
        f.filter(0.0, Vec2::ZERO);
        let out = f.filter(0.0, Vec2::new(5.0, 5.0));
        assert!(out.is_finite());
    }

    #[test]
    fn sanitize_clamps_and_replaces_nan() {
        let p = OneEuroParams::new(f64::NAN, -1.0, 1000.0).sanitized();
        assert_eq!(p.min_cutoff, OneEuroParams::default().min_cutoff);
        assert_eq!(p.beta, 0.0);
        assert_eq!(p.d_cutoff, 20.0);
    }

    #[test]
    fn gain_helper() {
        assert!((first_order_gain(1.0, 1.0) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        assert!(first_order_gain(1.0, 10.0) < 0.1);
    }
}
