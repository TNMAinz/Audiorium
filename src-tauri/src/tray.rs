//! The tray icon, and the question the window asks before it goes.
//!
//! Closing the window is two different wishes: quit the app, or get it out of the
//! way and keep what is playing. A close press is therefore held and turned into a
//! question for the interface; the two answers come back as commands below. The
//! tray icon is the way back to a window that was put away, so it exists from
//! launch rather than only after the first hide — a hidden window with nothing to
//! bring it back would be an app that cannot be reached.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Window, WindowEvent};

/// The event that carries a close press to the front end, which owns the dialog.
pub const CLOSE_REQUESTED: &str = "audiorium:close-requested";
/// A window that went to the tray. Nothing in it can be seen, so its meters and its
/// scene stop rather than burn a core drawing an invisible window.
pub const WINDOW_HIDDEN: &str = "audiorium:window-hidden";
/// A window that came back: the readings and the scene resume.
pub const WINDOW_SHOWN: &str = "audiorium:window-shown";

/// Builds the tray icon with its own menu: the app opens on a left click, and the
/// menu offers the same open plus a real quit.
pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open AUDIORIUM", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("audiorium")
        .tooltip("AUDIORIUM")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show(app),
            "quit" => app.exit(0),
            _ => {}
        })
        // A left click is the plain way back, so it opens rather than opening a menu.
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Puts the window back on screen: shown, restored from minimized and focused, so
/// the press has an effect on a window that was behind another one too.
fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        let _ = app.emit(WINDOW_SHOWN, ());
    }
}

/// Holds a close press and asks the front end instead of quitting. Every way of
/// closing — this app's own button, Alt+F4, the taskbar, the system menu — arrives
/// here, so all of them ask the same question.
pub fn intercept_close(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let _ = window.app_handle().emit(CLOSE_REQUESTED, ());
    }
}

/// A focused window is a window on screen, so a click on it resumes the readings even
/// if the tray's own "shown" notification was missed. Emitting again costs nothing:
/// the interface is already awake and stays awake.
pub fn intercept_focus(window: &Window, event: &WindowEvent) {
    if let WindowEvent::Focused(true) = event {
        let _ = window.app_handle().emit(WINDOW_SHOWN, ());
    }
}

/// "Minimize to tray": the window goes away and the app keeps running, so playback
/// and a running capture carry on. The tray icon is the way back.
#[tauri::command]
pub fn hide_to_tray(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
        let _ = app.emit(WINDOW_HIDDEN, ());
    }
}

/// "Close app": a real quit, with the same exit path as the tray's Quit, so the
/// window position is flushed before the process goes.
#[tauri::command]
pub fn quit_audiorium(app: AppHandle) {
    app.exit(0);
}
