//! Local mode's server in the iOS app (docs/ios.md#本机模式): the C functions `LocalServer.swift` calls.
//! Those that may fail answer with a C string of JSON, `{"value": …}` or `{"error": …}` with the
//! message for the user, which the app hands back to `nexttodo_free`.

use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Once};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use nexttodo_core::model::FileName;
use nexttodo_server::backup;
use nexttodo_server::error::for_user;
use nexttodo_server::local::{Local, StartError};
use serde_json::{Value, json};
use tokio::runtime::{Handle, Runtime};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// The server once started, until stopped, with the runtime its tasks run on: stopping the runtime
/// stops them all. iOS apps cannot start afresh, so the app stops and starts the server instead.
static SERVER: Mutex<Option<(Runtime, Arc<Local>)>> = Mutex::new(None);

static LOG: Once = Once::new();

/// The string the app passed at `s`, which is NUL-terminated and lives through the call.
#[allow(unsafe_code, reason = "a C string from the app")]
fn string(s: *const c_char) -> anyhow::Result<String> {
    // SAFETY: the app passes a valid C string, which it keeps for the call.
    Ok(unsafe { CStr::from_ptr(s) }.to_str()?.to_owned())
}

fn answer(reply: Value) -> *mut c_char {
    CString::new(reply.to_string()).expect("JSON has no NUL").into_raw()
}

/// Answers a call that does `what`: its value, or what to tell the user (`error::for_user`).
fn reply(what: &str, f: impl FnOnce() -> anyhow::Result<Value>) -> *mut c_char {
    answer(match f() {
        Ok(value) => json!({ "value": value }),
        Err(e) => json!({ "error": for_user(what, e.as_ref()) }),
    })
}

/// The server, once started, and where to run its work.
fn running() -> anyhow::Result<(Handle, Arc<Local>)> {
    let server = SERVER.lock().expect("server lock");
    let (runtime, local) = server.as_ref().context("本机模式没有在运行")?;
    Ok((runtime.handle().clone(), local.clone()))
}

/// Starts the server on the data directory `dir`, the first time since it last stopped, at `port`, 0
/// for one the system picks: `{url, user, epoch}`, or `{error, locked}` when it did not start, which a
/// later call tries again.
#[allow(unsafe_code, reason = "C calls it by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "C" fn nexttodo_start(dir: *const c_char, port: u16) -> *mut c_char {
    answer(match start(dir, port) {
        Ok(local) => json!({ "value": { "url": local.url, "user": local.user, "epoch": local.epoch } }),
        Err(e) => json!({ "error": e.for_user(), "locked": matches!(e, StartError::Locked) }),
    })
}

fn start(dir: *const c_char, port: u16) -> Result<Arc<Local>, StartError> {
    // As the app's own, in its subsystem; at the level a server logs at.
    LOG.call_once(|| {
        tracing_subscriber::registry().with(LevelFilter::INFO).with(tracing_oslog::OsLogger::new("cn.juniexd.nexttodo", "server")).init();
    });
    let dir = PathBuf::from(string(dir)?);
    let mut server = SERVER.lock().expect("server lock");
    if let Some((_, local)) = server.as_ref() {
        return Ok(local.clone());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let local = Arc::new(runtime.block_on(Local::start_at(&dir, port))?);
    *server = Some((runtime, local.clone()));
    Ok(local)
}

/// Stops the server and its tasks. What runs on the runtime stops at once, as when the other apps'
/// process ends: all of it can be cut short (docs/ios.md#本机模式). What runs on blocking threads runs
/// on until it returns, and may use the data: the data directory stays locked until it has, and the
/// calls under way are done, in the background. Starting again waits for that, as a new process of
/// the other apps waits for the old one.
#[allow(unsafe_code, reason = "C calls it by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "C" fn nexttodo_stop() {
    let stopped = SERVER.lock().expect("server lock").take();
    if let Some((runtime, local)) = stopped {
        // Dropping the runtime waits for its blocking threads; the lock goes with `local`, after.
        thread::spawn(move || {
            drop(runtime);
            drop(local);
        });
    }
}

/// Signs the app in: a new session's token, the earlier sessions revoked.
#[allow(unsafe_code, reason = "C calls it by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "C" fn nexttodo_session() -> *mut c_char {
    reply("登录本机模式", || {
        let (runtime, local) = running()?;
        Ok(runtime.block_on(local.session())?.into())
    })
}

