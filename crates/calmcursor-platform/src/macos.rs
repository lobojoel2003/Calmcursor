//! macOS backend: a Quartz event tap.
//!
//! A tap at the HID level sees every mouse event before any app. For a
//! hardware move it feeds the event's delta to the engine, runs one filter
//! step at once, and rewrites the event's location (and deltas) to the
//! smoothed position. Between hardware events the tick thread posts tagged
//! synthetic move (or drag) events so the pointer settles smoothly after the
//! hand stops. Button events are moved to the smoothed position too, so a
//! click always lands where the pointer is drawn.
//!
//! Needs Accessibility permission (System Settings → Privacy & Security →
//! Accessibility).

use crate::runner::{spawn_tick_thread, Output, Shared};
use crate::{PermissionStatus, PlatformError};
use calmcursor_core::{Button, ButtonAction, Rect, Vec2};
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const NAME: &str = "macOS Quartz event tap";
pub const PERMISSION_URL: Option<&str> =
    Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");

const PERMISSION_HELP: &str = "Open System Settings → Privacy & Security → Accessibility and \
switch on Calmcursor. If it is already on, switch it off and on again, then restart steadying.";

/// Marks events Calmcursor posts itself ("CALM" in ASCII).
const MAGIC: i64 = 0x4341_4C4D;
/// A move this far from where the pointer should be means another app
/// warped it.
const WARP_THRESHOLD: f64 = 150.0;

// --- Core Foundation / Core Graphics FFI -------------------------------------

type CFTypeRef = *const c_void;
type CFMachPortRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFStringRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CGEventRef = *mut c_void;
type CGEventSourceRef = *mut c_void;
type CGEventTapProxy = *mut c_void;
type CGDirectDisplayID = u32;
type CGEventTapCallBack =
    unsafe extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[repr(C)]
struct Opaque {
    _private: [u8; 0],
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventCreate(source: CGEventSourceRef) -> CGEventRef;
    fn CGEventCreateMouseEvent(
        source: CGEventSourceRef,
        mouse_type: u32,
        position: CGPoint,
        button: u32,
    ) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventSetLocation(event: CGEventRef, location: CGPoint);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    fn CGEventGetDoubleValueField(event: CGEventRef, field: u32) -> f64;
    fn CGEventSetDoubleValueField(event: CGEventRef, field: u32, value: f64);
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CGEventSourceCreate(state: i32) -> CGEventSourceRef;
    fn CGEventSourceSetLocalEventsSuppressionInterval(source: CGEventSourceRef, seconds: f64);
    fn CGGetActiveDisplayList(max: u32, displays: *mut CGDirectDisplayID, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: CGDirectDisplayID) -> CGRect;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopCommonModes: CFStringRef;
    static kCFBooleanTrue: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: Opaque;
    static kCFTypeDictionaryValueCallBacks: Opaque;
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFMachPortInvalidate(port: CFMachPortRef);
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRemoveSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRun();
    fn CFRunLoopStop(rl: CFRunLoopRef);
    fn CFRelease(cf: CFTypeRef);
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        count: isize,
        key_callbacks: *const Opaque,
        value_callbacks: *const Opaque,
    ) -> CFDictionaryRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
}

// Tap locations, placement and options.
const HID_EVENT_TAP: u32 = 0;
const HEAD_INSERT: u32 = 0;
const TAP_DEFAULT: u32 = 0;
const SOURCE_STATE_COMBINED: i32 = 0;

// Event types.
const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const RIGHT_DOWN: u32 = 3;
const RIGHT_UP: u32 = 4;
const MOVED: u32 = 5;
const LEFT_DRAGGED: u32 = 6;
const RIGHT_DRAGGED: u32 = 7;
const OTHER_DOWN: u32 = 25;
const OTHER_UP: u32 = 26;
const OTHER_DRAGGED: u32 = 27;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

// Event fields.
const FIELD_BUTTON_NUMBER: u32 = 3;
const FIELD_DELTA_X: u32 = 4;
const FIELD_DELTA_Y: u32 = 5;
const FIELD_SUBTYPE: u32 = 7;
const FIELD_USER_DATA: u32 = 42;
const SUBTYPE_TABLET_POINT: i64 = 1;
const SUBTYPE_TABLET_PROXIMITY: i64 = 2;

