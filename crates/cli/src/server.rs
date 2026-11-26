//! Where the server is and calling it: the saved sign-in, local mode's address, requests and their replies.

use crate::printable;
use anyhow::{Context, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ureq::http::StatusCode;

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub url: String,
    pub token: String,
}

fn config_path() -> anyhow::Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::home_dir().context("无法找到用户主目录")?.join(".config"),
    };
    Ok(base.join("nexttodo").join("config.json"))
}

pub fn read_config() -> anyhow::Result<Option<Config>> {
    match std::fs::read_to_string(config_path()?) {
        Ok(text) => Ok(Some(serde_json::from_str(&text).context("配置文件格式错误")?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("无法读取配置文件"),
    }
}

pub fn write_config(config: &Config) -> anyhow::Result<PathBuf> {
    let path = config_path()?;
    create_private(&path)?.write_all(serde_json::to_string_pretty(config)?.as_bytes())?;
    Ok(path)
}

/// A file only the user can read, in a directory only they can enter.
#[cfg(unix)]
fn create_private(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path.parent().expect("has a parent"))?;
    let file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?;
    // Set on the open file before the token goes in, also when the file was already there.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

/// On Windows the user's profile, where the file is, is only theirs already.
#[cfg(windows)]
fn create_private(path: &Path) -> std::io::Result<File> {
    std::fs::create_dir_all(path.parent().expect("has a parent"))?;
    File::create(path)
}

/// What `NEXTTODO_URL` is set to for the app on this computer in local mode.
pub const LOCAL: &str = "local";

const UNREACHABLE: &str = "无法连接本机模式的 NextTodo：App 未运行或正在启动";

/// The server and token: environment variables first, then the configuration file. In local mode
/// the token comes from the environment only: the file's is a server's.
pub fn connect() -> anyhow::Result<(Server, String)> {
    let (url, token) = (std::env::var("NEXTTODO_URL").ok(), std::env::var("NEXTTODO_TOKEN").ok());
    if url.as_deref() == Some(LOCAL) {
        let token = token.context("本机模式要设置 NEXTTODO_TOKEN：在 App 的设置里生成令牌")?;
        return Ok((Server::local()?, token));
    }
    let config = if url.is_some() && token.is_some() { None } else { read_config()? };
    let url = url.or_else(|| config.as_ref().map(|c| c.url.clone()));
    let token = token.or_else(|| config.map(|c| c.token));
    match (url, token) {
        (Some(url), Some(token)) => Ok((Server::new(&url), token)),
        _ => bail!("尚未登录：请先运行 nexttodo login --url <服务器地址>，或设置 NEXTTODO_URL 和 NEXTTODO_TOKEN"),
    }
}

/// Local mode's directory in the desktop app's data, where the app writes the address file.
fn local_dir() -> anyhow::Result<PathBuf> {
    const APP: &str = "cn.juniexd.nexttodo";
    if cfg!(target_os = "macos") {
        Ok(std::env::home_dir().context("无法找到用户主目录")?.join("Library/Application Support").join(APP).join("local"))
    } else if cfg!(windows) {
        Ok(PathBuf::from(std::env::var_os("APPDATA").context("没有 APPDATA 环境变量")?).join(APP).join("local"))
    } else {
        bail!("本机模式只有 macOS 和 Windows 的 App 有")
    }
}

/// Where the app listens for agents, and the CA of its certificate: a new pair each time it starts.
#[derive(Deserialize)]
struct Address {
    url: String,
    ca: String,
}

pub struct Server {
    agent: ureq::Agent,
    pub url: String,
    /// The app on this computer in local mode.
    pub local: bool,
    /// How long a request may take in all.
    pub timeout: Duration,
}

/// How long most requests may take; sending a file may take longer.
const TIMEOUT: Duration = Duration::from_secs(30);
/// How long an export or import may take: an account's files can be many.
pub const TRANSFER_TIMEOUT: Duration = Duration::from_secs(3600);

impl Server {
    pub fn new(url: &str) -> Self {
        let config = ureq::Agent::config_builder().http_status_as_error(false).build();
        Server { agent: config.into(), url: url.trim_end_matches('/').to_owned(), local: false, timeout: TIMEOUT }
    }

    /// The app on this computer in local mode, as its address file has it now (docs/local.md#agent-接入).
    /// Only the app's certificate of this run is trusted, which another program on a port the app had
    /// can't show: nothing is sent to it. No proxy, which would get the token, and no redirects.
    fn local() -> anyhow::Result<Self> {
        let path = local_dir()?.join("address");
        let text = match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => bail!("{UNREACHABLE}"),
            read => read.with_context(|| format!("无法读取本机模式的地址文件 {}", path.display()))?,
        };
        let bad = || format!("本机模式的地址文件 {} 格式错误", path.display());
        let address: Address = serde_json::from_str(&text).with_context(bad)?;
        // Nothing but the port may vary: no user, path or query.
        let port = address.url.strip_prefix("https://127.0.0.1:").and_then(|p| p.parse::<u16>().ok()).filter(|&p| p != 0);
        if port.is_none() {
            bail!("{}", bad());
        }
        let ca = ureq::tls::Certificate::from_pem(address.ca.as_bytes()).with_context(bad)?;
        let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::new_with_certs(&[ca])).build();
        let config = ureq::Agent::config_builder().tls_config(tls).proxy(None).max_redirects(0).http_status_as_error(false).build();
        Ok(Server { agent: config.into(), url: address.url, local: true, timeout: TIMEOUT })
    }

    /// In local mode, whether the other end could not show this run's certificate: it is another
    /// program, or a later run of the app. Trying again is no use. ureq reports a failed handshake
    /// as invalid data.
    fn mismatch(&self, e: &ureq::Error) -> bool {
        self.local && matches!(e, ureq::Error::Io(e) if e.kind() == std::io::ErrorKind::InvalidData)
    }

    /// What to say when a request could not be sent, or its reply not read.
    pub fn unreachable(&self, e: ureq::Error) -> anyhow::Error {
        if self.mismatch(&e) {
            anyhow!("本机 NextTodo 的证书不匹配，App 可能刚重启过")
        } else {
            anyhow::Error::new(e).context(if self.local { UNREACHABLE } else { "无法连接服务器" })
        }
    }

    /// Sends once and reads the whole reply.
    pub fn send(&self, path: &str, token: Option<&str>, key: Option<&str>, body: &Value) -> Result<(StatusCode, String), ureq::Error> {
        let mut req = self.agent.post(format!("{}{path}", self.url)).config().timeout_global(Some(self.timeout)).build();
        if let Some(key) = key {
            req = req.header("Idempotency-Key", key);
        }
        if let Some(token) = token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        let mut res = req.send_json(body)?;
        Ok((res.status(), res.body_mut().read_to_string()?))
    }

    /// Gets `path`, the reply if it is a success, else the server's error message.
    pub fn get(&self, path: &str, token: &str) -> anyhow::Result<ureq::http::Response<ureq::Body>> {
        let req = self.agent.get(format!("{}{path}", self.url)).config().timeout_global(Some(self.timeout)).build();
        let mut res = req.header("Authorization", format!("Bearer {token}")).call().map_err(|e| self.unreachable(e))?;
        if !res.status().is_success() {
            let text = res.body_mut().read_to_string().map_err(|e| self.unreachable(e))?;
            reply(res.status(), &text)?;
        }
        Ok(res)
    }

    /// Posts an MCP message to `/mcp`: its reply, or none when it accepted a notification.
    pub fn mcp(&self, token: &str, message: &str, version: Option<&str>) -> anyhow::Result<Option<Value>> {
        let mut req = self
            .agent
            .post(format!("{}/mcp", self.url))
            .config()
            .timeout_global(Some(self.timeout))
            .build()
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json, text/event-stream")
            .header("Content-Type", "application/json");
        if let Some(version) = version {
            req = req.header("MCP-Protocol-Version", version);
        }
        let mut res = req.send(message).map_err(|e| self.unreachable(e))?;
        if res.status() == StatusCode::ACCEPTED {
            return Ok(None);
        }
        let text = res.body_mut().read_to_string().map_err(|e| self.unreachable(e))?;
        reply(res.status(), &text).map(Some)
    }

    /// Uploads `bytes` as file `id`, of `media_type`.
    pub fn put_file(&self, token: &str, id: &str, media_type: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let req = self.agent.put(format!("{}/api/v1/files/{id}", self.url)).config().timeout_global(Some(self.timeout)).build();
        let req = req.header("Authorization", format!("Bearer {token}")).header("Content-Type", media_type);
        let mut res = req.send(bytes).map_err(|e| self.unreachable(e))?;
        let text = res.body_mut().read_to_string().map_err(|e| self.unreachable(e))?;
        reply(res.status(), &text)?;
        Ok(())
    }

    /// Posts JSON once and returns the reply, or the server's error message.
    pub fn post(&self, path: &str, token: Option<&str>, body: &Value) -> anyhow::Result<Value> {
        let (status, text) = self.send(path, token, None, body).map_err(|e| self.unreachable(e))?;
        reply(status, &text)
    }

    /// Calls a tool, a write as change `change`, and returns `(text, data)`. Network failures,
    /// including a reply cut off on the way back, are retried, a write as the same change, so that one
    /// which reached the server applies once. In local mode that is the same run of the app: one
    /// restarted since has another certificate, and the change never goes to a database restored
    /// meanwhile.
    pub fn tool(&self, name: &str, token: &str, change: Option<&str>, args: &Value) -> anyhow::Result<(String, Value)> {
        let path = format!("/api/v1/tools/{name}");
        // The server's answer lost on the way back, or a gateway's in its place: it may have gone in
        // (docs/agent.md#重试).
        let lost = |e: anyhow::Error| match change {
            Some(id) => anyhow!("{e:#}；改动可能已经生效，重发时加上 --request-id {id}，不会重复生效"),
            None => e,
        };
        let mut attempt = 0;
        let (status, text) = loop {
            match self.send(&path, Some(token), change, args) {
                Ok(res) => break res,
                Err(e) if attempt < 2 && !self.mismatch(&e) => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(500 * attempt));
                    eprintln!("{}", printable(&format!("连接服务器失败（{e}），第 {attempt} 次重试")));
                }
                Err(e) => return Err(lost(self.unreachable(e))),
            }
        };
        // A gateway's own answer: none of the server's errors at these statuses.
        let ours = |v: Value| matches!(v["code"].as_str(), Some("upstream" | "unavailable" | "outcome_unknown"));
        let gateway = matches!(status.as_u16(), 502..=504) && !serde_json::from_str(&text).is_ok_and(ours);
        match reply(status, &text).map_err(|e| if gateway { lost(e) } else { e })? {
            Value::Object(mut reply) => match (reply.remove("text"), reply.remove("data")) {
                (Some(Value::String(text)), Some(data)) => Ok((text, data)),
                _ => bail!("服务器回复缺少 text 或 data"),
            },
            _ => bail!("服务器回复格式错误"),
        }
    }
}

/// The body of a successful reply, or the error the server described.
pub fn reply(status: StatusCode, text: &str) -> anyhow::Result<Value> {
    if status == StatusCode::NO_CONTENT {
        return Ok(Value::Null);
    }
    if status.is_success() {
        return serde_json::from_str(text).context("服务器回复不是 JSON");
    }
    // A proxy in between may answer with a page of its own.
    let reply: Value = serde_json::from_str(text).unwrap_or_default();
    let message = reply["message"].as_str().map_or_else(|| format!("服务器返回 {status}"), str::to_owned);
    let details = &reply["details"];
    match (details["field"].as_str(), details["mutation_id"].as_str()) {
        (Some(field), _) => bail!("{message}（{field}）"),
        // What the command is sent again with (docs/agent.md#重试).
        (None, Some(id)) => bail!("{message}（--request-id {id}）"),
        (None, None) => bail!("{message}"),
    }
}
