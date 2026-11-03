//! Updates from the server the app is signed in to: a newer version is downloaded and put in place
//! of this one, and runs from the next start. See docs/macos.md#更新.

use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// Installs the version `<server>/app/macos.json` offers when it is newer, and returns it; nothing
/// when this one is the newest.
#[tauri::command]
pub async fn update(app: AppHandle, server: String) -> Result<Option<String>, String> {
    let url = format!("{}/app/macos.json", server.trim_end_matches('/')).parse().map_err(|e| format!("服务器地址无效：{e}"))?;
    let updater = app.updater_builder().endpoints(vec![url]).and_then(|b| b.build()).map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| format!("检查更新失败：{e}"))? else {
        return Ok(None);
    };
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| format!("安装更新失败：{e}"))?;
    Ok(Some(update.version))
}

/// Starts the app again, to run the version installed.
#[tauri::command]
pub fn restart(app: AppHandle) {
    app.restart();
}
