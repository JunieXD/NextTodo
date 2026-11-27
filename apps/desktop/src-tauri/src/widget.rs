//! The desktop widget: what is next, on the desktop under every window, still there as the desktop is
//! shown; ticked off and added to without opening anything. Shown while the settings say so, where it
//! was left. See docs/desktop.md#桌面小组件.

use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Mutex;

use nexttodo_server::error::for_user;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window};

const PREFIX: &str = "widget-";
const LIMIT: usize = 6;
const CHANGED: &str = "widgets-changed";
/// Asks a widget to save its changes and close.
const REMOVE: &str = "widget-remove";

const SIZE: (f64, f64) = (340.0, 440.0);
const LEAST: (f64, f64) = (280.0, 240.0);
const MARGIN: f64 = 24.0;
const GRIP: f64 = 22.0;

pub fn is_widget(label: &str) -> bool {
    label.starts_with(PREFIX)
}

/// Logical pixels at the scale of the screen it was on: on Windows the scale gives back its pixels.
#[derive(Clone, Copy, Serialize, Deserialize)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
}

#[derive(Serialize, Deserialize)]
struct Widget {
    id: u32,
    frame: Option<Frame>,
}

impl Widget {
    fn label(&self) -> String {
        format!("{PREFIX}{}", self.id)
    }
}

#[derive(Serialize, Deserialize)]
struct Kept {
    next_id: u32,
    widgets: Vec<Widget>,
}

impl Default for Kept {
    fn default() -> Self {
        Self { next_id: 1, widgets: Vec::new() }
    }
}

impl Kept {
    fn add(&mut self, frame: Frame) -> Result<String, String> {
        if self.widgets.len() >= LIMIT {
            return Err("最多可放 6 个桌面小组件".into());
        }
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or("桌面小组件编号已用完")?;
        let widget = Widget { id, frame: Some(frame) };
        let label = widget.label();
        self.widgets.push(widget);
        Ok(label)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OnDisk {
    Current(Kept),
    Single { shown: bool, frame: Option<Frame> },
}

impl From<OnDisk> for Kept {
    fn from(disk: OnDisk) -> Self {
        match disk {
            OnDisk::Current(kept) => kept,
            // The webview carries the old view over to widget-1, never to a widget added later.
            OnDisk::Single { shown, frame } => Self { next_id: 2, widgets: if shown { vec![Widget { id: 1, frame }] } else { Vec::new() } },
        }
    }
}

// Commands at once would otherwise open or remove the same window. Moved events take only FILE:
// Tauri can wait for the main thread while a command holds TURN.
static TURN: Mutex<()> = Mutex::new(());
static FILE: Mutex<()> = Mutex::new(());

fn file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_config_dir().map_err(|e| for_user("查找 App 的配置目录", &e))?.join("widget.json"))
}

fn kept(app: &AppHandle) -> Result<Kept, String> {
    match std::fs::read(file(app)?) {
        Ok(json) => serde_json::from_slice::<OnDisk>(&json).map(Kept::from).map_err(|e| for_user("读取桌面小组件的设置", &e)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Kept::default()),
        Err(e) => Err(for_user("读取桌面小组件的设置", &e)),
    }
}

fn update<T>(app: &AppHandle, change: impl FnOnce(&mut Kept) -> Result<T, String>) -> Result<T, String> {
    let _file = FILE.lock().unwrap();
    let mut kept = kept(app)?;
    let result = change(&mut kept)?;
    let file = file(app)?;
    // Written whole, then put in its place: a write cut short leaves the last one as it was.
    let part = file.with_extension("json.part");
    std::fs::create_dir_all(file.parent().expect("in the config directory"))
        .and_then(|()| std::fs::write(&part, serde_json::to_vec(&kept).expect("plain data")))
        .and_then(|()| std::fs::rename(&part, &file))
        .map_err(|e| for_user("保存桌面小组件的设置", &e))?;
    Ok(result)
}