/// Has the server look at the drive now, when it syncs through one, waiting up to `wait_ms` for what
/// is new there to be read in (docs/local/drive.md#运行). Nothing while it is not running.
#[allow(unsafe_code, reason = "C calls it by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "C" fn nexttodo_pull(wait_ms: u64) {
    if let Ok((runtime, local)) = running() {
        runtime.block_on(local.pull(Duration::from_millis(wait_ms)));
    }
}

/// Whether the server, once started, is still the one at its address: the system may have given its
/// port to another program while the app was suspended (`Local::answers`). False when not running.
#[allow(unsafe_code, reason = "C calls it by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "C" fn nexttodo_answers() -> bool {
    running().is_ok_and(|(runtime, local)| runtime.block_on(local.answers()))
}

/// Writes a backup of the data to the file `to`.
#[allow(unsafe_code, reason = "C calls it by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "C" fn nexttodo_backup(to: *const c_char) -> *mut c_char {
    reply("备份", || {
        let to = PathBuf::from(string(to)?);
        let (runtime, local) = running()?;
        runtime.block_on(local.backup(to))?;
        Ok(Value::Null)
    })
}

/// Prepares the backup in the file `from` to replace the data in `dir`; works without the server,
/// which may have failed to start.
#[allow(unsafe_code, reason = "C calls it by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "C" fn nexttodo_prepare(dir: *const c_char, from: *const c_char) -> *mut c_char {
    reply("读取备份", || {
        backup::prepare(&PathBuf::from(string(dir)?), &PathBuf::from(string(from)?))?;
        Ok(Value::Null)
    })
}

/// The user's go-ahead: the backup prepared replaces the data in `dir` when the server starts next.
#[allow(unsafe_code, reason = "C calls it by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "C" fn nexttodo_commit(dir: *const c_char) -> *mut c_char {
    reply("恢复", || {
        backup::commit(&PathBuf::from(string(dir)?))?;
        Ok(Value::Null)
    })
}

/// What an attachment called `name` is called as a file (docs/desktop.md#附件), to open or save.
#[allow(unsafe_code, reason = "C calls it by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "C" fn nexttodo_file_name(name: *const c_char) -> *mut c_char {
    reply("读取附件名称", || Ok(FileName::try_from(string(name)?).map_err(anyhow::Error::msg)?.on_disk().into()))
}

/// Frees a string this library answered with.
///
/// # Safety
///
/// `s` is one of those strings, each handed back once.
#[allow(unsafe_code, reason = "C calls it by its unmangled name, with a string of ours")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nexttodo_free(s: *mut c_char) {
    // SAFETY: as the caller promises.
    drop(unsafe { CString::from_raw(s) });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[allow(unsafe_code, reason = "frees what the library answered with")]
    fn take(s: *mut c_char) -> Value {
        // SAFETY: an answer of the library's, handed back once.
        let text = unsafe { CStr::from_ptr(s) }.to_str().unwrap().to_owned();
        unsafe { nexttodo_free(s) };
        serde_json::from_str(&text).unwrap()
    }

    /// Stopped, the server lets go of its data and its port: it starts again there, the same data. Work
    /// left on a blocking thread keeps the data until it ends.
    #[test]
    fn the_server_starts_again_where_it_stopped() {
        let dir = std::env::temp_dir().join(format!("nexttodo-ios-{}", uuid::Uuid::now_v7()));
        let path = CString::new(dir.to_str().unwrap()).unwrap();
        let first = take(nexttodo_start(path.as_ptr(), 0))["value"].clone();
        assert!(nexttodo_answers());
        let session = take(nexttodo_session());
        assert!(session["value"].is_string());
        let port: u16 = first["url"].as_str().unwrap().rsplit_once(':').unwrap().1.parse().unwrap();
        nexttodo_stop();
        assert!(take(nexttodo_session())["error"].is_string());
        assert!(!nexttodo_answers());
        let again = take(nexttodo_start(path.as_ptr(), port))["value"].clone();
        assert_eq!(again, first);

        let (started, running_now) = mpsc::channel();
        let (release, held) = mpsc::channel::<()>();
        running().unwrap().0.spawn_blocking(move || {
            started.send(()).unwrap();
            held.recv().unwrap();
        });
        running_now.recv().unwrap();
        nexttodo_stop();
        let lock = std::fs::File::open(dir.join("lock")).unwrap();
        assert!(matches!(lock.try_lock(), Err(std::fs::TryLockError::WouldBlock)));
        release.send(()).unwrap();
        assert!(take(nexttodo_start(path.as_ptr(), port))["value"].is_object());
        nexttodo_stop();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
