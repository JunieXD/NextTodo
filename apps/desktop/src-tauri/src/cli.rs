//! The command line the app carries, put where a terminal finds it from the settings. See
//! docs/desktop.md#命令行.

use std::path::PathBuf;
use std::process::Command;

/// Next to the app's own executable: where `externalBin` puts it on macOS, and the directory of it
/// that `resources` makes on Windows.
fn carried(name: &str) -> Result<PathBuf, String> {
    Ok(std::env::current_exe().map_err(|e| e.to_string())?.with_file_name(name))
}

#[tauri::command]
pub fn cli_installed() -> Result<bool, String> {
    platform::installed()
}

#[tauri::command]
pub async fn install_cli() -> Result<(), String> {
    // Waits on the system's password dialog on macOS: off the thread that runs the app.
    tauri::async_runtime::spawn_blocking(platform::install).await.map_err(|e| e.to_string())?
}

/// What went wrong, from a command's standard error.
fn failed(output: &std::process::Output) -> String {
    format!("安装失败：{}", String::from_utf8_lossy(&output.stderr).trim())
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    const LINK: &str = "/usr/local/bin/nexttodo";

    /// Takes the quarantine off the app, then links to its command line. A downloaded app has it: run
    /// from a terminal, the command line would be stopped, waiting on the system's dialog. The user
    /// opened the app already.
    const SCRIPT: &str = r#"set -e
xattr -dr com.apple.quarantine "${1%/Contents/MacOS/nexttodo}"
mkdir -p /usr/local/bin
ln -sfn "$1" /usr/local/bin/nexttodo"#;

    pub fn installed() -> Result<bool, String> {
        let carried = carried("nexttodo")?;
        // Missing, a file of its own or a link elsewhere: not this app's.
        Ok(std::fs::read_link(LINK).is_ok_and(|to| to == carried))
    }

    pub fn install() -> Result<(), String> {
        let carried = carried("nexttodo")?;
        // Opened from the disk image or Downloads, the app runs from a random path that goes away.
        if carried.to_string_lossy().contains("/AppTranslocation/") {
            return Err("请先把 NextTodo 移到“应用程序”文件夹，再安装命令行".into());
        }
        let run = |command: &mut Command| command.arg(&carried).output().map_err(|e| e.to_string());
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

    use super::*;

    /// Adds `NEXTTODO_BIN` to the user's PATH as the registry has it, `%variables%` unexpanded, then
    /// lets the programs running, Explorer among them, know: setting a variable through .NET does.
    const SCRIPT: &str = r#"
$key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
$dirs = @($key.GetValue('Path', '', 'DoNotExpandEnvironmentNames') -split ';' | Where-Object { $_ })
if ($dirs -notcontains $env:NEXTTODO_BIN) {
  $key.SetValue('Path', ($dirs + $env:NEXTTODO_BIN) -join ';', 'ExpandString')
  [Environment]::SetEnvironmentVariable('NEXTTODO_BIN', $env:NEXTTODO_BIN, 'User')
  [Environment]::SetEnvironmentVariable('NEXTTODO_BIN', $null, 'User')
}"#;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn installed() -> Result<bool, String> {
        let bin = carried("bin")?;
        let path = match windows_registry::CURRENT_USER.open("Environment").and_then(|k| k.get_string("Path")) {
            Ok(path) => path,
            // ERROR_FILE_NOT_FOUND: no PATH of the user's own yet.
            Err(e) if e.code().0 as u32 == 0x8007_0002 => String::new(),
            Err(e) => return Err(e.to_string()),
        };
        Ok(path.split(';').any(|dir| dir.trim_end_matches('\\').eq_ignore_ascii_case(&bin.to_string_lossy())))
    }

    pub fn install() -> Result<(), String> {
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
            .env("NEXTTODO_BIN", carried("bin")?)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if output.status.success() { Ok(()) } else { Err(failed(&output)) }
    }
}