fn mask(types: &[u32]) -> u64 {
    types.iter().fold(0, |m, &t| m | (1u64 << t))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn to_vec(p: CGPoint) -> Vec2 {
    Vec2::new(p.x, p.y)
}

fn to_point(v: Vec2) -> CGPoint {
    CGPoint { x: v.x, y: v.y }
}

fn current_location() -> Option<Vec2> {
    // SAFETY: creating and releasing an empty event to read the cursor location.
    unsafe {
        let ev = CGEventCreate(std::ptr::null_mut());
        if ev.is_null() {
            return None;
        }
        let p = CGEventGetLocation(ev);
        CFRelease(ev as CFTypeRef);
        Some(to_vec(p))
    }
}

/// Where the pointer was last put, shared by the tap and the tick thread.
type LastOutput = Arc<Mutex<Vec2>>;

struct TapContext {
    shared: Arc<Shared>,
    port: AtomicPtr<c_void>,
    last_output: LastOutput,
}

/// Sets an event's location and its deltas relative to `prev`.
///
/// # Safety
/// `event` must be a valid CGEvent.
unsafe fn place_event(event: CGEventRef, pos: Vec2, prev: Vec2) {
    let d = pos.round() - prev.round();
    // SAFETY: guaranteed by the caller.
    unsafe {
        CGEventSetLocation(event, to_point(pos));
        CGEventSetIntegerValueField(event, FIELD_DELTA_X, d.x as i64);
        CGEventSetIntegerValueField(event, FIELD_DELTA_Y, d.y as i64);
        CGEventSetDoubleValueField(event, FIELD_DELTA_X, d.x);
        CGEventSetDoubleValueField(event, FIELD_DELTA_Y, d.y);
    }
}

unsafe extern "C" fn tap_callback(
    _proxy: CGEventTapProxy,
    etype: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    // SAFETY: `user_info` is the TapContext leaked in `run_tap`, alive until
    // the tap is invalidated.
    let ctx = unsafe { &*(user_info as *const TapContext) };
    if etype == TAP_DISABLED_BY_TIMEOUT || etype == TAP_DISABLED_BY_USER_INPUT {
        let port = ctx.port.load(Ordering::Acquire);
        if !port.is_null() {
            // SAFETY: `port` is our live tap.
            unsafe { CGEventTapEnable(port, true) };
        }
        return event;
    }
    if event.is_null() {
        return event;
    }
    // SAFETY: `event` is a valid event for the duration of the callback.
    unsafe {
        if CGEventGetIntegerValueField(event, FIELD_USER_DATA) == MAGIC {
            return event;
        }
        match etype {
            MOVED | LEFT_DRAGGED | RIGHT_DRAGGED | OTHER_DRAGGED => on_motion(ctx, event),
            LEFT_DOWN | LEFT_UP | RIGHT_DOWN | RIGHT_UP | OTHER_DOWN | OTHER_UP => {
                on_button(ctx, etype, event)
            }
            _ => event,
        }
    }
}

/// # Safety
/// `event` must be a valid mouse event.
unsafe fn on_motion(ctx: &TapContext, event: CGEventRef) -> CGEventRef {
    // SAFETY: guaranteed by the caller.
    let (loc, subtype, delta) = unsafe {
        (
            to_vec(CGEventGetLocation(event)),
            CGEventGetIntegerValueField(event, FIELD_SUBTYPE),
            Vec2::new(
                CGEventGetDoubleValueField(event, FIELD_DELTA_X),
                CGEventGetDoubleValueField(event, FIELD_DELTA_Y),
            ),
        )
    };
    let mut last = lock(&ctx.last_output);
    // Drawing tablets report absolute positions: leave them alone.
    let absolute = subtype == SUBTYPE_TABLET_POINT || subtype == SUBTYPE_TABLET_PROXIMITY;
    let warped = loc.distance(*last + delta) > WARP_THRESHOLD;
    let smoothed = ctx.shared.input(|e, t| {
        if absolute || warped || !e.is_enabled() {
            e.sync(t, loc);
            None
        } else {
            e.on_motion(t, delta);
            e.tick(t);
            Some(e.position())
        }
    });
    match smoothed {
        Some(pos) => {
            // SAFETY: guaranteed by the caller.
            unsafe { place_event(event, pos, *last) };
            *last = pos;
        }
        None => *last = loc,
    }
    event
}

/// # Safety
/// `event` must be a valid mouse button event.
unsafe fn on_button(ctx: &TapContext, etype: u32, event: CGEventRef) -> CGEventRef {
    let pressed = matches!(etype, LEFT_DOWN | RIGHT_DOWN | OTHER_DOWN);
    let button = match etype {
        LEFT_DOWN | LEFT_UP => Button::Left,
        RIGHT_DOWN | RIGHT_UP => Button::Right,
        // SAFETY: guaranteed by the caller.
        _ => match unsafe { CGEventGetIntegerValueField(event, FIELD_BUTTON_NUMBER) } {
            3 => Button::Back,
            4 => Button::Forward,
            _ => Button::Middle,
        },
    };
    let (action, pos, enabled) = ctx.shared.input(|e, t| {
        (
            e.on_button(t, button, pressed),
            e.position(),
            e.is_enabled(),
        )
    });
    if action == ButtonAction::Suppress {
        return std::ptr::null_mut();
    }
    if enabled {
        // Clicks land where the steadied pointer is drawn.
        // SAFETY: guaranteed by the caller.
        unsafe { CGEventSetLocation(event, to_point(pos)) };
    }
    event
}

struct MacOutput {
    source: CGEventSourceRef,
    last_output: LastOutput,
}

// SAFETY: the event source is only used from the tick thread after creation.
unsafe impl Send for MacOutput {}

impl Drop for MacOutput {
    fn drop(&mut self) {
        if !self.source.is_null() {
            // SAFETY: we own one reference to the source.
            unsafe { CFRelease(self.source as CFTypeRef) };
        }
    }
}

impl Output for MacOutput {
    fn apply(&mut self, pos: Vec2, held: Option<Button>) -> Result<(), String> {
        let (etype, button) = match held {
            None => (MOVED, 0),
            Some(Button::Left) => (LEFT_DRAGGED, 0),
            Some(Button::Right) => (RIGHT_DRAGGED, 1),
            Some(Button::Middle) => (OTHER_DRAGGED, 2),
            Some(Button::Back) => (OTHER_DRAGGED, 3),
            Some(Button::Forward) => (OTHER_DRAGGED, 4),
        };
        let mut last = lock(&self.last_output);
        // SAFETY: creating, tagging, posting and releasing our own event.
        unsafe {
            let ev = CGEventCreateMouseEvent(self.source, etype, to_point(pos), button);
            if ev.is_null() {
                return Err("could not create a pointer event".into());
            }
            place_event(ev, pos, *last);
            CGEventSetIntegerValueField(ev, FIELD_USER_DATA, MAGIC);
            if button > 2 {
                CGEventSetIntegerValueField(ev, FIELD_BUTTON_NUMBER, i64::from(button));
            }
            CGEventPost(HID_EVENT_TAP, ev);
            CFRelease(ev as CFTypeRef);
        }
        *last = pos;
        Ok(())
    }

    fn bounds(&mut self) -> Option<Rect> {
        let mut ids = [0u32; 16];
        let mut count = 0u32;
        // SAFETY: `ids` has room for 16 displays.
        let err = unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut count) };
        if err != 0 || count == 0 {
            return None;
        }
        ids[..count as usize]
            .iter()
            .map(|&id| {
                // SAFETY: `id` came from the active display list.
                let r = unsafe { CGDisplayBounds(id) };
                Rect::from_origin_size(
                    r.origin.x,
                    r.origin.y,
                    (r.size.width - 1.0).max(0.0),
                    (r.size.height - 1.0).max(0.0),
                )
            })
            .reduce(|a, b| a.union(&b))
    }
}

