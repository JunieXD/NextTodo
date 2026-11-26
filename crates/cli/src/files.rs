//! Files: attached and downloaded, and all of one's data moved out and back in (`export`, `import`).

use crate::printable;
use crate::server::{Server, TRANSFER_TIMEOUT, connect, reply};
use crate::{Out, here};
use anyhow::{Context, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use nexttodo_core::model::{AttachmentId, BACKUP_FORMAT, FileName, MAX_FILE_SIZE};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Attaches each file to `task`, one change each, as `name` or the file's own. With `--json`, what each
/// change did, as one array: those made before one failed too.
pub fn attach(task: &str, files: &[PathBuf], name: Option<String>, out: &Out) -> anyhow::Result<()> {
    if name.is_some() && files.len() > 1 {
        bail!("--name 只能和一个文件一起给");
    }
    if out.request_id.is_some() && files.len() > 1 {
        bail!("--request-id 只能和一个文件一起给：每个文件是一次改动");
    }
    let (mut server, token) = connect()?;
    // 20 MB as base64 over a slow line.
    server.timeout = Duration::from_secs(600);
    let mut made = Vec::new();
    let attached = files.iter().try_for_each(|file| {
        let (text, data) =
            attach_one(&server, &token, task, file, name.as_deref(), &out.change()).with_context(|| file.display().to_string())?;
        if !out.json {
            println!("{}", printable(&text));
        }
        made.push(data);
        anyhow::Ok(())
    });
    if out.json {
        println!("{}", serde_json::to_string_pretty(&made)?);
    }
    attached
}

/// Attaches `file` to `task` as change `change`, as `name` or its own.
fn attach_one(server: &Server, token: &str, task: &str, file: &Path, name: Option<&str>, change: &str) -> anyhow::Result<(String, Value)> {
    // No more than one byte past the limit is read, whatever the file's size.
    let mut bytes = Vec::new();
    File::open(file).and_then(|f| f.take(u64::from(MAX_FILE_SIZE) + 1).read_to_end(&mut bytes)).context("无法读取")?;
    if bytes.len() > MAX_FILE_SIZE as usize {
        bail!("超过 20 MB");
    }
    let name = match name {
        Some(n) => n.to_owned(),
        None => file.file_name().context("文件路径没有文件名")?.to_string_lossy().into_owned(),
    };
    let mut args = json!({ "task": task, "name": name, "data": BASE64.encode(&bytes) });
    here(&mut args);
    server.tool("attach", token, Some(change), &args)
}

/// Saves attachment `id`'s file to `output`: a file, or a directory to put it in under the
/// attachment's name, as the server gives it. Never over a file already there.
pub fn download(id: &str, output: Option<PathBuf>, json_out: bool) -> anyhow::Result<()> {
    let (mut server, token) = connect()?;
    server.timeout = Duration::from_secs(600);
    let mut res = server.get(&format!("/api/v1/files/{id}"), &token)?;
    let name = named(&res).context("服务器回复缺少附件的名称")?.map_err(|e| anyhow!("附件的名称无效：{e}"))?;
    let path = match output {
        Some(dir) if dir.is_dir() => dir.join(name.on_disk()),
        Some(p) => p,
        None => PathBuf::from(name.on_disk()),
    };
    let mut file =
        std::fs::OpenOptions::new().write(true).create_new(true).open(&path).with_context(|| format!("无法写入 {}", path.display()))?;
    let copied = std::io::copy(&mut res.body_mut().as_reader(), &mut file);
    if let Err(e) = copied {
        // Half a file is no use; it is the one just made.
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(anyhow::Error::new(e).context("下载中断"));
    }
    if json_out {
        println!("{}", serde_json::to_string_pretty(&json!({ "id": id, "name": name.as_str(), "path": path }))?);
    } else {
        println!("{}", printable(&format!("已下载到 {}", path.display())));
    }
    Ok(())
}

/// The name a download is to be saved under, as the server gives it: none when it gives none, an error
/// when the name won't do.
fn named(res: &ureq::http::Response<ureq::Body>) -> Option<Result<FileName, String>> {
    let disposition = res.headers().get("content-disposition").and_then(|v| v.to_str().ok()).unwrap_or_default();
    disposition.split_once("filename*=UTF-8''").and_then(|(_, n)| decode(n)).map(FileName::try_from)
}

/// What to say where local mode has no export or import.
const LOCAL_BACKUP: &str = "本机模式用 App 设置里的“备份”和“恢复”";

/// Saves the account's own data as a backup to `file`, or here under the name the server gives it
/// (docs/local.md#导出); written in full beside it first, and never over a file already there, nor
/// over one another export is writing.
pub fn export(file: Option<PathBuf>, json_out: bool) -> anyhow::Result<()> {
    let (mut server, token) = connect()?;
    if server.local {
        bail!(LOCAL_BACKUP);
    }
    server.timeout = TRANSFER_TIMEOUT;
    let mut res = server.get(&format!("/api/v1/export?today={}", chrono::Local::now().format("%Y-%m-%d")), &token)?;
    let path = match file {
        Some(p) => p,
        None => PathBuf::from(named(&res).context("服务器回复缺少文件名")?.map_err(|e| anyhow!("文件名无效：{e}"))?.on_disk()),
    };
    if path.try_exists()? {
        bail!("{} 已存在", path.display());
    }
    let mut partial = path.clone().into_os_string();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let mut out = match std::fs::OpenOptions::new().write(true).create_new(true).open(&partial) {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            bail!("{} 已存在：可能另一个导出正在写它，或是上次中断时留下的，确认后删掉再试", partial.display())
        }
        opened => opened.with_context(|| format!("无法写入 {}", partial.display()))?,
    };
    let written = std::io::copy(&mut res.body_mut().as_reader(), &mut out).context("下载中断").and_then(|_| Ok(out.sync_all()?));
    drop(out);
    let published = written.and_then(|()| {
        // Made meanwhile, by another export say: that one stays.
        if path.try_exists()? {
            bail!("{} 已存在", path.display());
        }
        Ok(std::fs::rename(&partial, &path)?)
    });
    if let Err(e) = published {
        // Half a backup is no use.
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    if json_out {
        println!("{}", serde_json::to_string_pretty(&json!({ "path": path }))?);
    } else {
        println!("{}", printable(&format!("已导出到 {}", path.display())));
    }
    Ok(())
}

const NOT_A_BACKUP: &str = "这不是 NextTodo 的备份，或者文件已损坏";
const DATABASE_TOO_LARGE: &str = "备份里的数据库超过 20 MB，暂时导入不了";

/// Checks the backup in `file` whole, as a restore checks it, before anything of it is sent
/// (docs/local.md#导入): a tar of its manifest, then its attachments' files and database; or, `true`,
/// a database alone, as backups were before there were attachments.
fn check_backup(file: &mut File) -> anyhow::Result<bool> {
    let mut head = [0; 16];
    if file.read_exact(&mut head).is_ok() && &head == b"SQLite format 3\0" {
        if file.metadata()?.len() > u64::from(MAX_FILE_SIZE) {
            bail!(DATABASE_TOO_LARGE);
        }
        return Ok(true);
    }
    file.rewind()?;
    let mut archive = tar::Archive::new(file);
    // As they are: long names and extended headers, read in whole otherwise, are not a backup's.
    let mut entries = archive.entries().context(NOT_A_BACKUP)?.raw(true);
    let mut manifest = entries.next().context(NOT_A_BACKUP)?.context(NOT_A_BACKUP)?;
    let plain = manifest.header().entry_type().is_file();
    if !plain || manifest.path().context(NOT_A_BACKUP)?.to_str() != Some("backup.json") || manifest.size() > 1024 {
        bail!(NOT_A_BACKUP);
    }
    let mut text = String::new();
    manifest.read_to_string(&mut text).context(NOT_A_BACKUP)?;
    let format = serde_json::from_str::<Value>(&text).ok().and_then(|m| m["format"].as_i64()).context(NOT_A_BACKUP)?;
    if format > BACKUP_FORMAT {
        bail!("备份来自更新版本的 NextTodo，请先更新命令行");
    }
    let (mut files, mut database) = (HashSet::new(), false);
    for entry in entries {
        let entry = entry.context(NOT_A_BACKUP)?;
        let name = entry.path().context(NOT_A_BACKUP)?.to_str().context(NOT_A_BACKUP)?.to_owned();
        let size = entry.size();
        match name.strip_prefix("files/") {
            _ if !entry.header().entry_type().is_file() => bail!(NOT_A_BACKUP),
            Some(id) if attachment(id).is_some_and(|id| files.insert(id)) && size <= u64::from(MAX_FILE_SIZE) => {}
            None if name == "nexttodo.db" && !database && size > u64::from(MAX_FILE_SIZE) => bail!(DATABASE_TOO_LARGE),
            None if name == "nexttodo.db" && !database => database = true,
            _ => bail!(NOT_A_BACKUP),
        }
    }
    if !database {
        bail!(NOT_A_BACKUP);
    }
    Ok(false)
}

/// The attachment a backup's `files/<id>` is the file of, one id however it is written.
fn attachment(id: &str) -> Option<AttachmentId> {
    uuid::Uuid::parse_str(id).ok().and_then(|u| AttachmentId::try_from(u).ok())
}

/// At most `MAX_FILE_SIZE` bytes of `from`: more is not what the backup was checked to have.
fn read_up_to_limit(from: impl Read) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    from.take(u64::from(MAX_FILE_SIZE) + 1).read_to_end(&mut bytes).context(NOT_A_BACKUP)?;
    if bytes.len() > MAX_FILE_SIZE as usize {
        bail!(NOT_A_BACKUP);
    }
    Ok(bytes)
}

