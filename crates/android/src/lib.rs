//! Local mode's server in the Android app (docs/local.md#android): the native methods of
//! `cn.juniexd.nexttodo.LocalServer`. An error reaches Kotlin as a `RuntimeException` with the message for
//! the user.

use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use android_logger::Config;
use anyhow::Context;
use jni::objects::{JClass, JString};
use jni::{Env, EnvUnowned};
use nexttodo_core::model::FileName;
use nexttodo_server::backup;
use nexttodo_server::error::for_user;
use nexttodo_server::local::{Local, StartError};
use serde_json::json;

/// Runs the server's tasks for as long as the process does.
static RUNTIME: LazyLock<tokio::runtime::Runtime> =
    LazyLock::new(|| tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime"));

/// The server once started, until the process ends.
static SERVER: Mutex<Option<Arc<Local>>> = Mutex::new(None);

/// Runs the native method that does `what`: an error becomes a Java exception with what to tell the
/// user (`error::for_user`), a panic one of its own.
fn run<'a, T: Default>(mut env: EnvUnowned<'a>, what: &str, f: impl FnOnce(&mut Env<'a>) -> anyhow::Result<T>) -> T {
    env.with_env(|env| -> jni::errors::Result<T> {
        f(env).or_else(|e| {
            // Throwing gives `Err(JavaException)`, which is what is wanted: the exception goes up.
            let _ = env.throw(for_user(what, e.as_ref()));
            Ok(T::default())
        })
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

fn path(env: &Env, s: &JString) -> anyhow::Result<PathBuf> {
    Ok(s.try_to_string(env)?.into())
}

/// The server, once started.
fn server() -> Option<Arc<Local>> {
    SERVER.lock().expect("server lock").clone()
}

/// Runs `f` on the server started.
fn running<T>(f: impl FnOnce(&Local) -> anyhow::Result<T>) -> anyhow::Result<T> {
    f(server().context("本机模式没有在运行")?.as_ref())
}

/// Starts the server on the data directory `dir`, the first time: `{url, user, epoch}` as JSON, or
/// `{error, locked}` when it did not start, which a later call tries again.
#[allow(unsafe_code, reason = "JNI finds a native method by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_juniexd_nexttodo_LocalServer_start<'a>(env: EnvUnowned<'a>, _: JClass<'a>, dir: JString<'a>) -> JString<'a> {
    run(env, "启动本机模式", |env| {
        // As the app's own, tagged NextTodo; at the level a server logs at.
        android_logger::init_once(Config::default().with_tag("NextTodo").with_max_level(log::LevelFilter::Info));
        let dir = path(env, &dir)?;
        let mut server = SERVER.lock().expect("server lock");
        let started = match server.take() {
            Some(local) => Ok(local),
            None => RUNTIME.block_on(Local::start(&dir)).map(Arc::new),
        };
        let reply = match started {
            Ok(local) => {
                let local = server.insert(local);
                json!({ "url": local.url, "user": local.user, "epoch": local.epoch })
            }
            Err(e) => json!({ "error": e.for_user(), "locked": matches!(e, StartError::Locked) }),
        };
        Ok(JString::from_str(env, reply.to_string())?)
    })
}

/// Signs the app in: a new session's token, the earlier sessions revoked.
#[allow(unsafe_code, reason = "JNI finds a native method by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_juniexd_nexttodo_LocalServer_session<'a>(env: EnvUnowned<'a>, _: JClass<'a>) -> JString<'a> {
    run(env, "登录本机模式", |env| {
        let token = running(|local| Ok(RUNTIME.block_on(local.session())?))?;
        Ok(JString::from_str(env, token)?)
    })
}

/// Has the server look at the drive now, when it syncs through one, waiting up to `wait_ms` for what
/// is new there to be read in (docs/local/drive.md#运行). Nothing while it is not running.
#[allow(unsafe_code, reason = "JNI finds a native method by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_juniexd_nexttodo_LocalServer_pull<'a>(env: EnvUnowned<'a>, _: JClass<'a>, wait_ms: i64) {
    run(env, "同步", |_| {
        if let Some(local) = server() {
            RUNTIME.block_on(local.pull(Duration::from_millis(wait_ms.try_into()?)));
        }
        Ok(())
    })
}

/// Whether the server syncs through a drive: what the app sends then needs the network. Not while it is
/// not running.
#[allow(unsafe_code, reason = "JNI finds a native method by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_juniexd_nexttodo_LocalServer_syncing<'a>(env: EnvUnowned<'a>, _: JClass<'a>) -> bool {
    run(env, "读取同步的状态", |_| {
        Ok(match server() {
            Some(local) => RUNTIME.block_on(local.syncing()),
            None => false,
        })
    })
}

/// Writes a backup of the data to the file `to`.
#[allow(unsafe_code, reason = "JNI finds a native method by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_juniexd_nexttodo_LocalServer_backup<'a>(env: EnvUnowned<'a>, _: JClass<'a>, to: JString<'a>) {
    run(env, "备份", |env| {
        let to = path(env, &to)?;
        running(|local| RUNTIME.block_on(local.backup(to)))
    })
}

/// Prepares the backup in the file `from` to replace the data in `dir`; works without the server,
/// which may have failed to start.
#[allow(unsafe_code, reason = "JNI finds a native method by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_juniexd_nexttodo_LocalServer_prepare<'a>(
    env: EnvUnowned<'a>,
    _: JClass<'a>,
    dir: JString<'a>,
    from: JString<'a>,
) {
    run(env, "读取备份", |env| backup::prepare(&path(env, &dir)?, &path(env, &from)?))
}

/// What an attachment called `name` is called as a file (docs/desktop.md#附件), to open or save.
#[allow(unsafe_code, reason = "JNI finds a native method by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_juniexd_nexttodo_LocalServer_fileName<'a>(
    env: EnvUnowned<'a>,
    _: JClass<'a>,
    name: JString<'a>,
) -> JString<'a> {
    run(env, "读取附件名称", |env| {
        let name = FileName::try_from(name.try_to_string(env)?).map_err(anyhow::Error::msg)?;
        Ok(JString::from_str(env, name.on_disk())?)
    })
}

/// The user's go-ahead: the backup prepared replaces the data in `dir` when the app starts next.
#[allow(unsafe_code, reason = "JNI finds a native method by its unmangled name")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_cn_juniexd_nexttodo_LocalServer_commit<'a>(env: EnvUnowned<'a>, _: JClass<'a>, dir: JString<'a>) {
    run(env, "恢复", |env| backup::commit(&path(env, &dir)?))
}
