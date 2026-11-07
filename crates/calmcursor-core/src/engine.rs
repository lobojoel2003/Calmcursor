//! The real-time steadying pipeline.
//!
//! The OS hooks (in `calmcursor-platform`) drive an [`Engine`] like this:
//!
//! 1. Every hardware move is swallowed and reported with [`Engine::on_motion`]
//!    as a delta. The engine keeps the "raw" pointer: where the cursor would
//!    be without Calmcursor.
//! 2. A timer calls [`Engine::tick`] at a fixed rate (250 Hz by default). Each
//!    tick runs the raw position through the One Euro filter and returns the
//!    new smoothed position, which the hook applies to the real cursor.
//!    Ticking on a timer, instead of only on input, lets the cursor settle
//!    smoothly after the hand stops.
//! 3. Button presses go through [`Engine::on_button`], which can hold the
//!    pointer still while clicking ("click lock") and drop accidental
//!    double presses ("bounce filter").
//!
//! All times are seconds from an arbitrary monotonic origin.

use crate::filter::{OneEuro2D, OneEuroParams};
use crate::geometry::{Rect, Vec2};
use crate::settings::{ClickSettings, Settings};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Ticks further apart than this mean the engine was asleep; the filter is
/// re-seeded so the first step after a pause is smoothed like any other.
const RESUME_GAP: f64 = 0.1;
/// The output snaps to the raw position once it is this close (units).
const SETTLE_DISTANCE: f64 = 0.02;
/// The engine counts as idle this long (s) after the last input.
const IDLE_AFTER: f64 = 0.05;
/// Maximum trace samples kept between drains.
const TRACE_CAPACITY: usize = 4096;

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl Button {
    pub const ALL: [Button; 5] = [
        Button::Left,
        Button::Right,
        Button::Middle,
        Button::Back,
        Button::Forward,
    ];

    fn index(self) -> usize {
        self as usize
    }
}

/// What the hook should do with a button event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonAction {
    /// Deliver the event as normal.
    Pass,
    /// Drop the event (it was an accidental bounce).
    Suppress,
}

/// One point of the live trace: the raw pointer next to the smoothed one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceSample {
    pub t: f64,
    pub raw: Vec2,
    pub out: Vec2,
    /// Bit set of the `TraceSample::*` flags.
    pub flags: u8,
}

impl TraceSample {
    /// A button was pressed at this sample.
    pub const PRESS: u8 = 1;
    /// The pointer is held by click lock.
    pub const ANCHORED: u8 = 2;
    /// An accidental press was dropped.
    pub const SUPPRESSED: u8 = 4;
    /// Steadying is off; raw and output are the same.
    pub const BYPASS: u8 = 8;
}

/// Running counters, shown in the app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStats {
    pub motion_events: u64,
    pub ticks: u64,
    /// Presses delivered.
    pub clicks: u64,
    /// Presses where click lock held the pointer.
    pub steadied_clicks: u64,
    /// Accidental presses dropped by the bounce filter.
    pub suppressed_presses: u64,
}

#[derive(Clone, Copy, Debug)]
struct Anchor {
    since: f64,
    raw_at_press: Vec2,
    pos: Vec2,
}

#[derive(Clone, Copy, Debug, Default)]
struct ButtonState {
    pressed: bool,
    suppressed: bool,
    last_release: Option<f64>,
}

/// The steadying state machine. See the module docs.
#[derive(Debug)]
pub struct Engine {
    enabled: bool,
    filter: OneEuro2D,
    click: ClickSettings,
    tick_interval: f64,
    raw: Vec2,
    output: Vec2,
    bounds: Option<Rect>,
    anchor: Option<Anchor>,
    buttons: [ButtonState; 5],
    last_tick: Option<f64>,
    last_activity: f64,
    trace: Option<VecDeque<TraceSample>>,
    pending_flags: u8,
    stats: EngineStats,
}

