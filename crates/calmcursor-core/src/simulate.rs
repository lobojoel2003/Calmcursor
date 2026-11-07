//! A synthetic shaky hand.
//!
//! Used by the app's demo mode (to show what steadying does without needing a
//! real tremor), by the CLI to draw the README chart, and by the tests. Every
//! run with the same seed produces exactly the same movement.

use crate::engine::{Button, Engine, TraceSample};
use crate::geometry::Vec2;
use crate::settings::Settings;
use crate::tremor::min_jerk;
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

/// A small, fast, deterministic random number generator (xorshift64*).
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Standard normal (Box–Muller).
    pub fn normal(&mut self) -> f64 {
        let u1 = self.next_f64().max(1e-12);
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos()
    }
}

/// Describes a tremor to simulate.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TremorModel {
    /// Centre frequency (Hz).
    pub frequency_hz: f64,
    /// Typical swing either side of the intended position (px).
    pub amplitude_px: f64,
    /// Ratio of the minor to the major axis of the shaking (0 = a line).
    pub ellipticity: f64,
    /// Direction of the main axis (radians).
    pub angle: f64,
    /// Extra small random jitter (px).
    pub noise_px: f64,
}

impl TremorModel {
    pub fn new(frequency_hz: f64, amplitude_px: f64) -> Self {
        Self {
            frequency_hz,
            amplitude_px,
            ellipticity: 0.55,
            angle: 0.5,
            noise_px: 0.3,
        }
    }

    pub fn mild() -> Self {
        Self::new(6.5, 2.5)
    }

    pub fn moderate() -> Self {
        Self::new(5.4, 6.0)
    }

    pub fn strong() -> Self {
        Self::new(4.8, 12.0)
    }

    pub fn by_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "mild" => Some(Self::mild()),
            "moderate" => Some(Self::moderate()),
            "strong" => Some(Self::strong()),
            _ => None,
        }
    }
}

/// Produces tremor offsets over time: a wandering-frequency oscillation with
/// a slowly breathing amplitude, plus a little noise.
#[derive(Clone, Debug)]
pub struct ShakyHand {
    model: TremorModel,
    rng: Rng,
    phase: f64,
    wander: f64,
    env_phase: f64,
    last_t: Option<f64>,
}

impl ShakyHand {
    pub fn new(model: TremorModel, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let phase = rng.next_f64() * TAU;
        let env_phase = rng.next_f64() * TAU;
        Self {
            model,
            rng,
            phase,
            wander: 0.0,
            env_phase,
            last_t: None,
        }
    }

    pub fn model(&self) -> TremorModel {
        self.model
    }

    /// Tremor offset at time `t`. Call with increasing times.
    pub fn offset(&mut self, t: f64) -> Vec2 {
        let dt = self.last_t.map_or(0.0, |last| (t - last).max(0.0));
        self.last_t = Some(t);
        // Frequency wanders ±~5% (Ornstein–Uhlenbeck).
        self.wander += -self.wander * dt * 1.5 + self.rng.normal() * (dt * 0.003).sqrt();
        let freq = self.model.frequency_hz * (1.0 + self.wander.clamp(-0.12, 0.12));
        self.phase = (self.phase + TAU * freq * dt) % TAU;
        let envelope = 0.8 + 0.2 * (TAU * 0.23 * t + self.env_phase).sin();
        let a = self.model.amplitude_px * envelope;
        let major = a * self.phase.sin();
        let minor = a * self.model.ellipticity * (self.phase + 0.9).sin();
        let (s, c) = self.model.angle.sin_cos();
        let noise = Vec2::new(self.rng.normal(), self.rng.normal()) * self.model.noise_px;
        Vec2::new(major * c - minor * s, major * s + minor * c) + noise
    }
}

/// A looping path that reaches between waypoints and pauses at each one, the
/// way someone moves to a button and then clicks it.
#[derive(Clone, Debug)]
pub struct DemoPath {
    waypoints: Vec<Vec2>,
    move_s: f64,
    dwell_s: f64,
}

/// Where along a [`DemoPath`] the hand is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathPhase {
    Moving,
    /// Resting on a waypoint, `elapsed` seconds into the dwell.
    Dwelling {
        elapsed: f64,
    },
}

