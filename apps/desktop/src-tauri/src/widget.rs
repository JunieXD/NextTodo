//! The desktop widget: what is next, on the desktop under every window, still there as the desktop is
//! shown; ticked off and added to without opening anything. Shown while the settings say so, where it
//! was left. See docs/desktop.md#桌面小组件.

use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Mutex;

use nexttodo_server::error::for_user;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, LogicalPosition, Manager, WebviewUrl, WebviewWindowBuilder, Window};

pub const LABEL: &str = "widget";

/// Its size at first and the least it takes, in logical pixels; how far from the screen's corner it
/// first is; how far down the middle of its top row is, by which it is moved.
const SIZE: (f64, f64) = (340.0, 440.0);
const LEAST: (f64, f64) = (280.0, 240.0);
const MARGIN: f64 = 24.0;
const GRIP: f64 = 22.0;

/// Where it is, as tao places a new window, and how large, in logical pixels at `scale`, that of the
/// screen it was on. On macOS that is in points on the desktop; on Windows, its pixels over `scale`,
/// which only with `scale` give back where on which screen.
#[derive(Clone, Copy, Serialize, Deserialize)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scale: f64,
}

/// What this computer keeps: whether it is shown, and where it was last, for when it is again.
#[derive(Default, Serialize, Deserialize)]
struct Kept {
    shown: bool,
    frame: Option<Frame>,
}

/// Held while the settings show or close it, which two quick turns of the switch could do at once.
static TURN: Mutex<()> = Mutex::new(());

fn file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_config_dir().map_err(|e| for_user("查找 App 的配置目录", &e))?.join("widget.json"))
}

fn kept(app: &AppHandle) -> Result<Kept, String> {
    match std::fs::read(file(app)?) {
        Ok(json) => serde_json::from_slice(&json).map_err(|e| for_user("读取桌面小组件的设置", &e)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Kept::default()),
        Err(e) => Err(for_user("读取桌面小组件的设置", &e)),
    }
}

/// Changes what is kept, one change at a time: it is moved on the main thread while the settings may
/// show or close it on another.
fn update(app: &AppHandle, change: impl FnOnce(&mut Kept)) -> Result<(), String> {
    static FILE: Mutex<()> = Mutex::new(());
    let _file = FILE.lock().unwrap();
    let mut kept = kept(app)?;
    change(&mut kept);
    let file = file(app)?;
    // Written whole, then put in its place: a write cut short leaves the last one as it was.
    let part = file.with_extension("json.part");
    std::fs::create_dir_all(file.parent().expect("in the config directory"))
        .and_then(|()| std::fs::write(&part, serde_json::to_vec(&kept).expect("plain data")))
        .and_then(|()| std::fs::rename(&part, &file))
        .map_err(|e| for_user("保存桌面小组件的设置", &e))
}

/// Shows it as the app starts, when it was left shown. One that can't be shown is left out, and the
/// app starts all the same.
pub fn start(app: &AppHandle) {
    let shown =
        kept(app).and_then(|kept| if kept.shown { build(app, kept.frame).map_err(|e| for_user("打开桌面小组件", &e)) } else { Ok(()) });
    if let Err(e) = shown {
        tracing::warn!("desktop widget not shown: {e}");
    }
    #[cfg(windows)]
    watch(app.clone());
}

/// Whether it is shown.
#[tauri::command]
pub fn widget(app: AppHandle) -> Result<bool, String> {
    Ok(kept(&app)?.shown)
}

/// Shows it or closes it, from the settings, and then keeps that: one that didn't open stays off.
/// Asynchronous: on Windows a window made in a synchronous command never shows.
#[tauri::command]
pub async fn set_widget(app: AppHandle, shown: bool) -> Result<(), String> {
    let _turn = TURN.lock().unwrap();
    match (shown, app.get_webview_window(LABEL)) {
        (true, None) => build(&app, kept(&app)?.frame).map_err(|e| for_user("打开桌面小组件", &e))?,
        (false, Some(widget)) => widget.destroy().map_err(|e| for_user("关闭桌面小组件", &e))?,
        _ => {}
    }
    update(&app, |kept| kept.shown = shown)
}

