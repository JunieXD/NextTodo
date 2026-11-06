//! Local mode: the server inside the app, which the page starts and signs in to with `start_local`.
//! See docs/local.md.

use std::path::PathBuf;

use nexttodo_server::backup;
use nexttodo_server::local::{Local, StartError};
use serde::Serialize;
use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};

/// The server once started, for as long as the app runs.
#[derive(Default)]
pub struct Server(Mutex<Option<Local>>);

/// What the page needs to open the local user's data.
#[derive(Serialize)]
pub struct Started {
    url: String,
    token: String,
    user: String,
    epoch: String,
}

/// Why it did not start; `locked` while an earlier process of the app still has the data.
#[derive(Serialize)]
pub struct Failed {
    message: String,
    locked: bool,
}

impl From<StartError> for Failed {
    fn from(e: StartError) -> Self {
        Failed { locked: matches!(e, StartError::Locked), message: e.to_string() }
    }
}

/// The data directory, `local/` in the app's.
fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("local"))
}

/// The file that is there while the app is in local mode: writing or removing it is the switch. Not
/// in the page's `localStorage`, which WebKit writes to disk a while later: the app restarts right
/// after leaving local mode, and would come back in it.
const ENABLED: &str = "enabled";

#[tauri::command]
pub fn local_mode(app: AppHandle) -> Result<bool, String> {
    dir(&app)?.join(ENABLED).try_exists().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_local_mode(app: AppHandle, on: bool) -> Result<(), String> {
    let dir = dir(&app)?;
    let done = match on {
        true => std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(ENABLED), "")),
        false => match std::fs::remove_file(dir.join(ENABLED)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            done => done,
        },
    };
    done.map_err(|e| format!("无法切换本机模式：{e}"))
}

/// Starts the server the first time, and signs the page in: each call a new session, the earlier
/// ones revoked. Calls take turns; a start that failed is tried again by the next one.
#[tauri::command]
pub async fn start_local(app: AppHandle, server: State<'_, Server>) -> Result<Started, Failed> {
    let mut running = server.0.lock().await;
    let local = match running.take() {
        Some(local) => local,
        None => Local::start(&dir(&app).map_err(|message| Failed { message, locked: false })?).await?,
    };
    let local = running.insert(local);
    let token = local.session().await.map_err(|e| Failed { message: e.to_string(), locked: false })?;
    Ok(Started { url: local.url.clone(), token, user: local.user.clone(), epoch: local.epoch.clone() })
}

/// The file dialog, attached to the window, for backups.
fn dialog(app: &AppHandle) -> FileDialogBuilder<tauri::Wry> {
    let dialog = app.dialog().file().add_filter("NextTodo 备份", &["db"]);
    match app.get_webview_window("main") {
        Some(window) => dialog.set_parent(&window),
        None => dialog,
    }
}

/// The file the user picks, to open or to save as; none when they cancel.
async fn ask(dialog: FileDialogBuilder<tauri::Wry>, save: bool) -> Result<Option<PathBuf>, String> {
    let chosen = tauri::async_runtime::spawn_blocking(move || if save { dialog.blocking_save_file() } else { dialog.blocking_pick_file() })
        .await
        .map_err(|e| e.to_string())?;
    chosen.map(|file| file.into_path().map_err(|e| e.to_string())).transpose()
}

/// Asks where to save a backup, offering `name`, and writes it there. Where it went; nothing when the
/// user cancelled.
#[tauri::command]
pub async fn backup_local(app: AppHandle, server: State<'_, Server>, name: String) -> Result<Option<String>, String> {
    let Some(to) = ask(dialog(&app).set_file_name(name), true).await? else { return Ok(None) };
    let running = server.0.lock().await;
    let local = running.as_ref().ok_or("本机模式没有在运行")?;
    local.backup(to.clone()).await.map_err(|e| format!("备份失败：{e:#}"))?;
    Ok(Some(to.display().to_string()))
}

/// Asks for a backup, and prepares it to replace the data; whether the user picked one. Works without
/// the server, which may have failed to start.
#[tauri::command]
pub async fn prepare_restore(app: AppHandle) -> Result<bool, String> {
    let Some(from) = ask(dialog(&app), false).await? else { return Ok(false) };
    let dir = dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || backup::prepare(&dir, &from))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))?;
    Ok(true)
}

/// The user's go-ahead: the backup prepared replaces the data when the app starts next.
#[tauri::command]
pub fn commit_restore(app: AppHandle) -> Result<(), String> {
    backup::commit(&dir(&app)?).map_err(|e| format!("恢复失败：{e:#}"))
}