pub fn start(app: &AppHandle) {
    match kept(app) {
        Ok(kept) => {
            for (i, widget) in kept.widgets.iter().enumerate() {
                let frame = widget
                    .frame
                    .filter(|f| on_screen(app, f))
                    .unwrap_or_else(|| corner(app, widget.frame.map_or(SIZE, |f| (f.width, f.height)), i));
                if let Err(e) = build(app, &widget.label(), frame, None) {
                    tracing::warn!("desktop widget not shown: {e}");
                }
            }
        }
        Err(e) => tracing::warn!("desktop widgets not read: {e}"),
    }
    #[cfg(windows)]
    watch(app.clone());
}

#[derive(Serialize)]
pub struct Widgets {
    labels: Vec<String>,
    full: bool,
}

#[tauri::command]
pub fn widgets(app: AppHandle) -> Result<Widgets, String> {
    let kept = kept(&app)?;
    Ok(Widgets { labels: kept.widgets.iter().map(Widget::label).collect(), full: kept.widgets.len() >= LIMIT })
}

// Asynchronous: on Windows a window made in a synchronous command never shows.
#[tauri::command]
pub async fn add_widget(app: AppHandle, page: String) -> Result<(), String> {
    let _turn = TURN.lock().unwrap();
    let frame = placement(&app, &kept(&app)?.widgets);
    let label = update(&app, |kept| kept.add(frame))?;
    if let Err(e) = build(&app, &label, frame, Some(&page)) {
        forget(&app, &label)?;
        return Err(for_user("打开桌面小组件", &e));
    }
    changed(&app, None)
}

/// Asks the widget to close: it saves its changes first, for the main window to send them, then
/// calls `close_widget` (docs/desktop.md#桌面小组件). One no longer open just goes.
#[tauri::command]
pub async fn remove_widget(app: AppHandle, label: String) -> Result<(), String> {
    if !is_widget(&label) {
        return Err("这不是桌面小组件".into());
    }
    if app.get_webview_window(&label).is_some() {
        return app.emit_to(label.as_str(), REMOVE, ()).map_err(|e| for_user("移除桌面小组件", &e));
    }
    let _turn = TURN.lock().unwrap();
    forget(&app, &label)?;
    changed(&app, Some(&label))
}

/// The widget calling it closes, its changes saved.
#[tauri::command]
pub async fn close_widget(widget: WebviewWindow) -> Result<(), String> {
    let app = widget.app_handle();
    let _turn = TURN.lock().unwrap();
    if !is_widget(widget.label()) {
        return Err("这不是桌面小组件".into());
    }
    forget(app, widget.label())?;
    widget.destroy().map_err(|e| for_user("关闭桌面小组件", &e))?;
    changed(app, Some(widget.label()))
}

fn forget(app: &AppHandle, label: &str) -> Result<(), String> {
    update(app, |kept| {
        kept.widgets.retain(|w| w.label() != label);
        Ok(())
    })
}

fn changed(app: &AppHandle, removed: Option<&str>) -> Result<(), String> {
    app.emit_to("main", CHANGED, removed).map_err(|e| for_user("通知主窗口小组件有变动", &e))
}

pub fn moved(window: &Window) {
    let Some(frame) = frame_of(window) else { return };
    if let Err(e) = update(window.app_handle(), |kept| {
        if let Some(widget) = kept.widgets.iter_mut().find(|w| w.label() == window.label()) {
            widget.frame = Some(frame);
        }
        Ok(())
    }) {
        tracing::warn!("desktop widget's place not kept: {e}");
    }
}

/// None once it has closed.
fn frame_of(window: &Window) -> Option<Frame> {
    let (at, size, scale) = (window.outer_position().ok()?, window.inner_size().ok()?, window.scale_factor().ok()?);
    let (at, size) = (at.to_logical::<f64>(scale), size.to_logical::<f64>(scale));
    Some(Frame { x: at.x, y: at.y, width: size.width, height: size.height, scale })
}