impl DemoPath {
    pub fn new(waypoints: Vec<Vec2>, move_s: f64, dwell_s: f64) -> Self {
        assert!(
            waypoints.len() >= 2,
            "a demo path needs at least two waypoints"
        );
        Self {
            waypoints,
            move_s: move_s.max(0.05),
            dwell_s: dwell_s.max(0.0),
        }
    }

    /// A pleasant loop around an area of `w` × `h`.
    pub fn around(w: f64, h: f64) -> Self {
        let pts = [
            (0.18, 0.30),
            (0.62, 0.20),
            (0.84, 0.58),
            (0.50, 0.80),
            (0.20, 0.66),
        ];
        Self::new(
            pts.iter().map(|&(x, y)| Vec2::new(x * w, y * h)).collect(),
            0.9,
            0.9,
        )
    }

    pub fn period(&self) -> f64 {
        self.waypoints.len() as f64 * self.segment_s()
    }

    /// Length of one dwell plus one move.
    pub fn segment_s(&self) -> f64 {
        self.move_s + self.dwell_s
    }

    pub fn position(&self, t: f64) -> (Vec2, PathPhase) {
        let seg = self.move_s + self.dwell_s;
        let t = t.rem_euclid(self.period());
        let i = (t / seg) as usize % self.waypoints.len();
        let local = t - i as f64 * seg;
        let from = self.waypoints[i];
        let to = self.waypoints[(i + 1) % self.waypoints.len()];
        if local < self.dwell_s {
            (from, PathPhase::Dwelling { elapsed: local })
        } else {
            (
                from.lerp(to, min_jerk((local - self.dwell_s) / self.move_s)),
                PathPhase::Moving,
            )
        }
    }
}

/// A complete simulated session: a shaky hand moves along a [`DemoPath`],
/// sends 125 Hz "hardware" deltas into a real [`Engine`], and clicks at
/// each waypoint. The engine's trace is what the app draws in demo mode.
#[derive(Debug)]
pub struct Demo {
    engine: Engine,
    hand: ShakyHand,
    path: DemoPath,
    t: f64,
    next_input: f64,
    next_tick: f64,
    tick_interval: f64,
    last_actual: Vec2,
    clicked_cycle: Option<u64>,
    pressed_at: Option<f64>,
}

impl Demo {
    const INPUT_INTERVAL: f64 = 1.0 / 125.0;
    const CLICK_AT: f64 = 0.35;
    const CLICK_HOLD: f64 = 0.12;

    pub fn new(settings: &Settings, model: TremorModel, w: f64, h: f64, seed: u64) -> Self {
        let path = DemoPath::around(w, h);
        let start = path.position(0.0).0;
        let mut engine = Engine::new(settings, start);
        engine.set_trace_enabled(true);
        Self {
            engine,
            hand: ShakyHand::new(model, seed),
            path,
            t: 0.0,
            next_input: 0.0,
            next_tick: 0.0,
            tick_interval: settings.tick_interval(),
            last_actual: start,
            clicked_cycle: None,
            pressed_at: None,
        }
    }

    pub fn apply_settings(&mut self, settings: &Settings) {
        self.engine.apply_settings(settings);
        self.tick_interval = settings.tick_interval();
    }

