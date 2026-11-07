//! The command line the app carries, put where a terminal finds it from the settings. See
//! docs/desktop.md#命令行.

use std::path::PathBuf;
use std::process::{Command, Output};

use nexttodo_server::error::for_user;
use serde::Serialize;

/// Where the command line stands. On macOS its place may hold another one: `by` says what, and
/// installing replaces it only when told to.
#[derive(Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum Status {
    Installed,
    Missing,
    #[cfg(target_os = "macos")]
    Taken {
        by: String,
    },
}

/// Next to the app's own executable: where `externalBin` puts it on macOS, and the directory of it
/// that `resources` makes on Windows.
fn carried(name: &str) -> Result<PathBuf, String> {
    Ok(std::env::current_exe().map_err(|e| for_user("查找命令行", &e))?.with_file_name(name))
}

#[tauri::command]
pub fn cli_status() -> Result<Status, String> {
    platform::status()
}

#[tauri::command]
pub async fn install_cli(replace: bool) -> Result<(), String> {
    // Waits on the system's password dialog on macOS: off the thread that runs the app.
    tauri::async_runtime::spawn_blocking(move || platform::install(replace)).await.map_err(|e| for_user("安装命令行", &e))?
}

/// What to tell the user when a command failed: what it said goes to the log.
fn failed(output: &Output) -> String {
    tracing::warn!("安装命令行：{}", String::from_utf8_lossy(&output.stderr).trim());
    "安装命令行失败，请稍后重试".into()
}

#[cfg(target_os = "macos")]
mod platform {
    use std::io::ErrorKind;

    use super::*;

    const LINK: &str = "/usr/local/bin/nexttodo";

    /// Links to the command line, leaving a downloaded app's quarantine as it is: once opened, the app
    /// may be changed only by programs the user allowed App Management, which this one is not, as
    /// root neither. Run from a terminal, the command line is checked by Gatekeeper as the app it is
    /// in, and runs once the user opened that (docs/desktop.md#命令行).
    const SCRIPT: &str = r#"mkdir -p /usr/local/bin && ln -sfn "$1" /usr/local/bin/nexttodo"#;

    pub fn status() -> Result<Status, String> {
        let link = match std::fs::symlink_metadata(LINK) {
            Ok(m) if m.is_symlink() => std::fs::read_link(LINK).map_err(|e| for_user("读取命令行的状态", &e))?,
            Ok(_) => return Ok(Status::Taken { by: "是一个文件".into() }),
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Status::Missing),
            Err(e) => return Err(for_user("读取命令行的状态", &e)),
        };
        Ok(match link == carried("nexttodo")? {
            true => Status::Installed,
            false => Status::Taken { by: format!("指向 {}", link.display()) },
        })
    }

    pub fn install(replace: bool) -> Result<(), String> {
        if let Status::Taken { by } = status()?
            && !replace
        {
            return Err(format!("{LINK} 已存在，{by}"));
        }
        let carried = carried("nexttodo")?;
        // Opened from the disk image or Downloads, the app runs from a random path that goes away.
        if carried.to_string_lossy().contains("/AppTranslocation/") {
            return Err("请先把 NextTodo 移到“应用程序”文件夹，再安装命令行".into());
        }
        let run = |command: &mut Command| command.arg(&carried).output().map_err(|e| for_user("安装命令行", &e));
        if run(Command::new("/bin/sh").args(["-c", SCRIPT, "sh"]))?.status.success() {
            return Ok(());
        }
        // Only an administrator may write there on most Macs: the system asks for the password.
        let ask = r#"do shell script "/bin/sh -c " & quoted form of item 1 of argv & " sh " & quoted form of item 2 of argv with prompt "NextTodo 要安装命令行 nexttodo。" with administrator privileges"#;
        let output = run(Command::new("/usr/bin/osascript").args(["-e", "on run argv", "-e", ask, "-e", "end run", SCRIPT]))?;
        match output.status.success() {
            true => Ok(()),
            false if String::from_utf8_lossy(&output.stderr).contains("(-128)") => Err("已取消安装命令行".into()),
            false => Err(failed(&output)),
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::process::CommandExt;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    use super::*;

    /// Puts `NEXTTODO_BIN` first in the user's PATH, ahead of a command line installed otherwise, as the
    /// registry has it, `%variables%` unexpanded; then tells the programs running, Explorer among
    /// them, as the system's own settings do.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
$dirs = @($key.GetValue('Path', '', 'DoNotExpandEnvironmentNames') -split ';' | Where-Object { $_ })
if ($dirs -notcontains $env:NEXTTODO_BIN) {
  $key.SetValue('Path', (@($env:NEXTTODO_BIN) + $dirs) -join ';', 'ExpandString')
  Add-Type -Namespace NextTodo -Name User32 -MemberDefinition '[DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessageTimeout(IntPtr window, uint message, UIntPtr w, string l, uint flags, uint timeout, out UIntPtr result);'
  $result = [UIntPtr]::Zero
  # WM_SETTINGCHANGE to every window, giving up on those that hang.
  [void][NextTodo.User32]::SendMessageTimeout([IntPtr]0xffff, 0x1a, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result)
}"#;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn status() -> Result<Status, String> {
        let bin = carried("bin")?;
        let path = match windows_registry::CURRENT_USER.open("Environment").and_then(|k| k.get_string("Path")) {
            Ok(path) => path,
            // ERROR_FILE_NOT_FOUND: no PATH of the user's own yet.
            Err(e) if e.code().0 as u32 == 0x8007_0002 => String::new(),
            Err(e) => return Err(for_user("读取命令行的状态", &e)),
        };
        let on = path.split(';').any(|dir| dir.trim_end_matches('\\').eq_ignore_ascii_case(&bin.to_string_lossy()));
        Ok(if on { Status::Installed } else { Status::Missing })
    }

    /// Adding to PATH replaces nothing: `replace` has nothing to do here.
    pub fn install(_replace: bool) -> Result<(), String> {
        // As UTF-16 in base64, which no quoting on the command line can garble.
        let script = STANDARD.encode(SCRIPT.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>());
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &script])
            .env("NEXTTODO_BIN", carried("bin")?)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| for_user("安装命令行", &e))?;
        if output.status.success() { Ok(()) } else { Err(failed(&output)) }
    }
}