/// Where it was, or the top right corner of the main screen, with the screen it was on gone or scaled
/// otherwise. Not in the taskbar or Alt+Tab, and never in front of another window: clicked or typed
/// in, it stays under them. Not focused as it shows. A new one opens at the `first` page chosen for
/// it, which the page keeps from then on.
fn build(app: &AppHandle, label: &str, frame: Frame, first: Option<&str>) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .title("NextTodo")
        .position(frame.x, frame.y)
        .inner_size(frame.width, frame.height)
        .min_inner_size(LEAST.0, LEAST.1)
        .decorations(false)
        .shadow(false)
        // The page draws the widget, its background in the colour and opacity chosen.
        .transparent(true)
        .always_on_bottom(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        // Out of the taskbar there is no bringing it back; a double click on the top row would make it
        // fill the screen.
        .minimizable(false)
        .maximizable(false)
        .focused(false)
        // Files dragged in reach the page, as in the panel (docs/desktop.md#附件).
        .disable_drag_drop_handler();
    let builder = match first {
        Some(page) => builder.initialization_script(format!("window.nexttodoFirstPage = {};", serde_json::to_string(page)?)),
        None => builder,
    };
    #[cfg(windows)]
    let builder = builder.owner_raw(desktop());
    let widget = crate::links(builder).build()?;
    if let Err(e) = configure(&widget, frame) {
        widget.destroy()?;
        return Err(e);
    }
    Ok(())
}

fn configure(_widget: &WebviewWindow, _frame: Frame) -> tauri::Result<()> {
    #[cfg(windows)]
    {
        // Tao may turn the logical position and size back into pixels at another screen's scale.
        let Frame { x, y, width, height, scale } = _frame;
        _widget.set_position(LogicalPosition::new(x, y).to_physical::<i32>(scale))?;
        _widget.set_size(tauri::LogicalSize::new(width, height).to_physical::<u32>(scale))?;
    }
    #[cfg(target_os = "macos")]
    stationary(_widget)?;
    Ok(())
}

/// Whether the middle of its top row, where it is moved by, is on a screen at the scale it was on.
/// When tao lists no screens, as on some macOS versions (see panel.rs), where it was.
fn on_screen(app: &AppHandle, frame: &Frame) -> bool {
    let at = LogicalPosition::new(frame.x + frame.width / 2.0, frame.y + GRIP).to_physical::<f64>(frame.scale);
    let screens = app.available_monitors().unwrap_or_default();
    screens.is_empty()
        || screens.iter().any(|s| {
            let (corner, size) = (s.position().cast::<f64>(), s.size().cast::<f64>());
            s.scale_factor() == frame.scale
                && (corner.x..corner.x + size.width).contains(&at.x)
                && (corner.y..corner.y + size.height).contains(&at.y)
        })
}

/// The top right corner of the main screen's work area, `offset` times a margin to the bottom left, or
/// of the desktop when tao finds no screen. A widget from a larger screen is made to fit this one,
/// all of it in sight.
fn corner(app: &AppHandle, (width, height): (f64, f64), offset: usize) -> Frame {
    let shift = MARGIN * offset as f64;
    let Ok(Some(screen)) = app.primary_monitor() else { return Frame { x: MARGIN + shift, y: MARGIN + shift, width, height, scale: 1.0 } };
    let (scale, area) = (screen.scale_factor(), screen.work_area());
    let (left, top) = (f64::from(area.position.x) / scale, f64::from(area.position.y) / scale);
    let (across, down) = (f64::from(area.size.width) / scale, f64::from(area.size.height) / scale);
    let width = width.min(across - 2.0 * MARGIN).max(LEAST.0);
    let height = height.min(down - 2.0 * MARGIN).max(LEAST.1);
    let x = (left + across - width - MARGIN - shift).max(left + MARGIN);
    let y = (top + MARGIN + shift).min(top + down - height - MARGIN).max(top + MARGIN);
    Frame { x, y, width, height, scale }
}

fn placement(app: &AppHandle, widgets: &[Widget]) -> Frame {
    (0..=widgets.len())
        .rev()
        .map(|i| corner(app, SIZE, i))
        .find(|frame| !widgets.iter().filter_map(|w| w.frame).any(|f| f.x == frame.x && f.y == frame.y && f.scale == frame.scale))
        .unwrap_or_else(|| corner(app, SIZE, 0))
}

