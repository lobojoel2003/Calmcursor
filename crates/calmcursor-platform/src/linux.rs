//! Linux backend: evdev + uinput.
//!
//! Each physical mouse or trackball is grabbed exclusively (`EVIOCGRAB`), so
//! the display server no longer sees it. Calmcursor reads its events, feeds
//! movement through the engine, and replays everything through one virtual
//! pointer created with `uinput`. Buttons and wheels are forwarded at once;
//! movement is emitted by the tick thread as smoothed relative motion.
//!
//! Working below the display server means it behaves the same on X11 and on
//! every Wayland compositor. If Calmcursor exits or crashes, the kernel drops
//! the grabs and the real devices work normally again.
//!
//! Needs read access to `/dev/input/event*` and write access to
//! `/dev/uinput` (see `packaging/linux/70-calmcursor.rules`).

use crate::runner::{spawn_tick_thread, Output, Shared};
use crate::{PermissionStatus, PlatformError};
use calmcursor_core::{Button, ButtonAction, Vec2};
use evdev::uinput::VirtualDevice;
use evdev::{
    AttributeSet, BusType, Device, EventType, InputEvent, InputId, KeyCode, RelativeAxisCode,
    SynchronizationCode,
};
use std::collections::HashMap;
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const NAME: &str = "Linux evdev + uinput";
pub const PERMISSION_URL: Option<&str> =
    Some("https://github.com/lobojoel2003/Calmcursor#linux-permissions");

const VIRTUAL_NAME: &str = "Calmcursor virtual pointer";
const RESCAN_EVERY: Duration = Duration::from_secs(2);
const POLL_MS: i32 = 200;
/// One wheel notch in hi-res units.
const HI_RES_PER_NOTCH: i32 = 120;

const PERMISSION_HELP: &str = "Calmcursor needs to read your mouse (/dev/input) and create a \
virtual pointer (/dev/uinput). The .deb and .rpm packages set this up; otherwise install the \
udev rule from packaging/linux/70-calmcursor.rules:\n\n  sudo cp 70-calmcursor.rules \
/etc/udev/rules.d/\n  sudo modprobe uinput\n  sudo udevadm control --reload-rules && sudo \
udevadm trigger\n\nthen log out and back in.";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The virtual pointer plus the motion already sent through it.
struct VirtualOut {
    dev: VirtualDevice,
    /// Total motion emitted so far, in engine coordinates.
    emitted: Vec2,
}

impl VirtualOut {
    fn create() -> io::Result<Self> {
        let keys: AttributeSet<KeyCode> = [
            KeyCode::BTN_LEFT,
            KeyCode::BTN_RIGHT,
            KeyCode::BTN_MIDDLE,
            KeyCode::BTN_SIDE,
            KeyCode::BTN_EXTRA,
            KeyCode::BTN_FORWARD,
            KeyCode::BTN_BACK,
            KeyCode::BTN_TASK,
        ]
        .into_iter()
        .collect();
        let axes: AttributeSet<RelativeAxisCode> = [
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_HWHEEL,
            RelativeAxisCode::REL_WHEEL_HI_RES,
            RelativeAxisCode::REL_HWHEEL_HI_RES,
        ]
        .into_iter()
        .collect();
        let dev = VirtualDevice::builder()?
            .name(VIRTUAL_NAME)
            .input_id(InputId::new(BusType::BUS_VIRTUAL, 0, 0, 1))
            .with_keys(&keys)?
            .with_relative_axes(&axes)?
            .build()?;
        Ok(Self {
            dev,
            emitted: Vec2::ZERO,
        })
    }

    /// Emits whatever motion is needed to reach `target`, plus `extra` events,
    /// as one batch.
    fn emit_to(&mut self, target: Vec2, extra: &[InputEvent]) -> io::Result<()> {
        let target = target.round();
        let d = target - self.emitted;
        let mut batch = Vec::with_capacity(extra.len() + 2);
        if d.x != 0.0 {
            batch.push(rel(RelativeAxisCode::REL_X, d.x as i32));
        }
        if d.y != 0.0 {
            batch.push(rel(RelativeAxisCode::REL_Y, d.y as i32));
        }
        batch.extend_from_slice(extra);
        if batch.is_empty() {
            return Ok(());
        }
        self.dev.emit(&batch)?;
        self.emitted = target;
        Ok(())
    }

    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
        if events.is_empty() {
            Ok(())
        } else {
            self.dev.emit(events)
        }
    }
}

