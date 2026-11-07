//! Windows backend: a `WH_MOUSE_LL` low-level mouse hook.
//!
//! The hook sees every mouse event before Windows moves the cursor. For a
//! hardware move it records the delta (the would-be position minus the
//! current cursor position), feeds it to the engine and swallows the event,
//! so the cursor stays put. The tick thread then moves the cursor to the
//! smoothed position with `SetCursorPos`.
//!
//! Moves injected by other software (remote desktop, automation tools) are
//! passed through untouched and the engine re-centres on them.
//!
//! Windows will not let a normal process move the cursor over an elevated
//! (administrator) window. When `SetCursorPos` fails, the hook passes events
//! through until moving works again, so the pointer never freezes.

use crate::runner::{spawn_tick_thread, Output, Shared};
use crate::{PermissionStatus, PlatformError};
use calmcursor_core::{Button, ButtonAction, Rect, Vec2};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics,
    PostThreadMessageW, SetCursorPos, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    HC_ACTION, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_XBUTTONDOWN, WM_XBUTTONUP,
};

pub const NAME: &str = "Windows low-level mouse hook";
pub const PERMISSION_URL: Option<&str> = None;

/// How often to retry moving the cursor after Windows refused.
const OUTPUT_RETRY: Duration = Duration::from_millis(500);

/// State the hook procedure reaches through a global (hooks take no context).
struct HookContext {
    shared: Arc<Shared>,
    /// Last position set with `SetCursorPos`, packed as (x << 32 | y).
    last_set: AtomicI64,
    /// False while Windows refuses to move the cursor (elevated window).
    output_ok: AtomicBool,
}

static CONTEXT: Mutex<Option<Arc<HookContext>>> = Mutex::new(None);

fn context() -> Option<Arc<HookContext>> {
    CONTEXT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn pack(x: i32, y: i32) -> i64 {
    (i64::from(x) << 32) | i64::from(y as u32)
}

fn cursor_pos() -> Option<POINT> {
    let mut p = POINT::default();
    // SAFETY: `p` is a valid out pointer.
    unsafe { GetCursorPos(&mut p) }.ok().map(|_| p)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        if let Some(ctx) = context() {
            // SAFETY: for WH_MOUSE_LL with HC_ACTION, lparam points to a MSLLHOOKSTRUCT.
            let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            if handle_event(&ctx, wparam.0 as u32, info) {
                return LRESULT(1);
            }
        }
    }
    // SAFETY: forwarding the unchanged hook arguments.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Returns true to swallow the event.
fn handle_event(ctx: &HookContext, msg: u32, info: &MSLLHOOKSTRUCT) -> bool {
    let shared = &ctx.shared;
    let injected = info.flags & LLMHF_INJECTED != 0;
    if msg == WM_MOUSEMOVE {
        let pt = Vec2::new(f64::from(info.pt.x), f64::from(info.pt.y));
        if injected {
            // Our own SetCursorPos can echo back as an injected move.
            if ctx.last_set.load(Ordering::Relaxed) != pack(info.pt.x, info.pt.y) {
                shared.input(|e, t| e.sync(t, pt));
            }
            return false;
        }
        if !ctx.output_ok.load(Ordering::Relaxed) {
            shared.input(|e, t| e.sync(t, pt));
            return false;
        }
        let Some(cur) = cursor_pos() else {
            return false;
        };
        let delta = pt - Vec2::new(f64::from(cur.x), f64::from(cur.y));
        return shared.input(|e, t| {
            if e.is_enabled() {
                e.on_motion(t, delta);
                true
            } else {
                e.sync(t, pt);
                false
            }
        });
    }

    let (button, pressed) = match msg {
        WM_LBUTTONDOWN => (Button::Left, true),
        WM_LBUTTONUP => (Button::Left, false),
        WM_RBUTTONDOWN => (Button::Right, true),
        WM_RBUTTONUP => (Button::Right, false),
        WM_MBUTTONDOWN => (Button::Middle, true),
        WM_MBUTTONUP => (Button::Middle, false),
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            let which = if (info.mouseData >> 16) == 2 {
                Button::Forward
            } else {
                Button::Back
            };
            (which, msg == WM_XBUTTONDOWN)
        }
        _ => return false,
    };
    if injected {
        return false;
    }
    shared.input(|e, t| e.on_button(t, button, pressed)) == ButtonAction::Suppress
}

struct WindowsOutput {
    ctx: Arc<HookContext>,
    refused_at: Option<Instant>,
}

impl Output for WindowsOutput {
    fn on_thread_start(&mut self) {
        set_dpi_awareness();
    }

