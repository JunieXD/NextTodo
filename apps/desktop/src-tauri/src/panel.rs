//! The menu bar or tray panel: what is next or a folder, ticked off and added to without the main
//! window, under the menu bar icon or above the tray's. See docs/desktop.md#菜单栏小窗.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Window};

pub const LABEL: &str = "panel";

/// Its size, and the gap between it and the icon, in logical pixels.
const WIDTH: f64 = 360.0;
const HEIGHT: f64 = 480.0;
const GAP: f64 = 4.0;

/// When it last hid for losing the focus.
#[derive(Default)]
pub struct Hidden(Mutex<Option<Instant>>);

/// Made hidden as the app starts, so that it opens at once.
pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::default())
        .title("NextTodo")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .visible(false)
        .shadow(true)
        // Files dragged in reach the page, to be added as attachments (docs/desktop.md#附件).
        .disable_drag_drop_handler()
        // Rounded: on macOS the page draws the corners, clear outside them; Windows 11 rounds those of
        // a window with a shadow itself.
        .transparent(cfg!(target_os = "macos"));
    crate::links(builder).build()?;
    Ok(())
}

/// Opens it, with the quick add field focused.
pub fn open(app: &AppHandle) {
    let panel = app.get_webview_window(LABEL).expect("made as the app starts");
    place(app, &panel);
    // Nothing to do about a window that the system won't show.
    let _ = panel.show().and_then(|()| panel.set_focus());
    let _ = app.emit_to(LABEL, "shown", ());
}

/// Opens it, or closes it when open.
pub fn toggle(app: &AppHandle) {
    let panel = app.get_webview_window(LABEL).expect("made as the app starts");
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
    } else {
        open(app);
    }
}

/// A click on the icon. On Windows the tray takes the focus first, which hides the panel: that click
/// closes it rather than opening it again.
pub fn clicked(app: &AppHandle) {
    let hidden = *app.state::<Hidden>().0.lock().unwrap();
    if !hidden.is_some_and(|at| at.elapsed() < Duration::from_millis(500)) {
        toggle(app);
    }
}

/// Another window or app has taken the focus.
pub fn blurred(window: &Window) {
    *window.state::<Hidden>().0.lock().unwrap() = Some(Instant::now());
    let _ = window.hide();
}

/// Under the icon, or above it when it is in the lower half of its screen, as a tray at the bottom
/// is; within the screen's work area, or against its left and top edges when too small for it. Where
/// it was when the icon has no place on screen.
fn place(app: &AppHandle, panel: &tauri::WebviewWindow) {
    let tray = app.tray_by_id(crate::TRAY).expect("made as the app starts");
    let Ok(Some(icon)) = tray.rect() else { return };
    // In physical pixels, as the tray gives them.
    let (at, size) = (icon.position.to_physical::<f64>(1.0), icon.size.to_physical::<f64>(1.0));
    let (middle, center) = (at.y + size.height / 2.0, at.x + size.width / 2.0);
    // The screen the icon is on. On some macOS versions tao lists no screens (nor finds one from a
    // point): the main one there, which has the menu bar.
    let screens = panel.available_monitors().unwrap_or_default();
    let on = screens.into_iter().find(|s| {
        let (corner, size) = (s.position().cast::<f64>(), s.size().cast::<f64>());
        (corner.x..corner.x + size.width).contains(&center) && (corner.y..corner.y + size.height).contains(&middle)
    });
    let Some(screen) = on.or_else(|| panel.primary_monitor().ok().flatten()) else { return };
    let scale = screen.scale_factor();
    let area = screen.work_area();
    let (left, top) = (f64::from(area.position.x), f64::from(area.position.y));
    let (right, bottom) = (left + f64::from(area.size.width), top + f64::from(area.size.height));
    let (width, height, gap) = (WIDTH * scale, HEIGHT * scale, GAP * scale);
    let x = (center - width / 2.0).min(right - width).max(left);
    let y = if middle < (top + bottom) / 2.0 { at.y + size.height + gap } else { at.y - height - gap };
    let y = y.min(bottom - height).max(top);
    // That is where its page goes: on Windows a window's position counts the invisible resize border
    // around it too, 13 pixels on the left at 200%.
    let (Ok(page), Ok(window)) = (panel.inner_position(), panel.outer_position()) else { return };
    let (x, y) = (x - f64::from(page.x - window.x), y - f64::from(page.y - window.y));
    // macOS places windows in points, the same on every screen; Windows in each screen's pixels.
    #[cfg(target_os = "macos")]
    let to = tauri::LogicalPosition::new(x / scale, y / scale);
    #[cfg(not(target_os = "macos"))]
    let to = tauri::PhysicalPosition::new(x, y);
    let _ = panel.set_position(to);
}
