//! The global shortcut for the panel: none until the user picks one in the settings, kept on this
//! computer and registered as the app starts. See docs/desktop.md#小窗快捷键.

use std::io::ErrorKind;
use std::path::PathBuf;

use nexttodo_server::error::for_user;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// What the settings show: the shortcut, as `control+alt+KeyK`, and whether it could be registered.
#[derive(Serialize)]
pub struct Saved {
    keys: String,
    taken: bool,
}

/// The file holding it, there only while there is one.
fn file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_config_dir().map_err(|e| for_user("查找 App 的配置目录", &e))?.join("shortcut"))
}

fn saved(app: &AppHandle) -> Result<Option<Shortcut>, String> {
    match std::fs::read_to_string(file(app)?) {
        Ok(keys) => keys.parse().map(Some).map_err(|e| for_user("读取小窗快捷键", &e)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(for_user("读取小窗快捷键", &e)),
    }
}

/// Registers the saved one unless it is already. One that another app has taken is left for the
/// settings to tell.
pub fn register(app: &AppHandle) -> Result<(), String> {
    let Some(shortcut) = saved(app)? else { return Ok(()) };
    let shortcuts = app.global_shortcut();
    if !shortcuts.is_registered(shortcut)
        && let Err(e) = shortcuts.register(shortcut)
    {
        tracing::warn!("注册小窗快捷键 {shortcut}：{e}");
    }
    Ok(())
}

#[tauri::command]
pub fn shortcut(app: AppHandle) -> Result<Option<Saved>, String> {
    Ok(saved(&app)?.map(|s| Saved { keys: s.to_string(), taken: !app.global_shortcut().is_registered(s) }))
}

/// Puts `keys` in place of the shortcut, or none. What is registered stays what is saved: one that
/// can't be registered is not saved, and one that can't be saved is not left registered.
#[tauri::command]
pub fn set_shortcut(app: AppHandle, keys: Option<String>) -> Result<(), String> {
    let file = file(&app)?;
    let shortcuts = app.global_shortcut();
    let Some(keys) = keys else {
        if let Err(e) = std::fs::remove_file(&file)
            && e.kind() != ErrorKind::NotFound
        {
            return Err(for_user("清除小窗快捷键", &e));
        }
        return shortcuts.unregister_all().map_err(|e| for_user("停用小窗快捷键", &e));
    };
    let shortcut: Shortcut = keys.parse().map_err(|_| "不支持这个按键".to_owned())?;
    shortcuts.unregister_all().map_err(|e| for_user("停用小窗快捷键", &e))?;
    shortcuts.register(shortcut).map_err(|_| "该快捷键无法使用，可能已被其他应用占用，请换一个".to_owned())?;
    std::fs::create_dir_all(file.parent().expect("in the config directory"))
        .and_then(|()| std::fs::write(&file, shortcut.to_string()))
        .map_err(|e| {
            // Taking one ends with the saved one registered again.
            let _ = shortcuts.unregister_all();
            for_user("保存小窗快捷键", &e)
        })
}

/// Stops it while the settings take a new one, so that pressing the same keys there does not open the
/// panel; starts it again after.
#[tauri::command]
pub fn pause_shortcut(app: AppHandle, paused: bool) -> Result<(), String> {
    match paused {
        true => app.global_shortcut().unregister_all().map_err(|e| for_user("停用小窗快捷键", &e)),
        false => register(&app),
    }
}
