//! State shared between the OS hook threads and the tick thread.

use calmcursor_core::{Button, Engine, Rect, Settings, Vec2};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long the tick thread sleeps when nothing is happening.
const IDLE_WAIT: Duration = Duration::from_millis(250);
/// How often desktop bounds are refreshed (monitors can be plugged in).
const BOUNDS_REFRESH: Duration = Duration::from_secs(2);

struct State {
    engine: Engine,
    input_pending: bool,
}

/// The engine plus everything the threads need to cooperate.
pub struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    running: AtomicBool,
    epoch: Instant,
    tick_interval_us: AtomicU64,
    warning: Mutex<Option<String>>,
    devices: Mutex<Vec<String>>,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("running", &self.is_running())
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic elsewhere must never leave the pointer stuck: recover the data.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    pub(crate) fn new(settings: &Settings) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                engine: Engine::new(settings, Vec2::ZERO),
                input_pending: false,
            }),
            wake: Condvar::new(),
            running: AtomicBool::new(true),
            epoch: Instant::now(),
            tick_interval_us: AtomicU64::new(interval_us(settings)),
            warning: Mutex::new(None),
            devices: Mutex::new(Vec::new()),
        })
    }

    /// Seconds since the steadier started (monotonic).
    pub fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    /// Runs `f` with the engine locked.
    pub fn with_engine<R>(&self, f: impl FnOnce(&mut Engine) -> R) -> R {
        f(&mut lock(&self.state).engine)
    }

    /// Feeds input to the engine and wakes the tick thread so the pointer
    /// updates right away.
    pub(crate) fn input<R>(&self, f: impl FnOnce(&mut Engine, f64) -> R) -> R {
        let mut s = lock(&self.state);
        let r = f(&mut s.engine, self.now());
        s.input_pending = true;
        drop(s);
        self.wake.notify_one();
        r
    }

    pub(crate) fn apply_settings(&self, settings: &Settings) {
        self.tick_interval_us
            .store(interval_us(settings), Ordering::Relaxed);
        self.input(|e, _| e.apply_settings(settings));
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.input(|e, _| e.set_enabled(enabled));
    }

    pub(crate) fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    pub(crate) fn stop(&self) {
        self.running.store(false, Ordering::Release);
        self.wake.notify_all();
    }

    pub(crate) fn set_warning(&self, warning: Option<String>) {
        let mut w = lock(&self.warning);
        if *w != warning {
            if let Some(msg) = &warning {
                log::warn!("{msg}");
            }
            *w = warning;
        }
    }

    pub(crate) fn warning(&self) -> Option<String> {
        lock(&self.warning).clone()
    }

    pub(crate) fn set_devices(&self, devices: Vec<String>) {
        *lock(&self.devices) = devices;
    }

    pub(crate) fn devices(&self) -> Vec<String> {
        lock(&self.devices).clone()
    }

    fn tick_interval(&self) -> Duration {
        Duration::from_micros(self.tick_interval_us.load(Ordering::Relaxed))
    }
}

fn interval_us(settings: &Settings) -> u64 {
    (settings.tick_interval() * 1_000_000.0).round() as u64
}

/// How a backend moves the visible pointer.
pub(crate) trait Output: Send + 'static {
    /// Called once on the tick thread before the first tick.
    fn on_thread_start(&mut self) {}

    /// Moves the pointer to `pos` (engine coordinates). `held` is the button
    /// held down, if any, so drags can be reported as drags.
    fn apply(&mut self, pos: Vec2, held: Option<Button>) -> Result<(), String>;

    /// The desktop's bounds in engine coordinates, if the backend knows them.
    fn bounds(&mut self) -> Option<Rect> {
        None
    }
}

/// Starts the thread that runs the filter and applies its output.
pub(crate) fn spawn_tick_thread<O: Output>(shared: Arc<Shared>, out: O) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("calmcursor-tick".into())
        .spawn(move || tick_loop(&shared, out))
        .expect("failed to spawn the tick thread")
}

fn tick_loop<O: Output>(shared: &Shared, mut out: O) {
    out.on_thread_start();
    let mut last_bounds = None::<Instant>;
    let mut guard = lock(&shared.state);
    while shared.is_running() {
        if last_bounds.is_none_or(|t| t.elapsed() >= BOUNDS_REFRESH) {
            drop(guard);
            let bounds = out.bounds();
            guard = lock(&shared.state);
            if bounds.is_some() && guard.engine.bounds() != bounds {
                guard.engine.set_bounds(bounds);
            }
            last_bounds = Some(Instant::now());
        }

        guard.input_pending = false;
        let moved = guard.engine.tick(shared.now());
        if let Some(pos) = moved {
            let held = guard.engine.held_button();
            drop(guard);
            match out.apply(pos, held) {
                Ok(()) => {}
                Err(e) => shared.set_warning(Some(e)),
            }
            guard = lock(&shared.state);
        }
        if guard.input_pending {
            continue;
        }
        let timeout = if guard.engine.is_idle(shared.now()) {
            IDLE_WAIT
        } else {
            shared.tick_interval()
        };
        guard = shared
            .wake
            .wait_timeout(guard, timeout)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    struct Recorder(mpsc::Sender<Vec2>);

    impl Output for Recorder {
        fn apply(&mut self, pos: Vec2, _held: Option<Button>) -> Result<(), String> {
            self.0.send(pos).map_err(|e| e.to_string())
        }
    }

    #[test]
    fn tick_thread_smooths_and_settles() {
        let shared = Shared::new(&Settings::default());
        let (tx, rx) = mpsc::channel();
        let handle = spawn_tick_thread(shared.clone(), Recorder(tx));
        shared.input(|e, t| e.on_motion(t, Vec2::new(30.0, 0.0)));
        let first = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("pointer moved");
        assert!(
            first.x > 0.0 && first.x < 30.0,
            "first step is smoothed: {first:?}"
        );
        let mut last = first;
        while let Ok(p) = rx.recv_timeout(Duration::from_millis(500)) {
            last = p;
        }
        assert_eq!(last, Vec2::new(30.0, 0.0), "settles exactly on the target");
        shared.stop();
        handle.join().unwrap();
    }

    #[test]
    fn settings_and_toggle_are_applied() {
        let shared = Shared::new(&Settings::default());
        let s = Settings {
            tick_hz: 500,
            ..Settings::default()
        };
        shared.apply_settings(&s);
        assert_eq!(shared.tick_interval(), Duration::from_micros(2000));
        shared.set_enabled(false);
        assert!(!shared.with_engine(|e| e.is_enabled()));
        shared.set_warning(Some("x".into()));
        assert_eq!(shared.warning().as_deref(), Some("x"));
    }
}