pub fn moved(window: &Window) {
    let Some(frame) = frame_of(window) else { return };
    if let Err(e) = update(window.app_handle(), |kept| kept.frame = Some(frame)) {
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
/// in, it stays under them. Not focused as it shows.
fn build(app: &AppHandle, frame: Option<Frame>) -> tauri::Result<()> {
    let frame = frame.filter(|f| on_screen(app, f)).unwrap_or_else(|| corner(app));
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::default())
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
    // Owned, it is neither in the taskbar nor in Alt+Tab.
    #[cfg(windows)]
    {
        let widget = crate::links(builder.owner_raw(owned())).build()?;
        // Tao takes a logical position to the first screen it is on at that screen's scale, which with
        // screens of different scales can be another one, and a move between them may leave its pixels
        // as they were: put where it was and as large, to the pixel.
        widget.set_position(LogicalPosition::new(frame.x, frame.y).to_physical::<i32>(frame.scale))?;
        widget.set_size(tauri::LogicalSize::new(frame.width, frame.height).to_physical::<u32>(frame.scale))?;
    }
    #[cfg(target_os = "macos")]
    stationary(&crate::links(builder).build()?)?;
    #[cfg(not(any(windows, target_os = "macos")))]
    crate::links(builder).build()?;
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

/// The top right corner of the main screen's work area, or of the desktop when tao finds no screen.
fn corner(app: &AppHandle) -> Frame {
    let (width, height) = SIZE;
    let Ok(Some(screen)) = app.primary_monitor() else { return Frame { x: MARGIN, y: MARGIN, width, height, scale: 1.0 } };
    let (scale, area) = (screen.scale_factor(), screen.work_area());
    let right = (f64::from(area.position.x) + f64::from(area.size.width)) / scale;
    Frame { x: right - width - MARGIN, y: f64::from(area.position.y) / scale + MARGIN, width, height, scale }
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

/// The desktop's window that owns it, as a handle in a number, to tell when Explorer has made another.
#[cfg(windows)]
static OWNER: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// The desktop's window (Progman), which owns it: an owned window stays just above its owner, which is
/// below every other window, and is raised with it as the desktop is shown (Win+D and the like), when
/// a window merely kept at the bottom would be left under it.
#[cfg(windows)]
fn owned() -> windows::Win32::Foundation::HWND {
    let desktop = desktop();
    OWNER.store(desktop.0 as isize, std::sync::atomic::Ordering::Relaxed);
    desktop
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
            let Some(widget) = app.get_webview_window(LABEL) else { continue };
            let desktop = desktop();
            if desktop.is_invalid() || desktop.0 as isize == OWNER.load(std::sync::atomic::Ordering::Relaxed) {
                continue;
            }
            match widget.hwnd() {
                Ok(hwnd) => {
                    own(hwnd, desktop);
                    OWNER.store(desktop.0 as isize, std::sync::atomic::Ordering::Relaxed);
                }
                // Closed meanwhile; made again, it takes the new one.
                Err(e) => tracing::warn!("desktop widget not handed to the new desktop window: {e}"),
            }
        }
    });
}

/// Makes `desktop` the owner of `widget`, as `owner_raw` does as a window is made.
#[cfg(windows)]
#[expect(unsafe_code, reason = "the windows crate marks every Win32 call unsafe; this one takes two handles and nothing else")]
fn own(widget: windows::Win32::Foundation::HWND, desktop: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{GWLP_HWNDPARENT, SetWindowLongPtrW};
    // SAFETY: both are window handles; for a top-level window GWLP_HWNDPARENT is its owner, and a handle
    // gone meanwhile only makes the call fail.
    unsafe { SetWindowLongPtrW(widget, GWLP_HWNDPARENT, desktop.0 as isize) };
}