fn rel(axis: RelativeAxisCode, value: i32) -> InputEvent {
    InputEvent::new(EventType::RELATIVE.0, axis.0, value)
}

struct LinuxOutput(Arc<Mutex<VirtualOut>>);

impl Output for LinuxOutput {
    fn apply(&mut self, pos: Vec2, _held: Option<Button>) -> Result<(), String> {
        lock(&self.0)
            .emit_to(pos, &[])
            .map_err(|e| format!("could not move the pointer: {e}"))
    }
}

/// Is this a mouse-like device Calmcursor can safely grab?
fn classify(dev: &Device) -> Result<(), &'static str> {
    if dev.name() == Some(VIRTUAL_NAME) {
        return Err("own virtual device");
    }
    let rel = dev.supported_relative_axes().ok_or("no relative axes")?;
    if !(rel.contains(RelativeAxisCode::REL_X) && rel.contains(RelativeAxisCode::REL_Y)) {
        return Err("no relative motion");
    }
    let keys = dev.supported_keys().ok_or("no buttons")?;
    if !keys.contains(KeyCode::BTN_LEFT) {
        return Err("no left button");
    }
    // Grabbing a device that also types (some receivers combine both) would
    // swallow keystrokes, so leave those alone.
    if keys.iter().any(|k| k.0 > 0 && k.0 < 0x100) {
        return Err("also sends keyboard keys");
    }
    // Touchpads and tablets report absolute positions; not handled yet.
    if dev
        .supported_absolute_axes()
        .is_some_and(|a| a.iter().next().is_some())
    {
        return Err("absolute device (touchpad or tablet)");
    }
    Ok(())
}

fn button_for(code: KeyCode) -> Option<Button> {
    match code {
        KeyCode::BTN_LEFT => Some(Button::Left),
        KeyCode::BTN_RIGHT => Some(Button::Right),
        KeyCode::BTN_MIDDLE => Some(Button::Middle),
        KeyCode::BTN_SIDE | KeyCode::BTN_BACK => Some(Button::Back),
        KeyCode::BTN_EXTRA | KeyCode::BTN_FORWARD => Some(Button::Forward),
        _ => None,
    }
}

fn wait_readable(fd: i32, timeout_ms: i32) -> bool {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `pfd` is a valid pollfd for the duration of the call.
    let r = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
    r > 0
}

