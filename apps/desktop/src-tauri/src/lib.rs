//! The desktop app, for macOS and Windows: the web app in a window that closing only hides, a menu
//! bar or tray icon and a global shortcut for quick add, deadline reminders while it runs, updates,
//! and the server inside it for local mode. See docs/desktop.md.

mod local;
mod reminders;
mod updates;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::NewWindowResponse;
#[cfg(target_os = "macos")]
use tauri::{ActivationPolicy, RunEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_notification::{Action, ActionType, NotificationExt};

/// Started at login: in the menu bar only, without the window.
const HIDDEN: &str = "--hidden";

fn show(app: &AppHandle) {
    // Started at login in the menu bar only, the app takes its place in the Dock with the window.
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(ActivationPolicy::Regular);
    let window = app.get_webview_window("main").expect("the main window is in tauri.conf.json");
    // Nothing to do about a window that the system won't show.
    let _ = window.show().and_then(|()| window.unminimize()).and_then(|()| window.set_focus());
}

/// The main window of tauri.conf.json, where it is left to be made here: links that would open a
/// new one, like those in descriptions, open in the default browser or mail app instead.
fn window(app: &AppHandle) -> tauri::Result<()> {
    let config = app.config().app.windows.iter().find(|w| w.label == "main").expect("the main window is in tauri.conf.json");
    WebviewWindowBuilder::from_config(app, config)?
        .on_new_window(|url, _| {
            if matches!(url.scheme(), "http" | "https" | "mailto") {
                // Nothing to do about a link the system won't open.
                let _ = open::that_detached(url.as_str());
            }
            NewWindowResponse::Deny
        })
        .build()?;
    Ok(())
}

/// The window, with the quick add field focused.
fn quick_add(app: &AppHandle) {
    show(app);
    let _ = app.emit_to("main", "quick-add", ());
}

fn tray(app: &AppHandle) -> tauri::Result<()> {
    let add = MenuItem::with_id(app, "add", "快速添加", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "打开 NextTodo", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&add, &open, &PredefinedMenuItem::separator(app)?, &quit])?;
    // macOS tints a one-colour image to suit the menu bar; the Windows tray shows the app icon.
    #[cfg(target_os = "macos")]
    let icon = tauri::include_image!("icons/tray.png");
    #[cfg(not(target_os = "macos"))]
    let icon = app.default_window_icon().expect("tauri.conf.json has the app icon").clone();
    TrayIconBuilder::new()
        .icon(icon)
        .icon_as_template(true)
        .tooltip("NextTodo")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "add" => quick_add(app),
            "open" => show(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                quick_add(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    // ⌃⌥Space on macOS, Ctrl+Alt+Space on Windows, from any app.
    let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![HIDDEN])))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, pressed, event| {
                    if *pressed == shortcut && event.state == ShortcutState::Pressed {
                        quick_add(app);
                    }
                })
                .build(),
        )
        .manage(reminders::Reminders::default())
        .manage(updates::Downloaded::default())
        .manage(local::Server::default())
        .invoke_handler(tauri::generate_handler![
            reminders::set_reminders,
            reminders::clear_reminders,
            updates::update,
            updates::restart,
            local::local_mode,
            local::set_local_mode,
            local::start_local,
            local::backup_local,
            local::prepare_restore,
            local::commit_restore
        ])
        .setup(move |app| {
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            window(app.handle())?;
            tray(app.handle())?;
            // A click on a reminder, or on its button, opens its task.
            let open = Action::builder(reminders::OPEN, "打开").build();
            app.notification().register_action_types(vec![ActionType::builder(reminders::KIND).actions(vec![open]).build()])?;
            let handle = app.handle().clone();
            app.notification().on_action(move |action| {
                if !matches!(action.action_id(), "tap" | reminders::OPEN) {
                    return;
                }
                if let Some(task) = action.notification().and_then(|n| n.extra().get("task")) {
                    show(&handle);
                    let _ = handle.emit_to("main", "open-task", task);
                }
            })?;
            // Taken by another app, the shortcut does nothing; the menu bar or tray icon still works.
            if let Err(e) = app.global_shortcut().register(shortcut) {
                eprintln!("全局快捷键注册失败：{e}");
            }
            reminders::start(app.handle().clone());
            if std::env::args().any(|a| a == HIDDEN) {
                #[cfg(target_os = "macos")]
                app.set_activation_policy(ActivationPolicy::Accessory);
            } else {
                show(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the app in the menu bar or tray, syncing and reminding.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
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