impl Engine {
    /// Creates an engine with the pointer at `start`.
    pub fn new(settings: &Settings, start: Vec2) -> Self {
        let settings = settings.clone().sanitized();
        Self {
            enabled: settings.enabled,
            filter: OneEuro2D::new(settings.filter.params()),
            click: settings.click,
            tick_interval: settings.tick_interval(),
            raw: start,
            output: start,
            bounds: None,
            anchor: None,
            buttons: [ButtonState::default(); 5],
            last_tick: None,
            last_activity: f64::NEG_INFINITY,
            trace: None,
            pending_flags: 0,
            stats: EngineStats::default(),
        }
    }

    /// Applies new settings without disturbing the pointer.
    pub fn apply_settings(&mut self, settings: &Settings) {
        let settings = settings.clone().sanitized();
        self.filter.set_params(settings.filter.params());
        self.click = settings.click;
        if !self.click.anchor_enabled {
            self.anchor = None;
        }
        self.tick_interval = settings.tick_interval();
        self.set_enabled(settings.enabled);
    }

    /// Turns steadying on or off. The pointer stays where it is.
    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled == self.enabled {
            return;
        }
        self.enabled = enabled;
        self.raw = self.output;
        self.anchor = None;
        self.filter.reset();
        for b in &mut self.buttons {
            b.suppressed = false;
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn params(&self) -> OneEuroParams {
        self.filter.params()
    }

    /// Limits the pointer to `bounds` (the desktop), or removes the limit.
    pub fn set_bounds(&mut self, bounds: Option<Rect>) {
        self.bounds = bounds;
        if let Some(b) = bounds {
            self.raw = b.clamp(self.raw);
            self.output = b.clamp(self.output);
        }
    }

    pub fn bounds(&self) -> Option<Rect> {
        self.bounds
    }

    /// The smoothed pointer position.
    pub fn position(&self) -> Vec2 {
        self.output
    }

    /// Where the pointer would be without steadying.
    pub fn raw_position(&self) -> Vec2 {
        self.raw
    }

    /// True while click lock is holding the pointer.
    pub fn is_anchored(&self) -> bool {
        self.anchor.is_some()
    }

    /// The first button currently held down (and not suppressed).
    pub fn held_button(&self) -> Option<Button> {
        Button::ALL
            .into_iter()
            .find(|b| self.buttons[b.index()].pressed && !self.buttons[b.index()].suppressed)
    }

    pub fn stats(&self) -> EngineStats {
        self.stats
    }

    /// A hardware movement of `delta` at time `t`.
    pub fn on_motion(&mut self, t: f64, delta: Vec2) {
        if !delta.is_finite() {
            return;
        }
        self.raw += delta;
        if let Some(b) = self.bounds {
            self.raw = b.clamp(self.raw);
        }
        self.last_activity = t;
        self.stats.motion_events += 1;
        if !self.enabled {
            self.output = self.raw;
            self.record(t, TraceSample::BYPASS);
        }
    }

    /// The pointer was moved by something else (another app, a warp, or a
    /// pass-through while paused). Re-centres everything on `pos`.
    pub fn sync(&mut self, t: f64, pos: Vec2) {
        if !pos.is_finite() {
            return;
        }
        let pos = self.bounds.map_or(pos, |b| b.clamp(pos));
        self.raw = pos;
        self.output = pos;
        self.anchor = None;
        self.filter.reset();
        self.last_activity = t;
        let flags = if self.enabled { 0 } else { TraceSample::BYPASS };
        self.record(t, flags);
    }

    /// A button changed state. Returns whether to deliver or drop the event.
    pub fn on_button(&mut self, t: f64, button: Button, pressed: bool) -> ButtonAction {
        let debounce = f64::from(self.click.debounce_ms) / 1000.0;
        let enabled = self.enabled;
        let state = &mut self.buttons[button.index()];

        if !enabled {
            state.pressed = pressed;
            state.suppressed = false;
            if !pressed {
                state.last_release = Some(t);
            }
            return ButtonAction::Pass;
        }

        if pressed {
            if state.pressed {
                // Repeated press without a release: nothing new to decide.
                return if state.suppressed {
                    ButtonAction::Suppress
                } else {
                    ButtonAction::Pass
                };
            }
            state.pressed = true;
            let bounce = debounce > 0.0 && state.last_release.is_some_and(|r| t - r < debounce);
            if bounce {
                state.suppressed = true;
                self.stats.suppressed_presses += 1;
                self.pending_flags |= TraceSample::SUPPRESSED;
                return ButtonAction::Suppress;
            }
            state.suppressed = false;
            self.stats.clicks += 1;
            self.pending_flags |= TraceSample::PRESS;
            self.last_activity = t;
            if self.click.anchor_enabled {
                self.anchor = Some(Anchor {
                    since: t,
                    raw_at_press: self.raw,
                    pos: self.output,
                });
                self.stats.steadied_clicks += 1;
            }
            ButtonAction::Pass
        } else {
            if !state.pressed {
                return ButtonAction::Pass;
            }
            state.pressed = false;
            state.last_release = Some(t);
            self.last_activity = t;
            if std::mem::take(&mut state.suppressed) {
                ButtonAction::Suppress
            } else {
                ButtonAction::Pass
            }
        }
    }

    /// Advances the filter to time `t`. Returns the new smoothed position if
    /// it changed.
    pub fn tick(&mut self, t: f64) -> Option<Vec2> {
        self.stats.ticks += 1;
        let resumed = self.last_tick.is_none_or(|last| t - last > RESUME_GAP);
        self.last_tick = Some(t);
        if !self.enabled {
            return None;
        }
        if resumed {
            self.filter.reset_to(t - self.tick_interval, self.output);
        }

        if let Some(anchor) = self.anchor {
            let drifted = self.raw.distance(anchor.raw_at_press) > self.click.anchor_radius_px;
            let expired = (t - anchor.since) * 1000.0 >= f64::from(self.click.anchor_ms);
            if drifted || expired {
                self.anchor = None;
                self.filter.reset_to(t - self.tick_interval, anchor.pos);
            } else {
                self.record(t, TraceSample::ANCHORED);
                return None;
            }
        }

        let mut next = self.filter.filter(t, self.raw);
        if let Some(b) = self.bounds {
            next = b.clamp(next);
        }
        if next.distance(self.raw) < SETTLE_DISTANCE && self.filter.speed() < 1.0 {
            next = self.raw;
            self.filter.reset_to(t, next);
        }
        let moved = next != self.output;
        self.output = next;
        self.record(t, 0);
        moved.then_some(next)
    }

    /// True when ticking would change nothing until new input arrives, so
    /// the caller may sleep.
    pub fn is_idle(&self, t: f64) -> bool {
        !self.enabled
            || (self.anchor.is_none()
                && self.output == self.raw
                && t - self.last_activity > IDLE_AFTER)
    }

    /// Starts or stops recording the live trace.
    pub fn set_trace_enabled(&mut self, on: bool) {
        match (on, self.trace.is_some()) {
            (true, false) => self.trace = Some(VecDeque::with_capacity(512)),
            (false, true) => self.trace = None,
            _ => {}
        }
    }

    pub fn trace_enabled(&self) -> bool {
        self.trace.is_some()
    }

    /// Returns and clears the recorded trace samples.
    pub fn drain_trace(&mut self) -> Vec<TraceSample> {
        self.trace
            .as_mut()
            .map(|q| q.drain(..).collect())
            .unwrap_or_default()
    }

    fn record(&mut self, t: f64, flags: u8) {
        if let Some(q) = self.trace.as_mut() {
            if q.len() >= TRACE_CAPACITY {
                q.pop_front();
            }
            q.push_back(TraceSample {
                t,
                raw: self.raw,
                out: self.output,
                flags: flags | std::mem::take(&mut self.pending_flags),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const DT: f64 = 1.0 / 250.0;

    fn engine() -> Engine {
        Engine::new(&Settings::default(), Vec2::new(500.0, 500.0))
    }

    fn run_ticks(e: &mut Engine, t0: f64, n: usize) -> f64 {
        let mut t = t0;
        for _ in 0..n {
            t += DT;
            e.tick(t);
        }
        t
    }

    #[test]
    fn disabled_engine_passes_motion_through() {
        let s = Settings {
            enabled: false,
            ..Settings::default()
        };
        let mut e = Engine::new(&s, Vec2::ZERO);
        e.on_motion(0.0, Vec2::new(5.0, -2.0));
        assert_eq!(e.position(), Vec2::new(5.0, -2.0));
        assert_eq!(e.tick(0.004), None);
        assert!(e.is_idle(0.0));
    }

    #[test]
    fn smooths_then_settles_exactly() {
        let mut e = engine();
        e.on_motion(0.0, Vec2::new(40.0, 0.0));
        let first = e.tick(DT).expect("moves");
        assert!(
            first.x > 500.0 && first.x < 520.0,
            "smoothed, not jumped: {first:?}"
        );
        let t = run_ticks(&mut e, DT, 1500);
        assert_eq!(e.position(), Vec2::new(540.0, 500.0));
        assert!(e.is_idle(t));
        assert_eq!(e.tick(t + DT), None);
    }

    #[test]
    fn resuming_after_idle_is_still_smooth() {
        let mut e = engine();
        e.tick(0.0);
        // Long sleep, then a move: must not jump straight to the target.
        e.on_motion(10.0, Vec2::new(50.0, 0.0));
        let p = e.tick(10.0 + DT).unwrap();
        assert!(p.x < 515.0, "jumped after idle: {p:?}");
    }

    #[test]
    fn removes_most_of_a_tremor() {
        // 5 Hz, ±6 px tremor delivered as 125 Hz hardware deltas.
        let mut e = engine();
        let mut last = Vec2::ZERO;
        let mut peak: f64 = 0.0;
        let mut t = 0.0;
        for i in 0..2500 {
            t = i as f64 * DT;
            if i % 2 == 0 {
                let pos = Vec2::new(6.0 * (TAU * 5.0 * t).sin(), 4.0 * (TAU * 5.0 * t).cos());
                e.on_motion(t, pos - last);
                last = pos;
            }
            e.tick(t);
            if t > 3.0 {
                peak = peak.max(e.position().distance(Vec2::new(500.0, 500.0)));
            }
        }
        assert!(t > 9.0);
        assert!(peak < 2.0, "residual tremor {peak} px");
    }

    #[test]
    fn click_lock_holds_pointer_during_press() {
        let mut e = engine();
        let t = run_ticks(&mut e, 0.0, 10);
        assert_eq!(e.on_button(t, Button::Left, true), ButtonAction::Pass);
        assert!(e.is_anchored());
        let held = e.position();
        // Shaky press: small movement inside the radius.
        e.on_motion(t + 0.01, Vec2::new(4.0, 3.0));
        e.tick(t + 0.02);
        assert_eq!(e.position(), held);
        assert_eq!(
            e.on_button(t + 0.1, Button::Left, false),
            ButtonAction::Pass
        );
        e.tick(t + 0.2);
        assert_eq!(e.position(), held, "still held until anchor_ms");
        // After 250 ms the pointer glides towards the hand again.
        e.tick(t + 0.26);
        assert!(!e.is_anchored());
        assert_ne!(e.position(), held);
        assert_eq!(e.stats().steadied_clicks, 1);
    }

    #[test]
    fn dragging_beyond_radius_releases_click_lock() {
        let mut e = engine();
        let t = run_ticks(&mut e, 0.0, 10);
        e.on_button(t, Button::Left, true);
        e.on_motion(t + 0.01, Vec2::new(30.0, 0.0));
        e.tick(t + 0.012);
        assert!(!e.is_anchored());
        assert_eq!(e.held_button(), Some(Button::Left));
        assert!(e.position().x > 500.0);
    }

    #[test]
    fn click_lock_can_be_disabled() {
        let mut s = Settings::default();
        s.click.anchor_enabled = false;
        let mut e = Engine::new(&s, Vec2::ZERO);
        e.on_button(0.0, Button::Left, true);
        assert!(!e.is_anchored());
    }

    #[test]
    fn bounce_filter_drops_fast_second_press() {
        let mut s = Settings::default();
        s.click.debounce_ms = 80;
        let mut e = Engine::new(&s, Vec2::ZERO);
        assert_eq!(e.on_button(0.0, Button::Left, true), ButtonAction::Pass);
        assert_eq!(e.on_button(0.1, Button::Left, false), ButtonAction::Pass);
        // Bounce 30 ms later: both down and its up are dropped.
        assert_eq!(
            e.on_button(0.13, Button::Left, true),
            ButtonAction::Suppress
        );
        assert_eq!(e.held_button(), None);
        assert_eq!(
            e.on_button(0.15, Button::Left, false),
            ButtonAction::Suppress
        );
        // A deliberate press later is delivered.
        assert_eq!(e.on_button(0.4, Button::Left, true), ButtonAction::Pass);
        // Other buttons are independent.
        assert_eq!(e.on_button(0.41, Button::Right, true), ButtonAction::Pass);
        assert_eq!(e.stats().suppressed_presses, 1);
        assert_eq!(e.stats().clicks, 3);
    }

    #[test]
    fn bounds_clamp_raw_and_output() {
        let mut e = engine();
        e.set_bounds(Some(Rect::from_origin_size(0.0, 0.0, 1000.0, 800.0)));
        e.on_motion(0.0, Vec2::new(5000.0, 5000.0));
        assert_eq!(e.raw_position(), Vec2::new(1000.0, 800.0));
        run_ticks(&mut e, 0.0, 2000);
        assert_eq!(e.position(), Vec2::new(1000.0, 800.0));
        // Moving back responds at once (no dead zone past the edge).
        e.on_motion(9.0, Vec2::new(-10.0, 0.0));
        assert_eq!(e.raw_position(), Vec2::new(990.0, 800.0));
    }

    #[test]
    fn sync_recentres_everything() {
        let mut e = engine();
        e.on_motion(0.0, Vec2::new(100.0, 0.0));
        e.tick(DT);
        e.sync(0.01, Vec2::new(10.0, 10.0));
        assert_eq!(e.position(), Vec2::new(10.0, 10.0));
        assert_eq!(e.raw_position(), Vec2::new(10.0, 10.0));
    }

    #[test]
    fn toggling_keeps_pointer_in_place() {
        let mut e = engine();
        e.on_motion(0.0, Vec2::new(100.0, 0.0));
        e.tick(DT);
        let p = e.position();
        e.set_enabled(false);
        assert_eq!(e.position(), p);
        assert_eq!(e.raw_position(), p);
        e.set_enabled(true);
        assert_eq!(e.position(), p);
    }

    #[test]
    fn trace_records_raw_and_output_with_flags() {
        let mut e = engine();
        assert!(e.drain_trace().is_empty());
        e.set_trace_enabled(true);
        e.on_motion(0.0, Vec2::new(10.0, 0.0));
        e.tick(DT);
        e.on_button(0.005, Button::Left, true);
        e.tick(2.0 * DT);
        let trace = e.drain_trace();
        assert_eq!(trace.len(), 2);
        assert_eq!(trace[0].raw, Vec2::new(510.0, 500.0));
        assert!(trace[0].out.x < 510.0);
        assert_eq!(trace[1].flags & TraceSample::PRESS, TraceSample::PRESS);
        assert_eq!(
            trace[1].flags & TraceSample::ANCHORED,
            TraceSample::ANCHORED
        );
        assert!(e.drain_trace().is_empty());
        e.set_trace_enabled(false);
        e.tick(3.0 * DT);
        assert!(e.drain_trace().is_empty());
    }

    #[test]
    fn trace_is_bounded() {
        let mut e = engine();
        e.set_trace_enabled(true);
        for i in 0..(TRACE_CAPACITY + 100) {
            e.sync(i as f64, Vec2::ZERO);
        }
        assert_eq!(e.drain_trace().len(), TRACE_CAPACITY);
    }

    #[test]
    fn non_finite_input_is_ignored() {
        let mut e = engine();
        e.on_motion(0.0, Vec2::new(f64::NAN, 1.0));
        e.sync(0.0, Vec2::new(f64::INFINITY, 0.0));
        assert_eq!(e.raw_position(), Vec2::new(500.0, 500.0));
    }
}
