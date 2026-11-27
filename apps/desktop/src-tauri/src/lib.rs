//! The desktop app, for macOS and Windows: the web app in a window that closing only hides, a menu
//! bar or tray icon and a global shortcut for the panel, the desktop widget, reminders while
//! it runs, updates, the server inside it for local mode, and the command line it carries. See
//! docs/desktop.md.

mod attachments;
mod cli;
mod local;
mod panel;
mod reminders;
mod shortcut;
mod updates;
mod widget;

use std::io::IsTerminal;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::NewWindowResponse;
#[cfg(target_os = "macos")]
use tauri::{ActivationPolicy, RunEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::ShortcutState;
use tauri_plugin_notification::{Action, ActionType, NotificationExt};
use tracing_subscriber::EnvFilter;

/// Started at login: in the menu bar only, without the window.
const HIDDEN: &str = "--hidden";

/// The menu bar or tray icon.
const TRAY: &str = "tray";

fn show(app: &AppHandle) {
    // Started at login in the menu bar only, the app takes its place in the Dock with the window.
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(ActivationPolicy::Regular);
    let window = app.get_webview_window("main").expect("the main window is in tauri.conf.json");
    // Nothing to do about a window that the system won't show.
    let _ = window.show().and_then(|()| window.unminimize()).and_then(|()| window.set_focus());
}

/// Links that would open a new window, like those in descriptions, open in the default browser or
/// mail app instead.
fn links<R: Runtime, M: Manager<R>>(builder: WebviewWindowBuilder<'_, R, M>) -> WebviewWindowBuilder<'_, R, M> {
    builder.on_new_window(|url, _| {
        if matches!(url.scheme(), "http" | "https" | "mailto") {
            // Nothing to do about a link the system won't open.
            let _ = open::that_detached(url.as_str());
        }
        NewWindowResponse::Deny
    })
}

/// The main window of tauri.conf.json, where it is left to be made here, for its links.
fn window(app: &AppHandle) -> tauri::Result<()> {
    let config = app.config().app.windows.iter().find(|w| w.label == "main").expect("the main window is in tauri.conf.json");
    links(WebviewWindowBuilder::from_config(app, config)?).build()?;
    Ok(())
}

/// The main window, at `path` of the web app: for the panel, a task or what it shows; for a
/// reminder, its task.
#[tauri::command]
fn open_main(app: AppHandle, path: String) {
    show(&app);
    let _ = app.emit_to("main", "open", path);
    let _ = app.get_webview_window(panel::LABEL).expect("made as the app starts").hide();
}

fn tray(app: &AppHandle) -> tauri::Result<()> {
    let add = MenuItem::with_id(app, "add", "快速添加", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "打开 NextTodo", true, None::<&str>)?;
    let lock = MenuItem::with_id(app, "lock", "锁定", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&add, &open, &lock, &PredefinedMenuItem::separator(app)?, &quit])?;
    // macOS tints a one-colour image to suit the menu bar; the Windows tray shows the app icon.
    #[cfg(target_os = "macos")]
    let icon = tauri::include_image!("icons/tray.png");
    #[cfg(not(target_os = "macos"))]
    let icon = app.default_window_icon().expect("tauri.conf.json has the app icon").clone();
    TrayIconBuilder::with_id(TRAY)
        .icon(icon)
        .icon_as_template(true)
        .tooltip("NextTodo")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "add" => panel::open(app),
            "open" => show(app),
            // The page locks, or opens the privacy lock's settings when there is none.
            "lock" => {
                let _ = app.emit_to("main", "lock", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                panel::clicked(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![HIDDEN])))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                // The panel's, the only one.
                .with_handler(|app, _, event| {
                    if event.state == ShortcutState::Pressed {
                        panel::toggle(app);
                    }
                })
                .build(),
        )
        .manage(reminders::Reminders::default())
        .manage(updates::Downloaded::default())
        .manage(local::Server::default())
        .manage(panel::Hidden::default())
        .invoke_handler(tauri::generate_handler![
            open_main,
            panel::hide_panel,
            reminders::set_reminders,
            reminders::clear_reminders,
            updates::update,
            updates::restart,
            local::local_mode,
            local::set_local_mode,
            local::start_local,
            local::backup_local,
            local::prepare_restore,
            local::commit_restore,
            attachments::open_attachment,
            attachments::save_attachment,
            cli::cli_status,
            cli::install_cli,
            shortcut::shortcut,
            shortcut::set_shortcut,
            shortcut::pause_shortcut,
            widget::widgets,
            widget::add_widget,
            widget::remove_widget,
            widget::close_widget
        ])
        .setup(|app| {
            attachments::clear();
            window(app.handle())?;
            panel::build(app.handle())?;
            widget::start(app.handle());
            tray(app.handle())?;
            // A click on a reminder, or on its button, opens its task; on a daily digest, 接下来.
            let open = Action::builder(reminders::OPEN, "打开").build();
            app.notification().register_action_types(vec![ActionType::builder(reminders::KIND).actions(vec![open]).build()])?;
            let handle = app.handle().clone();
            app.notification().on_action(move |action| {
                if !matches!(action.action_id(), "tap" | reminders::OPEN) {
                    return;
                }
                let task = action.notification().and_then(|n| n.extra().get("task")).and_then(|t| t.as_str());
                open_main(handle.clone(), task.map_or_else(|| "/".into(), |task| format!("/?open={task}")));
            })?;
            // One that can't be read does nothing, and the settings tell.
            let _ = shortcut::register(app.handle());
            reminders::start(app.handle().clone());
            if std::env::args().any(|a| a == HIDDEN) {
                #[cfg(target_os = "macos")]
                app.set_activation_policy(ActivationPolicy::Accessory);
            } else {
                show(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Closing a window keeps the app in the menu bar or tray, syncing and reminding. The widget,
            // closed in the settings, stays.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                if !widget::is_widget(window.label()) {
                    let _ = window.hide();
                }
            }
            WindowEvent::Moved(_) | WindowEvent::Resized(_) if widget::is_widget(window.label()) => widget::moved(window),
            WindowEvent::Focused(false) if window.label() == panel::LABEL => panel::blurred(window),
            WindowEvent::Focused(true) => local::focused(window.app_handle()),
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("the app starts")
        .run(|_app, _event| {
            // Clicking the Dock icon brings the window back.
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen { .. } = _event {
                show(_app);
            }
        });
}
