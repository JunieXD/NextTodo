//! Updates from the server the app is signed in to, or in local mode from GitHub: a newer version is
//! downloaded, and runs from the next start. See docs/desktop.md#更新 and docs/local.md#更新.

use std::sync::Mutex;

use serde::Serialize;

use tauri::{AppHandle, State, Url};
use tauri_plugin_http::reqwest;
use tauri_plugin_updater::{Update, UpdaterExt};

/// A version downloaded on Windows, installed when the app restarts: installing it there quits the
/// app at once. macOS installs it in place right away, to run from the next start.
#[derive(Default)]
pub struct Downloaded(Mutex<Option<(Update, Vec<u8>)>>);

/// Why looking for or getting a version failed, which the page tells the user in its words
/// (apps/web/src/updates.ts): the place updates come from out of reach, nothing usable there, or a
/// version downloaded that would not install. What went wrong exactly goes to the log.
#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Failure {
    Offline,
    Unavailable,
    Install,
}

impl Failure {
    /// Offline when the place could not be reached, else unavailable; logged under `doing`. Errors
    /// are logged without their address: the server's may have been typed with a password in it, and
    /// GitHub's downloads redirect to addresses signed for a while.
    fn fetching(doing: &str, unreached: bool, e: impl std::fmt::Display) -> Self {
        (if unreached { Failure::Offline } else { Failure::Unavailable }).logged(doing, e)
    }

    fn logged(self, doing: &str, e: impl std::fmt::Display) -> Self {
        tracing::warn!("{doing}：{e}");
        self
    }
}

/// Downloads the version the file at `manifest` offers when it is newer, and returns it; nothing when
/// this one is the newest. A server's file gives its downloads relative to itself, so that it does
/// not need to know its own address; GitHub's gives them in full.
#[tauri::command]
pub async fn update(app: AppHandle, downloaded: State<'_, Downloaded>, manifest: String) -> Result<Option<String>, Failure> {
    let fetched = |e: reqwest::Error| {
        let e = e.without_url();
        Failure::fetching("检查更新", e.is_connect() || e.is_timeout(), e)
    };
    let url = Url::parse(&manifest).map_err(|e| Failure::Unavailable.logged("更新地址无效", e))?;
    let res = reqwest::get(url.clone()).await.and_then(|r| r.error_for_status()).map_err(fetched)?;
    let bytes = res.bytes().await.map_err(fetched)?;
    let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| Failure::Unavailable.logged("更新信息无效", e))?;
    if let Some(platforms) = manifest["platforms"].as_object_mut() {
        for platform in platforms.values_mut() {
            let download = platform["url"]
                .as_str()
                .and_then(|u| url.join(u).ok())
                .ok_or_else(|| Failure::Unavailable.logged("更新信息无效", "下载地址无效"))?;
            platform["url"] = download.as_str().into();
        }
    }
    let updater =
        app.updater_builder().endpoints(vec![url]).and_then(|b| b.build()).map_err(|e| Failure::Unavailable.logged("检查更新", e))?;
    let Some(update) = updater.restore_update(manifest).map_err(|e| Failure::Unavailable.logged("更新信息无效", e))? else {
        return Ok(None);
    };
    // The plugin's client brings no TLS crypto, and panics with none installed: its own check
    // installs ring, which restoring an update skips. Installing fails only when one is already.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| match e {
        tauri_plugin_updater::Error::Reqwest(e) => {
            let e = e.without_url();
            Failure::fetching("下载更新", e.is_connect() || e.is_timeout(), e)
        }
        e => Failure::Install.logged("下载更新", e),
    })?;
    let version = update.version.clone();
    if cfg!(windows) {
        *downloaded.0.lock().expect("never poisoned") = Some((update, bytes));
    } else {
        update.install(bytes).map_err(|e| Failure::Install.logged("安装更新", e))?;
    }
    Ok(Some(version))
}

/// Starts the app again, running the version downloaded if there is one: on Windows its installer
/// does, once it has installed it. The app exits first as when quit, letting go of what it holds,
/// such as the single instance's socket and local mode's data directory.
#[tauri::command]
pub fn restart(app: AppHandle, downloaded: State<'_, Downloaded>) -> Result<(), Failure> {
    // Kept should installing fail, for another try.
    if let Some((update, bytes)) = downloaded.0.lock().expect("never poisoned").as_ref() {
        update.install(bytes).map_err(|e| Failure::Install.logged("安装更新", e))?;
    }
    app.request_restart();
    Ok(())
}