/// Adds the backup at `file` to the account (docs/local.md#导入): checked whole, each of its files
/// uploaded under a new id, made in their order, then the import asked for. What is checked is what
/// is sent: read twice through the one opening of it.
pub fn import(file: &Path, out: &Out) -> anyhow::Result<()> {
    let (mut server, token) = connect()?;
    if server.local {
        bail!(LOCAL_BACKUP);
    }
    server.timeout = TRANSFER_TIMEOUT;
    let mut opened = File::open(file).context("无法读取备份文件")?;
    let bare = check_backup(&mut opened)?;
    opened.rewind()?;
    let (mut database, mut files) = (None, Map::new());
    // The attachments' types come from the backup's database.
    let mut upload = |old: Option<AttachmentId>, bytes: &[u8]| {
        let id = uuid::Uuid::now_v7().to_string();
        server.put_file(&token, &id, if old.is_some() { "application/octet-stream" } else { "application/vnd.sqlite3" }, bytes)?;
        match old {
            Some(old) => _ = files.insert(old.to_string(), id.into()),
            None => database = Some(id),
        }
        anyhow::Ok(())
    };
    if bare {
        upload(None, &read_up_to_limit(&mut opened)?)?;
    } else {
        let mut archive = tar::Archive::new(&mut opened);
        for entry in archive.entries().context(NOT_A_BACKUP)?.raw(true).skip(1) {
            let entry = entry.context(NOT_A_BACKUP)?;
            let name = entry.path().context(NOT_A_BACKUP)?.to_string_lossy().into_owned();
            let old = name.strip_prefix("files/").map(|id| attachment(id).context(NOT_A_BACKUP)).transpose()?;
            upload(old, &read_up_to_limit(entry)?)?;
        }
    }
    let change = out.change();
    let body = json!({ "id": change, "database": database.context(NOT_A_BACKUP)?, "files": files });
    let (status, text) = server
        .send("/api/v1/import", Some(&token), None, &body)
        .map_err(|e| anyhow!("{:#}；导入可能已经完成，重发时加上 --request-id {change}，不会重复导入", server.unreachable(e)))?;
    let made = reply(status, &text)?;
    if out.json {
        println!("{}", serde_json::to_string_pretty(&made)?);
    } else {
        println!(
            "已导入 {} 个事项、{} 个文件夹，批次 {}；撤销用 nexttodo undo {}",
            made["tasks"], made["folders"], made["batch"], made["batch"]
        );
    }
    Ok(())
}

/// Text percent-encoded, as in `filename*`; `None` when it isn't.
fn decode(s: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut rest = s.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        if b == b'%' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(b);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}