/// Stays where it is as the desktop is shown and in Mission Control, and out of ⌘` (as Übersicht's
/// widgets). Tao has no setting for these: its NSWindow, found among the app's, takes them.
#[cfg(target_os = "macos")]
fn stationary(widget: &tauri::WebviewWindow) -> tauri::Result<()> {
    use objc2_app_kit::{NSApplication, NSWindow, NSWindowCollectionBehavior};
    let made = widget.clone();
    widget.run_on_main_thread(move || {
        // Closed again meanwhile.
        let Ok(ns) = made.ns_window() else { return };
        let mtm = objc2::MainThreadMarker::new().expect("on the main thread");
        let window = NSApplication::sharedApplication(mtm)
            .windows()
            .to_vec()
            .into_iter()
            .find(|w| std::ptr::eq(&**w, ns.cast::<NSWindow>()))
            .expect("one of the app's windows");
        let behavior = NSWindowCollectionBehavior::Stationary | NSWindowCollectionBehavior::IgnoresCycle;
        window.setCollectionBehavior(window.collectionBehavior() | behavior);
    })
}

/// Null while Explorer is not running.
#[cfg(windows)]
#[expect(unsafe_code, reason = "the windows crate marks every Win32 call unsafe; this one takes nothing and only returns a handle")]
fn desktop() -> windows::Win32::Foundation::HWND {
    // SAFETY: no arguments, no memory; the handle is only compared and handed over as an owner.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetShellWindow() }
}

/// Explorer restarted makes the desktop's window anew, and the widget loses its owner: it is handed to
/// the new one. Looked at every 10 seconds; Windows tells no one but the taskbar's icons.
#[cfg(windows)]
fn watch(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(10));
            let desktop = desktop();
            if desktop.is_invalid() {
                continue;
            }
            for (label, widget) in app.webview_windows() {
                if is_widget(&label) {
                    match widget.hwnd() {
                        Ok(hwnd) => restore_owner(hwnd, desktop),
                        // Closed meanwhile; made again, it takes the new one.
                        Err(e) => tracing::warn!("desktop widget's owner not checked: {e}"),
                    }
                }
            }
        }
    });
}

/// Gives the widget to the new desktop, as `owner_raw` does when it is made.
#[cfg(windows)]
#[expect(unsafe_code, reason = "the windows crate marks every Win32 call unsafe; this one takes two handles and nothing else")]
fn restore_owner(widget: windows::Win32::Foundation::HWND, desktop: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{GW_OWNER, GWLP_HWNDPARENT, GetWindow, SetWindowLongPtrW};
    // SAFETY: both are window handles; for a top-level window GWLP_HWNDPARENT is its owner, and a handle
    // gone meanwhile only makes the call fail.
    unsafe {
        if GetWindow(widget, GW_OWNER).ok() != Some(desktop) {
            SetWindowLongPtrW(widget, GWLP_HWNDPARENT, desktop.0 as isize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Frame = Frame { x: 1200.0, y: 49.0, width: 374.0, height: 459.0, scale: 2.0 };

    #[test]
    fn upgrades_the_visible_widget_with_its_frame() {
        let single = serde_json::json!({ "shown": true, "frame": FRAME });
        let kept = Kept::from(serde_json::from_value::<OnDisk>(single).unwrap());
        let [widget] = kept.widgets.as_slice() else { panic!("expected one widget") };
        assert_eq!(widget.label(), "widget-1");
        assert_eq!(serde_json::to_value(widget.frame).unwrap(), serde_json::to_value(FRAME).unwrap());
    }

    #[test]
    fn upgrades_a_hidden_widget_without_restoring_it_or_its_view() {
        let single = serde_json::json!({ "shown": false, "frame": FRAME });
        let mut kept = Kept::from(serde_json::from_value::<OnDisk>(single).unwrap());
        assert!(kept.widgets.is_empty());
        assert_eq!(kept.add(FRAME).unwrap(), "widget-2");
    }

    #[test]
    fn caps_the_list_and_does_not_reuse_removed_numbers() {
        let mut kept = Kept::default();
        for id in 1..=6 {
            assert_eq!(kept.add(FRAME).unwrap(), format!("widget-{id}"));
        }
        assert!(kept.add(FRAME).is_err());
        kept.widgets.remove(1);
        let disk = serde_json::to_value(&kept).unwrap();
        let mut kept = Kept::from(serde_json::from_value::<OnDisk>(disk).unwrap());
        assert_eq!(kept.add(FRAME).unwrap(), "widget-7");
        assert!(kept.add(FRAME).is_err());
    }
}