    pub fn set_model(&mut self, model: TremorModel) {
        let seed = self.t.to_bits();
        self.hand = ShakyHand::new(model, seed);
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn time(&self) -> f64 {
        self.t
    }

    /// Simulates `dt` seconds.
    pub fn advance(&mut self, dt: f64) {
        let end = self.t + dt.clamp(0.0, 1.0);
        while self.next_input.min(self.next_tick) <= end {
            if self.next_input <= self.next_tick {
                let t = self.next_input;
                self.input(t);
                self.next_input += Self::INPUT_INTERVAL;
            } else {
                let t = self.next_tick;
                self.engine.tick(t);
                self.next_tick += self.tick_interval;
            }
        }
        self.t = end;
    }

    fn input(&mut self, t: f64) {
        let (intended, phase) = self.path.position(t);
        let actual = intended + self.hand.offset(t);
        self.engine.on_motion(t, actual - self.last_actual);
        self.last_actual = actual;

        let cycle = (t / self.path.segment_s()) as u64;
        if let PathPhase::Dwelling { elapsed } = phase {
            if elapsed >= Self::CLICK_AT && self.clicked_cycle != Some(cycle) {
                self.clicked_cycle = Some(cycle);
                self.pressed_at = Some(t);
                self.engine.on_button(t, Button::Left, true);
            }
        }
        if let Some(p) = self.pressed_at {
            if t - p >= Self::CLICK_HOLD {
                self.pressed_at = None;
                self.engine.on_button(t, Button::Left, false);
            }
        }
    }

    pub fn drain_trace(&mut self) -> Vec<TraceSample> {
        self.engine.drain_trace()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic_and_in_range() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            let x = a.next_f64();
            assert_eq!(x, b.next_f64());
            assert!((0.0..1.0).contains(&x));
        }
        let mean: f64 = (0..20_000).map(|_| a.normal()).sum::<f64>() / 20_000.0;
        assert!(mean.abs() < 0.05);
    }

    #[test]
    fn hand_amplitude_matches_model() {
        let mut hand = ShakyHand::new(TremorModel::new(5.0, 6.0), 1);
        let offsets: Vec<f64> = (0..5000)
            .map(|i| hand.offset(i as f64 / 250.0).length())
            .collect();
        let max = offsets.iter().copied().fold(0.0, f64::max);
        assert!(max > 4.0 && max < 9.0, "max swing {max}");
    }

    #[test]
    fn path_dwells_then_moves() {
        let p = DemoPath::new(vec![Vec2::ZERO, Vec2::new(100.0, 0.0)], 1.0, 0.5);
        assert_eq!(
            p.position(0.2),
            (Vec2::ZERO, PathPhase::Dwelling { elapsed: 0.2 })
        );
        let (mid, phase) = p.position(1.0);
        assert_eq!(phase, PathPhase::Moving);
        assert!((mid.x - 50.0).abs() < 1e-9);
        assert_eq!(p.period(), 3.0);
    }

    #[test]
    fn demo_produces_steadier_output_and_clicks() {
        let mut demo = Demo::new(
            &Settings::default(),
            TremorModel::moderate(),
            800.0,
            500.0,
            9,
        );
        demo.advance(0.5);
        demo.drain_trace();
        let mut trace = Vec::new();
        for _ in 0..240 {
            demo.advance(1.0 / 60.0);
            trace.extend(demo.drain_trace());
        }
        assert!(trace.len() > 900);
        // At rest (late in each dwell) most of the tremor is gone…
        let (mut rest_raw, mut rest_out) = (Vec::new(), Vec::new());
        for s in &trace {
            if let (wp, PathPhase::Dwelling { elapsed }) = demo.path.position(s.t) {
                if elapsed > 0.45 {
                    rest_raw.push(s.raw - wp);
                    rest_out.push(s.out - wp);
                }
            }
        }
        let (r_raw, r_out) = (crate::tremor::rms(&rest_raw), crate::tremor::rms(&rest_out));
        assert!(
            r_out < r_raw * 0.35,
            "at rest: raw {r_raw}, smoothed {r_out}"
        );
        // …and over the whole run the tremor band (4–8 Hz) is well reduced,
        // even though the filter opens up during fast reaches by design.
        let raw: Vec<Vec2> = trace.iter().map(|s| s.raw).collect();
        let out: Vec<Vec2> = trace.iter().map(|s| s.out).collect();
        let band: Vec<f64> = (0..=40).map(|i| 4.0 + i as f64 * 0.1).collect();
        let power = |v: &[Vec2]| {
            let hp = crate::tremor::high_pass(v, 250.0);
            crate::tremor::power_spectrum(&hp, 250.0, &band)
                .iter()
                .sum::<f64>()
                .sqrt()
        };
        let (p_raw, p_out) = (power(&raw), power(&out));
        assert!(
            p_out < p_raw * 0.5,
            "tremor band: raw {p_raw}, smoothed {p_out}"
        );
        assert!(demo.engine().stats().clicks >= 2);
        assert!(trace.iter().any(|s| s.flags & TraceSample::PRESS != 0));
    }
}
