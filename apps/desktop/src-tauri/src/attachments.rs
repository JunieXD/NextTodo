//! Attachments' files, which the page hands over to open with the system's default app or to save
//! where the user picks (docs/desktop.md#附件). The request's body is the file; its attachment's id
//! and name are in the headers, the name URL-encoded.

use std::path::{Path, PathBuf};

use nexttodo_core::model::{AttachmentId, FileName};
use nexttodo_server::error::for_user;
use percent_encoding::percent_decode_str;
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

/// Where files go to be opened: in the system's temporary directory, cleared as the app starts.
fn dir() -> PathBuf {
    std::env::temp_dir().join("NextTodo").join("attachments")
}

/// Deletes the files opened in earlier runs.
pub fn clear() {
    // None there, or one an app still has open on Windows: next time.
    let _ = std::fs::remove_dir_all(dir());
}

/// The file in `request`, and what it is called on the disk.
fn file<'a>(request: &'a Request<'_>) -> Result<(AttachmentId, String, &'a [u8]), String> {
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).ok_or_else(|| format!("缺少请求头 {name}"));
    let id: AttachmentId = serde_json::from_value(header("x-id")?.into()).map_err(|e| e.to_string())?;
    let name = percent_decode_str(header("x-name")?).decode_utf8().map_err(|e| e.to_string())?;
    let name = FileName::try_from(name.into_owned())?;
    let InvokeBody::Raw(bytes) = request.body() else { return Err("请求体应为文件".into()) };
    Ok((id, name.on_disk(), bytes))
}

/// Opens the file with the system's default app for its kind.
#[tauri::command]
pub async fn open_attachment(request: Request<'_>) -> Result<(), String> {
    let (id, name, bytes) = file(&request)?;
    let path = dir().join(id.to_string()).join(name);
    write(&path, bytes).map_err(|e| for_user("打开附件", &e))?;
    tauri::async_runtime::spawn_blocking(move || open::that(path))
        .await
        .map_err(|e| for_user("打开附件", &e))?
        .map_err(|_| "没有能打开这种文件的应用".into())
}

fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().expect("in the attachment's directory"))?;
    std::fs::write(path, bytes)
}

/// Asks where to save the file, offering its name, and writes it there. Where it went; nothing when the
/// user cancelled.
#[tauri::command]
pub async fn save_attachment(app: AppHandle, request: Request<'_>) -> Result<Option<String>, String> {
    let (_, name, bytes) = file(&request)?;
    let mut dialog = app.dialog().file().set_file_name(name);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    let chosen =
        tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file()).await.map_err(|e| for_user("选择保存位置", &e))?;
    let Some(to) = chosen.map(|f| f.into_path()).transpose().map_err(|e| for_user("选择保存位置", &e))? else { return Ok(None) };
    std::fs::write(&to, bytes).map_err(|e| for_user("保存附件", &e))?;
    Ok(Some(to.display().to_string()))
}