struct RunLoop(CFRunLoopRef);
// SAFETY: CFRunLoopStop may be called from any thread.
unsafe impl Send for RunLoop {}

#[derive(Debug)]
pub struct Backend {
    run_loop: CFRunLoopRef,
    tap: JoinHandle<()>,
    tick: JoinHandle<()>,
}

// SAFETY: the run loop reference is only used with CFRunLoopStop.
unsafe impl Send for Backend {}

impl Backend {
    pub fn start(shared: Arc<Shared>) -> Result<Self, PlatformError> {
        let start = current_location().unwrap_or_default();
        shared.with_engine(|e| e.sync(0.0, start));
        let last_output: LastOutput = Arc::new(Mutex::new(start));

        let (tx, rx) = std::sync::mpsc::channel::<Result<RunLoop, PlatformError>>();
        let ctx = TapContext {
            shared: shared.clone(),
            port: AtomicPtr::new(std::ptr::null_mut()),
            last_output: last_output.clone(),
        };
        let tap = std::thread::Builder::new()
            .name("calmcursor-event-tap".into())
            .spawn(move || run_tap(ctx, tx))
            .map_err(|e| PlatformError::os(e.to_string()))?;
        let run_loop = match rx.recv() {
            Ok(Ok(rl)) => rl.0,
            Ok(Err(e)) => {
                let _ = tap.join();
                return Err(e);
            }
            Err(_) => return Err(PlatformError::os("the event tap thread exited")),
        };

        // SAFETY: creating our own event source.
        let source = unsafe {
            let s = CGEventSourceCreate(SOURCE_STATE_COMBINED);
            if !s.is_null() {
                CGEventSourceSetLocalEventsSuppressionInterval(s, 0.0);
            }
            s
        };
        let tick = spawn_tick_thread(
            shared,
            MacOutput {
                source,
                last_output,
            },
        );
        Ok(Self {
            run_loop,
            tap,
            tick,
        })
    }

