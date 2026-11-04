//! Updates from the server the app is signed in to: a newer version is downloaded, and runs from the
//! next start. See docs/desktop.md#更新.

use std::sync::Mutex;

use tauri::{AppHandle, State, Url};
use tauri_plugin_http::reqwest;
use tauri_plugin_updater::{Update, UpdaterExt};

/// A version downloaded on Windows, installed when the app restarts: installing it there quits the
/// app at once. macOS installs it in place right away, to run from the next start.
#[derive(Default)]
pub struct Downloaded(Mutex<Option<(Update, Vec<u8>)>>);

/// Downloads the version `<server>/app/desktop.json` offers when it is newer, and returns it; nothing
/// when this one is the newest. The file gives its downloads relative to itself, so that a server
/// does not need to know its own address.
#[tauri::command]
pub async fn update(app: AppHandle, downloaded: State<'_, Downloaded>, server: String) -> Result<Option<String>, String> {
    let url = Url::parse(&format!("{}/app/desktop.json", server.trim_end_matches('/'))).map_err(|e| format!("服务器地址无效：{e}"))?;
    let mut manifest = manifest(&url).await.map_err(|e| format!("检查更新失败：{e}"))?;
    if let Some(platforms) = manifest["platforms"].as_object_mut() {
        for platform in platforms.values_mut() {
            let download = platform["url"].as_str().and_then(|u| url.join(u).ok()).ok_or("检查更新失败：下载地址无效")?;
            platform["url"] = download.as_str().into();
        }
    }
    let updater = app.updater_builder().endpoints(vec![url]).and_then(|b| b.build()).map_err(|e| e.to_string())?;
    let Some(update) = updater.restore_update(manifest).map_err(|e| format!("检查更新失败：{e}"))? else {
        return Ok(None);
    };
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| format!("下载更新失败：{e}"))?;
    let version = update.version.clone();
    if cfg!(windows) {
        *downloaded.0.lock().expect("never poisoned") = Some((update, bytes));
    } else {
        update.install(bytes).map_err(|e| format!("安装更新失败：{e}"))?;
    }
    Ok(Some(version))
}

async fn manifest(url: &Url) -> Result<serde_json::Value, String> {
    let res = reqwest::get(url.clone()).await.and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    serde_json::from_slice(&res.bytes().await.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Starts the app again, to run the version downloaded: on Windows its installer does, once it has
/// installed it.
#[tauri::command]
pub fn restart(app: AppHandle, downloaded: State<'_, Downloaded>) -> Result<(), String> {
    if let Some((update, bytes)) = downloaded.0.lock().expect("never poisoned").take() {
        update.install(bytes).map_err(|e| format!("安装更新失败：{e}"))?;
    }
    app.restart();
}