    fn apply(&mut self, pos: Vec2, _held: Option<Button>) -> Result<(), String> {
        let p = pos.round();
        let (x, y) = (p.x as i32, p.y as i32);
        self.ctx.last_set.store(pack(x, y), Ordering::Relaxed);
        // SAFETY: plain Win32 call with integer arguments.
        match unsafe { SetCursorPos(x, y) } {
            Ok(()) => {
                if self.refused_at.take().is_some() {
                    self.ctx.shared.set_warning(None);
                }
                Ok(())
            }
            Err(e) => {
                self.ctx.output_ok.store(false, Ordering::Relaxed);
                self.refused_at = Some(Instant::now());
                Err(format!(
                    "Windows did not allow moving the pointer ({e}); steadying is paused over \
                     this window. Run Calmcursor as administrator to steady elevated apps."
                ))
            }
        }
    }

    fn bounds(&mut self) -> Option<Rect> {
        // Re-test a refused output: setting the cursor to where it already is
        // changes nothing visible.
        if !self.ctx.output_ok.load(Ordering::Relaxed)
            && self.refused_at.is_none_or(|t| t.elapsed() >= OUTPUT_RETRY)
        {
            if let Some(p) = cursor_pos() {
                // SAFETY: plain Win32 call.
                if unsafe { SetCursorPos(p.x, p.y) }.is_ok() {
                    self.ctx.output_ok.store(true, Ordering::Relaxed);
                    self.refused_at = None;
                    self.ctx.shared.set_warning(None);
                } else {
                    self.refused_at = Some(Instant::now());
                }
            }
        }
        // SAFETY: plain Win32 calls.
        let (x, y, w, h) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        (w > 0 && h > 0).then(|| {
            Rect::from_corners(
                Vec2::new(f64::from(x), f64::from(y)),
                Vec2::new(f64::from(x + w - 1), f64::from(y + h - 1)),
            )
        })
    }
}

fn set_dpi_awareness() {
    // SAFETY: affects only the calling thread.
    unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[derive(Debug)]
pub struct Backend {
    hook_thread_id: u32,
    hook: JoinHandle<()>,
    tick: JoinHandle<()>,
}

impl Backend {
    pub fn start(shared: Arc<Shared>) -> Result<Self, PlatformError> {
        let ctx = Arc::new(HookContext {
            shared: shared.clone(),
            last_set: AtomicI64::new(i64::MIN),
            output_ok: AtomicBool::new(true),
        });
        {
            let mut slot = CONTEXT.lock().unwrap_or_else(|e| e.into_inner());
            if slot.is_some() {
                return Err(PlatformError::os("Calmcursor is already steadying"));
            }
            *slot = Some(ctx.clone());
        }

        set_dpi_awareness();
        if let Some(p) = cursor_pos() {
            shared.with_engine(|e| e.sync(0.0, Vec2::new(f64::from(p.x), f64::from(p.y))));
        }

        let (tx, rx) = std::sync::mpsc::channel::<Result<u32, String>>();
        let hook = std::thread::Builder::new()
            .name("calmcursor-hook".into())
            .spawn(move || {
                set_dpi_awareness();
                // SAFETY: standard hook installation on this thread, which then
                // pumps messages until WM_QUIT so the hook keeps being called.
                unsafe {
                    let module = match GetModuleHandleW(None) {
                        Ok(m) => m,
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string()));
                            return;
                        }
                    };
                    let hook = match SetWindowsHookExW(
                        WH_MOUSE_LL,
                        Some(mouse_proc),
                        Some(module.into()),
                        0,
                    ) {
                        Ok(h) => h,
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string()));
                            return;
                        }
                    };
                    let _ = tx.send(Ok(GetCurrentThreadId()));
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    let _ = UnhookWindowsHookEx(hook);
                }
            })
            .map_err(|e| PlatformError::os(e.to_string()))?;

        let hook_thread_id = match rx.recv() {
            Ok(Ok(id)) => id,
            Ok(Err(e)) => {
                clear_context();
                let _ = hook.join();
                return Err(PlatformError::os(format!(
                    "could not install the mouse hook: {e}"
                )));
            }
            Err(_) => {
                clear_context();
                return Err(PlatformError::os("the mouse hook thread exited"));
            }
        };

        // 1 ms timer resolution so 250 Hz ticks really are 4 ms apart.
        // SAFETY: balanced by timeEndPeriod in stop().
        unsafe {
            timeBeginPeriod(1);
        }
        let tick = spawn_tick_thread(
            shared,
            WindowsOutput {
                ctx,
                refused_at: None,
            },
        );
        Ok(Self {
            hook_thread_id,
            hook,
            tick,
        })
    }

    pub fn stop(self) {
        // SAFETY: posting WM_QUIT to our own hook thread.
        unsafe {
            let _ = PostThreadMessageW(self.hook_thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        let _ = self.hook.join();
        let _ = self.tick.join();
        clear_context();
        // SAFETY: balances timeBeginPeriod in start().
        unsafe {
            timeEndPeriod(1);
        }
    }
}

fn clear_context() {
    *CONTEXT.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn permission_status() -> PermissionStatus {
    PermissionStatus::Granted
}

pub fn request_permission() -> bool {
    true
}
