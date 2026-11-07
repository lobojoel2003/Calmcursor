//! The tray (Windows, Linux) or menu bar (macOS) icon and its menu.

use crate::state::{lock, show_main_window, AppState};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let enabled = state.settings().enabled;
    let toggle = CheckMenuItem::with_id(app, "toggle", "Steadying", true, enabled, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Calmcursor", true, None::<&str>)?;
    let calibrate = MenuItem::with_id(app, "calibrate", "Calibrate…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Calmcursor", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &toggle,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &calibrate,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let (icon, template) = if cfg!(target_os = "macos") {
        (
            Image::from_bytes(include_bytes!("../icons/tray-template.png"))?,
            true,
        )
    } else {
        (
            Image::from_bytes(include_bytes!("../icons/tray.png"))?,
            false,
        )
    };

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(template)
        .tooltip(if enabled {
            "Calmcursor: steadying on"
        } else {
            "Calmcursor: paused"
        })
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => app.state::<AppState>().toggle(app),
            "open" => show_main_window(app),
            "calibrate" => {
                show_main_window(app);
                let _ = app.emit("navigate", "calibrate");
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    *lock(&state.tray_toggle) = Some(toggle);
    Ok(())
}