    pub fn stop(self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !self.tap.is_finished() && Instant::now() < deadline {
            // SAFETY: stopping our own run loop; safe from any thread.
            unsafe { CFRunLoopStop(self.run_loop) };
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.tap.join();
        let _ = self.tick.join();
    }
}

fn run_tap(ctx: TapContext, tx: std::sync::mpsc::Sender<Result<RunLoop, PlatformError>>) {
    let events = mask(&[
        MOVED,
        LEFT_DRAGGED,
        RIGHT_DRAGGED,
        OTHER_DRAGGED,
        LEFT_DOWN,
        LEFT_UP,
        RIGHT_DOWN,
        RIGHT_UP,
        OTHER_DOWN,
        OTHER_UP,
    ]);
    let ctx = Box::into_raw(Box::new(ctx));
    // SAFETY: standard event tap set-up. `ctx` stays alive until after the
    // tap is invalidated, and is freed exactly once at the end.
    unsafe {
        let port = CGEventTapCreate(
            HID_EVENT_TAP,
            HEAD_INSERT,
            TAP_DEFAULT,
            events,
            tap_callback,
            ctx as *mut c_void,
        );
        if port.is_null() {
            drop(Box::from_raw(ctx));
            let _ = tx.send(Err(PlatformError::PermissionDenied {
                message: "Calmcursor needs Accessibility access to steady the pointer".into(),
                help: PERMISSION_HELP.into(),
            }));
            return;
        }
        (*ctx).port.store(port, Ordering::Release);
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0);
        let rl = CFRunLoopGetCurrent();
        CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
        CGEventTapEnable(port, true);
        let _ = tx.send(Ok(RunLoop(rl)));

        CFRunLoopRun();

        CGEventTapEnable(port, false);
        CFRunLoopRemoveSource(rl, source, kCFRunLoopCommonModes);
        CFMachPortInvalidate(port);
        CFRelease(source as CFTypeRef);
        CFRelease(port as CFTypeRef);
        drop(Box::from_raw(ctx));
    }
}

pub fn permission_status() -> PermissionStatus {
    // SAFETY: simple query.
    if unsafe { AXIsProcessTrusted() } != 0 {
        PermissionStatus::Granted
    } else {
        PermissionStatus::Needed {
            message: "Calmcursor needs Accessibility access to steady the pointer".into(),
            help: PERMISSION_HELP.into(),
        }
    }
}

pub fn request_permission() -> bool {
    // SAFETY: builds a one-entry CFDictionary {prompt: true}, asks, releases it.
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );
        let trusted = AXIsProcessTrustedWithOptions(options) != 0;
        if !options.is_null() {
            CFRelease(options);
        }
        trusted
    }
}
