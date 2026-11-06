//! Local mode: the server inside the app, which the page starts and signs in to with `start_local`.
//! See docs/local.md.

use std::path::PathBuf;

use nexttodo_server::local::{Local, StartError};
use serde::Serialize;
use tauri::async_runtime::Mutex;
use tauri::{AppHandle, Manager, State};

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