/// Reads one grabbed device until it disappears or the steadier stops.
fn reader_loop(shared: Arc<Shared>, out: Arc<Mutex<VirtualOut>>, mut dev: Device, name: String) {
    let axes = dev.supported_relative_axes();
    let has_hires = axes.is_some_and(|r| r.contains(RelativeAxisCode::REL_WHEEL_HI_RES));
    let has_hhires = axes.is_some_and(|r| r.contains(RelativeAxisCode::REL_HWHEEL_HI_RES));
    if let Err(e) = dev.set_nonblocking(true) {
        log::warn!("{name}: cannot set non-blocking mode: {e}");
        return;
    }
    let fd = dev.as_raw_fd();
    let mut motion = Vec2::ZERO;
    let mut pending: Vec<InputEvent> = Vec::new();

    'outer: while shared.is_running() {
        if !wait_readable(fd, POLL_MS) {
            continue;
        }
        let events = match dev.fetch_events() {
            Ok(events) => events.collect::<Vec<_>>(),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
            Err(e) => {
                log::info!("{name}: stopped reading ({e})");
                break 'outer;
            }
        };
        for ev in events {
            let ty = ev.event_type();
            if ty == EventType::RELATIVE {
                match RelativeAxisCode(ev.code()) {
                    RelativeAxisCode::REL_X => motion.x += f64::from(ev.value()),
                    RelativeAxisCode::REL_Y => motion.y += f64::from(ev.value()),
                    RelativeAxisCode::REL_WHEEL => {
                        pending.push(ev);
                        if !has_hires {
                            pending.push(rel(
                                RelativeAxisCode::REL_WHEEL_HI_RES,
                                ev.value() * HI_RES_PER_NOTCH,
                            ));
                        }
                    }
                    RelativeAxisCode::REL_HWHEEL => {
                        pending.push(ev);
                        if !has_hhires {
                            pending.push(rel(
                                RelativeAxisCode::REL_HWHEEL_HI_RES,
                                ev.value() * HI_RES_PER_NOTCH,
                            ));
                        }
                    }
                    RelativeAxisCode::REL_WHEEL_HI_RES | RelativeAxisCode::REL_HWHEEL_HI_RES => {
                        pending.push(ev);
                    }
                    _ => {}
                }
            } else if ty == EventType::KEY {
                let code = KeyCode(ev.code());
                let action = match (button_for(code), ev.value()) {
                    (Some(b), 0) => shared.input(|e, t| e.on_button(t, b, false)),
                    (Some(b), 1) => shared.input(|e, t| e.on_button(t, b, true)),
                    _ => ButtonAction::Pass,
                };
                if action == ButtonAction::Pass {
                    pending.push(ev);
                }
            } else if ty == EventType::SYNCHRONIZATION
                && ev.code() == SynchronizationCode::SYN_REPORT.0
            {
                let moved = motion != Vec2::ZERO;
                let delta = std::mem::take(&mut motion);
                let result = if moved {
                    let (enabled, raw) = shared.input(|e, t| {
                        e.on_motion(t, delta);
                        (e.is_enabled(), e.raw_position())
                    });
                    if enabled {
                        lock(&out).emit(&pending)
                    } else {
                        // Paused: pass movement straight through, now.
                        lock(&out).emit_to(raw, &pending)
                    }
                } else {
                    lock(&out).emit(&pending)
                };
                pending.clear();
                if let Err(e) = result {
                    shared.set_warning(Some(format!("could not forward input: {e}")));
                }
            }
        }
    }
    let _ = dev.ungrab();
}

struct Readers {
    threads: HashMap<PathBuf, (String, JoinHandle<()>)>,
    skipped: HashMap<PathBuf, String>,
}

impl Readers {
    fn reap(&mut self) {
        self.threads.retain(|_, (_, h)| !h.is_finished());
    }

    fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.threads.values().map(|(n, _)| n.clone()).collect();
        v.sort();
        v
    }
}

/// Grabs every eligible device not already handled. Returns how many were added.
fn scan(shared: &Arc<Shared>, out: &Arc<Mutex<VirtualOut>>, readers: &mut Readers) -> usize {
    readers.reap();
    let mut added = 0;
    for (path, mut dev) in evdev::enumerate() {
        if readers.threads.contains_key(&path) || readers.skipped.contains_key(&path) {
            continue;
        }
        let name = dev.name().unwrap_or("unnamed device").to_string();
        if let Err(why) = classify(&dev) {
            if why != "own virtual device" {
                log::debug!("skipping {name} ({}): {why}", path.display());
            }
            readers.skipped.insert(path, why.to_string());
            continue;
        }
        if let Err(e) = dev.grab() {
            log::warn!("cannot grab {name} ({}): {e}", path.display());
            continue;
        }
        log::info!("steadying {name} ({})", path.display());
        let (s, o, n) = (shared.clone(), out.clone(), name.clone());
        let handle = std::thread::Builder::new()
            .name(format!("calmcursor-evdev-{}", path.display()))
            .spawn(move || reader_loop(s, o, dev, n))
            .expect("failed to spawn a reader thread");
        readers.threads.insert(path, (name, handle));
        added += 1;
    }
    // Forget skipped paths that vanished, so a replugged device is checked again.
    readers.skipped.retain(|p, _| p.exists());
    shared.set_devices(readers.names());
    added
}

fn input_dir_status() -> Result<(), PlatformError> {
    let entries = std::fs::read_dir("/dev/input").map_err(|e| PlatformError::NoDevices {
        help: format!("/dev/input is not available ({e})."),
    })?;
    let mut saw_event_node = false;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("event") {
            continue;
        }
        saw_event_node = true;
        match std::fs::File::open(entry.path()) {
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => continue,
            Err(_) => continue,
        }
    }
    if saw_event_node {
        Err(PlatformError::PermissionDenied {
            message: "cannot read input devices in /dev/input".into(),
            help: PERMISSION_HELP.into(),
        })
    } else {
        Err(no_devices())
    }
}

