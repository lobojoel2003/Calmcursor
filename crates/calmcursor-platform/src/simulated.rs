//! A backend fed by a simulated shaky hand instead of real hardware.
//!
//! It runs the real engine and tick thread, but input comes from
//! [`calmcursor_core::simulate`] and output goes nowhere, so the real pointer
//! is never touched. Select it with `CALMCURSOR_BACKEND=simulated`.

use crate::runner::{spawn_tick_thread, Output, Shared};
use calmcursor_core::simulate::{DemoPath, PathPhase, ShakyHand, TremorModel};
use calmcursor_core::{Button, Vec2};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

pub const NAME: &str = "Simulated input (demo, real pointer untouched)";

const INPUT_INTERVAL: Duration = Duration::from_millis(8);
const CLICK_AT: f64 = 0.35;
const CLICK_HOLD: f64 = 0.12;

struct NullOutput;

impl Output for NullOutput {
    fn apply(&mut self, _pos: Vec2, _held: Option<Button>) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct Backend {
    input: JoinHandle<()>,
    tick: JoinHandle<()>,
}

impl Backend {
    pub fn start(shared: Arc<Shared>, model: TremorModel) -> Self {
        // A small area, like moving between nearby buttons, so the shake
        // is easy to see in the live trace.
        let path = DemoPath::around(330.0, 205.0);
        let start = path.position(0.0).0;
        shared.with_engine(|e| e.sync(0.0, start));
        shared.set_devices(vec!["Simulated shaky hand".into()]);
        let tick = spawn_tick_thread(shared.clone(), NullOutput);
        let input = std::thread::Builder::new()
            .name("calmcursor-simulated-hand".into())
            .spawn(move || {
                let mut hand = ShakyHand::new(model, 7);
                let mut last = start;
                let mut clicked = None;
                let mut pressed_at = None;
                while shared.is_running() {
                    let t = shared.now();
                    let (intended, phase) = path.position(t);
                    let actual = intended + hand.offset(t);
                    shared.input(|e, now| e.on_motion(now, actual - last));
                    last = actual;
                    let cycle = (t / path.segment_s()) as u64;
                    if let PathPhase::Dwelling { elapsed } = phase {
                        if elapsed >= CLICK_AT && clicked != Some(cycle) {
                            clicked = Some(cycle);
                            pressed_at = Some(t);
                            shared.input(|e, now| e.on_button(now, Button::Left, true));
                        }
                    }
                    if pressed_at.is_some_and(|p| t - p >= CLICK_HOLD) {
                        pressed_at = None;
                        shared.input(|e, now| e.on_button(now, Button::Left, false));
                    }
                    std::thread::sleep(INPUT_INTERVAL);
                }
            })
            .expect("failed to spawn the simulated hand");
        Self { input, tick }
    }

    pub fn stop(self) {
        let _ = self.input.join();
        let _ = self.tick.join();
    }
}

#[cfg(test)]
mod tests {
    use crate::Steadier;
    use calmcursor_core::simulate::TremorModel;
    use calmcursor_core::Settings;
    use std::time::Duration;

    #[test]
    fn simulated_steadier_produces_a_trace() {
        let s = Steadier::start_simulated(&Settings::default(), TremorModel::moderate());
        s.set_trace(true);
        std::thread::sleep(Duration::from_millis(600));
        let trace = s.drain_trace();
        assert!(trace.len() > 50, "only {} samples", trace.len());
        let status = s.status();
        assert!(status.running);
        assert!(status.stats.motion_events > 30);
        assert_eq!(status.backend, super::NAME);
        s.stop();
    }
}
