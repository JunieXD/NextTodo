//! Local mode: the server inside the app, which the page starts and signs in to with `start_local`.
//! See docs/local.md.

use std::path::PathBuf;
use std::time::Duration;

use nexttodo_server::backup;
use nexttodo_server::error::for_user;
use nexttodo_server::local::{Local, StartError};
use serde::Serialize;
use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};

/// The server once started, for as long as the app runs, and the session the app's windows share.
#[derive(Default)]
pub struct Server(Mutex<Running>);

#[derive(Default)]
struct Running {
    local: Option<Local>,
    token: Option<String>,
}

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
        Failed { locked: matches!(e, StartError::Locked), message: e.for_user() }
    }
}

/// The data directory, `local/` in the app's.
fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| for_user("查找 App 的数据目录", &e))?.join("local"))
}

/// The file that is there while the app is in local mode: writing or removing it is the switch. Not
/// in the page's `localStorage`, which WebKit writes to disk a while later: the app restarts right
/// after leaving local mode, and would come back in it.
const ENABLED: &str = "enabled";

#[tauri::command]
pub fn local_mode(app: AppHandle) -> Result<bool, String> {
    dir(&app)?.join(ENABLED).try_exists().map_err(|e| for_user("读取本机模式的设置", &e))
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
    done.map_err(|e| for_user("切换本机模式", &e))
}

/// Starts the server the first time, and signs the page in with the session of the app's windows: a
/// new one, the earlier ones revoked, the first time or when told to `renew` it. Calls take turns; a
/// start that failed is tried again by the next one.
#[tauri::command]
pub async fn start_local(app: AppHandle, server: State<'_, Server>, renew: bool) -> Result<Started, Failed> {
    let mut running = server.0.lock().await;
    let running = &mut *running;
    let local = match &mut running.local {
        Some(local) => local,
        None => running.local.insert(Local::start(&dir(&app).map_err(|message| Failed { message, locked: false })?).await?),
    };
    let token = match &running.token {
        Some(token) if !renew => token.clone(),
        _ => running.token.insert(local.session().await.map_err(|e| Failed { message: e.describe(), locked: false })?).clone(),
    };
    Ok(Started { url: local.url.clone(), token, user: local.user.clone(), epoch: local.epoch.clone() })
}

/// Has the server look at the drive now, when it syncs through one, as a window of the app comes to the
/// front (docs/local.md#运行).
pub fn focused(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(local) = &app.state::<Server>().0.lock().await.local {
            local.pull(Duration::ZERO).await;
        }
    });
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
        .map_err(|e| for_user("选择文件", &e))?;
    chosen.map(|file| file.into_path().map_err(|e| for_user("选择文件", &e))).transpose()
}

/// Asks where to save a backup, offering `name`, and writes it there. Where it went; nothing when the
/// user cancelled.
#[tauri::command]
pub async fn backup_local(app: AppHandle, server: State<'_, Server>, name: String) -> Result<Option<String>, String> {
    let Some(to) = ask(dialog(&app).set_file_name(name), true).await? else { return Ok(None) };
    let running = server.0.lock().await;
    let local = running.local.as_ref().ok_or("本机模式没有在运行")?;
    local.backup(to.clone()).await.map_err(|e| for_user("备份", e.as_ref()))?;
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
        .map_err(|e| for_user("读取备份", &e))?
        .map_err(|e| for_user("读取备份", e.as_ref()))?;
    Ok(true)
}

/// The user's go-ahead: the backup prepared replaces the data when the app starts next.
#[tauri::command]
pub fn commit_restore(app: AppHandle) -> Result<(), String> {
    backup::commit(&dir(&app)?).map_err(|e| for_user("恢复", e.as_ref()))
}