fn no_devices() -> PlatformError {
    PlatformError::NoDevices {
        help: "Plug in a mouse or trackball. On Linux, Calmcursor steadies mice and trackballs; \
               touchpads and drawing tablets are not supported yet."
            .into(),
    }
}

fn uinput_error(e: io::Error) -> PlatformError {
    match e.kind() {
        io::ErrorKind::PermissionDenied => PlatformError::PermissionDenied {
            message: "cannot create a virtual pointer (/dev/uinput)".into(),
            help: PERMISSION_HELP.into(),
        },
        io::ErrorKind::NotFound => PlatformError::PermissionDenied {
            message: "/dev/uinput does not exist (the uinput kernel module is not loaded)".into(),
            help: PERMISSION_HELP.into(),
        },
        _ => PlatformError::os(format!("cannot create a virtual pointer: {e}")),
    }
}

#[derive(Debug)]
pub struct Backend {
    tick: JoinHandle<()>,
    scanner: JoinHandle<()>,
}

impl Backend {
    pub fn start(shared: Arc<Shared>) -> Result<Self, PlatformError> {
        input_dir_status()?;
        let eligible = evdev::enumerate()
            .filter(|(_, d)| classify(d).is_ok())
            .count();
        if eligible == 0 {
            return Err(no_devices());
        }
        // Create the virtual pointer before grabbing anything, so a failure
        // here can never leave the real mouse grabbed and dead.
        let out = Arc::new(Mutex::new(VirtualOut::create().map_err(uinput_error)?));

        let mut readers = Readers {
            threads: HashMap::new(),
            skipped: HashMap::new(),
        };
        if scan(&shared, &out, &mut readers) == 0 {
            return Err(PlatformError::PermissionDenied {
                message: "found a mouse but could not take it over".into(),
                help: PERMISSION_HELP.into(),
            });
        }

        let tick = spawn_tick_thread(shared.clone(), LinuxOutput(out.clone()));
        let scanner = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("calmcursor-hotplug".into())
                .spawn(move || {
                    let mut last = Instant::now();
                    while shared.is_running() {
                        std::thread::sleep(Duration::from_millis(100));
                        if last.elapsed() >= RESCAN_EVERY {
                            scan(&shared, &out, &mut readers);
                            last = Instant::now();
                        }
                    }
                    for (_, (_, h)) in readers.threads.drain() {
                        let _ = h.join();
                    }
                })
                .expect("failed to spawn the hot-plug thread")
        };
        Ok(Self { tick, scanner })
    }

    pub fn stop(self) {
        let _ = self.scanner.join();
        let _ = self.tick.join();
    }
}

fn writable(path: &Path) -> bool {
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a valid NUL-terminated path.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

pub fn permission_status() -> PermissionStatus {
    let uinput = Path::new("/dev/uinput");
    if !uinput.exists() {
        return PermissionStatus::Needed {
            message: "/dev/uinput does not exist (the uinput kernel module is not loaded)".into(),
            help: PERMISSION_HELP.into(),
        };
    }
    if !writable(uinput) {
        return PermissionStatus::Needed {
            message: "no write access to /dev/uinput".into(),
            help: PERMISSION_HELP.into(),
        };
    }
    match input_dir_status() {
        Ok(()) => PermissionStatus::Granted,
        Err(PlatformError::PermissionDenied { message, help }) => {
            PermissionStatus::Needed { message, help }
        }
        Err(_) => PermissionStatus::Unknown,
    }
}

pub fn request_permission() -> bool {
    matches!(permission_status(), PermissionStatus::Granted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_map_to_engine_buttons() {
        assert_eq!(button_for(KeyCode::BTN_LEFT), Some(Button::Left));
        assert_eq!(button_for(KeyCode::BTN_SIDE), Some(Button::Back));
        assert_eq!(button_for(KeyCode::BTN_EXTRA), Some(Button::Forward));
        assert_eq!(button_for(KeyCode::KEY_A), None);
    }

    #[test]
    fn permission_status_does_not_panic() {
        let _ = permission_status();
    }
}
