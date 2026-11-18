//! `nexttodo`: the command line for NextTodo. Each command calls one tool on the server, which does
//! all the work; this only sends the arguments and prints the result. See `docs/agent.md`.

use std::fs::File;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::{Parser, Subcommand};
use nexttodo_core::model::{FileName, MAX_FILE_SIZE};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use ureq::http::StatusCode;

/// The release this was built for, which `deploy/build.sh` gives; the crate's own version otherwise.
const VERSION: &str = match option_env!("NEXTTODO_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// The skill of this version, which `skill install` puts where agents find it.
const SKILL: &str = include_str!("../../../skills/nexttodo/SKILL.md");

#[derive(Parser)]
#[command(name = "nexttodo", version = VERSION, about = "NextTodo 命令行：查看和管理待办")]
struct Cli {
    #[arg(long, global = true, help = "输出 JSON 数据")]
    json: bool,
    #[arg(
        long,
        global = true,
        value_name = "UUIDv7",
        help = "写操作的改动 ID：报“可能已经生效”时，用错误里给的 ID 重发同一条命令，生效过的不会再生效一次"
    )]
    request_id: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "用用户名和密码登录，生成令牌存到配置文件")]
    Login {
        #[arg(long, help = "服务器地址，如 https://todo.example.com")]
        url: Option<String>,
        #[arg(long, default_value = "命令行", help = "令牌名，会显示在改动记录里")]
        name: String,
        #[arg(long, help = "生成只读令牌")]
        read_only: bool,
    },
    #[command(about = "文件夹树")]
    Tree,
    #[command(about = "列出文件夹里的子文件夹和事项")]
    Ls {
        #[arg(help = "文件夹路径，默认根目录")]
        folder: Option<String>,
        #[arg(long, help = "包括已完成的")]
        all: bool,
        #[arg(long, help = "最多列出多少项，默认 50，最多 500")]
        limit: Option<u32>,
    },
    #[command(about = "接下来：现在就能做的事项")]
    Next {
        #[arg(long = "folder", num_args = 1.., value_name = "路径", help = "只看这些文件夹（含子文件夹）里的")]
        folders: Option<Vec<String>>,
        #[arg(long, help = "最多列出多少项，默认 50，最多 500")]
        limit: Option<u32>,
    },
    #[command(about = "事项详情")]
    Show {
        #[arg(value_parser = task, help = TASK)]
        task: String,
    },
    #[command(about = "搜索事项的标题和描述")]
    Search {
        #[arg(required = true, help = "关键词")]
        query: Vec<String>,
        #[arg(long, help = "最多列出多少项，默认 50，最多 500")]
        limit: Option<u32>,
    },
    #[command(about = "文件夹里的事项和依赖")]
    Graph {
        #[arg(help = "文件夹路径，默认全部")]
        folder: Option<String>,
        #[arg(long, help = "包括已完成的")]
        all: bool,
        #[arg(long, help = "输出 Mermaid 流程图")]
        mermaid: bool,
    },
    #[command(about = "最近的改动批次")]
    Log {
        #[arg(short = 'n', default_value_t = 10, help = "条数，最多 100")]
        limit: u32,
    },
    #[command(about = "AI 整理的请求：等待、正在整理和失败的")]
    Jobs,
    #[command(about = "添加一个事项")]
    Add {
        title: String,
        #[arg(long, help = "文件夹路径，不存在时一起新建")]
        folder: Option<String>,
        #[arg(short, long, allow_hyphen_values = true, help = "描述，Markdown")]
        description: Option<String>,
        #[arg(long, help = "开始时间：2026-11-14 或 \"2026-11-14 09:30\"，没到时事项等着")]
        start: Option<String>,
        #[arg(long, help = "截止时间：2026-11-20 或 \"2026-11-20 18:00\"")]
        due: Option<String>,
        #[arg(long, help = "提前多少分钟提醒，0 表示截止时")]
        remind: Option<u32>,
        #[arg(
            long,
            value_name = "间隔",
            help = "重复：1d 每天、2w 每两周、3m 每三个月、1y 每年，1w:1,2,3,4,5 每个工作日、1w:1,3,5 每周一三五，1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天；要有截止时间"
        )]
        repeat: Option<String>,
        #[arg(long, requires = "repeat", value_parser = ["due", "done"], help = "下一次从哪天算：due 按截止日期（默认），done 从完成那天")]
        repeat_from: Option<String>,
        #[arg(long, num_args = 1.., value_name = "用户名", help = "负责人：共享的文件夹里的共享人或成员")]
        assign: Vec<String>,
        #[arg(long, num_args = 1.., value_parser = task, help = "前置事项的编号")]
        after: Vec<String>,
        #[arg(long, num_args = 1.., value_parser = task, help = "后续事项的编号")]
        before: Vec<String>,
    },
    #[command(about = "修改事项")]
    Edit {
        #[arg(value_parser = task, help = TASK)]
        task: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(short, long, allow_hyphen_values = true, help = "描述，整段替换")]
        description: Option<String>,
        #[arg(
            long,
            num_args = 2,
            allow_hyphen_values = true,
            value_names = ["原文", "新文"],
            conflicts_with = "description",
            help = "把描述里恰好出现一次的原文换成新文，可以重复"
        )]
        replace: Vec<String>,
        #[arg(long, allow_hyphen_values = true, conflicts_with = "description", help = "在描述末尾另起一行追加")]
        append: Option<String>,
        #[arg(long, conflicts_with = "no_start", help = "开始时间：2026-11-14 或 \"2026-11-14 09:30\"；只写日期的去掉原来的几点")]
        start: Option<String>,
        #[arg(long, help = "清除开始时间")]
        no_start: bool,
        #[arg(long, conflicts_with = "no_due", help = "截止时间：2026-11-20 或 \"2026-11-20 18:00\"")]
        due: Option<String>,
        #[arg(long, help = "清除截止时间")]
        no_due: bool,
        #[arg(long, conflicts_with = "no_remind", help = "提前多少分钟提醒")]
        remind: Option<u32>,
        #[arg(long, help = "清除提醒")]
        no_remind: bool,
        #[arg(
            long,
            value_name = "间隔",
            conflicts_with = "no_repeat",
            help = "重复：1d 每天、2w 每两周、3m 每三个月、1y 每年，1w:1,2,3,4,5 每个工作日、1w:1,3,5 每周一三五，1m:1,15 每月 1 号和 15 号、1m:31 每月最后一天"
        )]
        repeat: Option<String>,
        #[arg(long, requires = "repeat", value_parser = ["due", "done"], help = "下一次从哪天算：due 按截止日期（默认），done 从完成那天")]
        repeat_from: Option<String>,
        #[arg(long, help = "不再重复")]
        no_repeat: bool,
        #[arg(long, num_args = 1.., value_name = "用户名", help = "加上负责人")]
        assign: Vec<String>,
        #[arg(long, num_args = 1.., value_name = "用户名", help = "去掉负责人")]
        unassign: Vec<String>,
    },
    #[command(about = "标为完成")]
    Done {
        #[arg(required = true, value_parser = task, help = TASK)]
        tasks: Vec<String>,
        #[arg(long, help = "重复的事项结束重复、直接完成，如读完一本书时")]
        end_repeat: bool,
    },
    #[command(about = "重新打开")]
    Reopen {
        #[arg(required = true, value_parser = task, help = TASK)]
        tasks: Vec<String>,
    },
    #[command(about = "新建文件夹，缺的上级一起建")]
    Mkdir { path: String },
    #[command(about = "文件夹或附件改名")]
    Rename {
        #[arg(value_name = "文件夹或附件 ID", help = "文件夹路径；带 --attachment 时是附件 ID")]
        target: String,
        name: String,
        #[arg(long, help = "给附件改名")]
        attachment: bool,
    },
    #[command(about = "移动事项（编号）或文件夹（路径）到另一个文件夹")]
    Mv {
        #[arg(required = true, num_args = 2.., help = "要移动的事项和文件夹，最后一个是目标文件夹")]
        items: Vec<String>,
    },
    #[command(about = "删除事项（编号）或文件夹（路径，连同里面的一切）")]
    Rm {
        #[arg(required = true)]
        items: Vec<String>,
    },
    #[command(about = "加减依赖")]
    Dep {
        #[command(subcommand)]
        command: Dep,
    },
    #[command(about = "给事项加附件，每个文件一个批次")]
    Attach {
        #[arg(value_parser = task, help = TASK)]
        task: String,
        #[arg(required = true, value_name = "文件")]
        files: Vec<PathBuf>,
        #[arg(long, help = "附件的名称，默认是文件名；只能和一个文件一起给")]
        name: Option<String>,
    },
    #[command(about = "删除附件")]
    Detach {
        #[arg(required = true, value_name = "附件 ID")]
        attachments: Vec<String>,
    },
    #[command(about = "下载附件")]
    Download {
        #[arg(value_name = "附件 ID")]
        id: String,
        #[arg(short, long, help = "存到哪个文件或目录，默认当前目录，用附件的名称")]
        output: Option<PathBuf>,
    },
    #[command(about = "按计划批量新建文件夹、事项和依赖")]
    Apply {
        #[arg(help = "YAML 计划文件，- 表示从标准输入读")]
        file: PathBuf,
        #[arg(long, help = "只预览，不保存")]
        dry_run: bool,
    },
    #[command(about = "把自己的文件夹共享给别人，或添加成员")]
    Share {
        folder: String,
        #[arg(required = true, value_name = "用户名")]
        users: Vec<String>,
    },
    #[command(about = "共享人移除成员，不写用户名就是停止共享；成员不写用户名就是自己退出")]
    Unshare {
        folder: String,
        #[arg(value_name = "用户名")]
        users: Vec<String>,
    },
    #[command(about = "撤销一个批次")]
    Undo { batch: i64 },
    #[command(about = "MCP 服务器（stdio），转接到 NEXTTODO_URL 的服务器；本机模式的 App 用它接入 agent")]
    Mcp,
    #[command(about = "教 agent 使用 NextTodo 的 skill")]
    Skill {
        #[command(subcommand)]
        command: Skill,
    },
}

#[derive(Subcommand)]
enum Skill {
    #[command(about = "装到 agent 的 skill 目录：默认是本机已有的 ~/.claude/skills、~/.codex/skills")]
    Install {
        #[arg(long, help = "skill 目录，可以给多个")]
        dir: Vec<PathBuf>,
    },
}

#[derive(Subcommand)]
enum Dep {
    #[command(about = "加依赖：前置完成后才能做后续")]
    Add {
        #[arg(value_parser = task)]
        before: String,
        #[arg(value_parser = task)]
        after: String,
    },
    #[command(about = "删依赖")]
    Rm {
        #[arg(value_parser = task)]
        before: String,
        #[arg(value_parser = task)]
        after: String,
    },
}

const TASK: &str = "事项编号：12 或 #12；别人共享的文件夹里的写成 @alice#42";

/// A task by number: `12`, `#12`, or `@alice#42` in a folder someone else shares.
fn task(s: &str) -> Result<String, String> {
    let number = match s.strip_prefix('@') {
        Some(rest) => rest.split_once('#').map(|(_, n)| n),
        None => Some(s.strip_prefix('#').unwrap_or(s)),
    };
    match number.map(str::parse::<i64>) {
        Some(Ok(_)) => Ok(s.to_owned()),
        _ => Err(format!("“{s}”不是事项编号")),
    }
}

/// Splits arguments into tasks and folder paths.
fn items(items: &[String]) -> (Vec<&str>, Vec<&str>) {
    items.iter().map(String::as_str).partition(|item| task(item).is_ok())
}

#[derive(Serialize, Deserialize)]
struct Config {
    url: String,
    token: String,
}

fn config_path() -> anyhow::Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::home_dir().context("无法找到用户主目录")?.join(".config"),
    };
    Ok(base.join("nexttodo").join("config.json"))
}

fn read_config() -> anyhow::Result<Option<Config>> {
    match std::fs::read_to_string(config_path()?) {
        Ok(text) => Ok(Some(serde_json::from_str(&text).context("配置文件格式错误")?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("无法读取配置文件"),
    }
}

fn write_config(config: &Config) -> anyhow::Result<PathBuf> {
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
const LOCAL: &str = "local";

const UNREACHABLE: &str = "无法连接本机模式的 NextTodo：App 未运行或正在启动";

/// The server and token: environment variables first, then the configuration file. In local mode
/// the token comes from the environment only: the file's is a server's.
fn connect() -> anyhow::Result<(Server, String)> {
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

struct Server {
    agent: ureq::Agent,
    url: String,
    /// The app on this computer in local mode.
    local: bool,
    /// How long a request may take in all.
    timeout: Duration,
}

/// How long most requests may take; sending a file may take longer.
const TIMEOUT: Duration = Duration::from_secs(30);

impl Server {
    fn new(url: &str) -> Self {
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
    fn unreachable(&self, e: ureq::Error) -> anyhow::Error {
        if self.mismatch(&e) {
            anyhow!("本机 NextTodo 的证书不匹配，App 可能刚重启过")
        } else {
            anyhow::Error::new(e).context(if self.local { UNREACHABLE } else { "无法连接服务器" })
        }
    }

    /// Sends once and reads the whole reply.
    fn send(&self, path: &str, token: Option<&str>, key: Option<&str>, body: &Value) -> Result<(StatusCode, String), ureq::Error> {
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

    /// Posts JSON once and returns the reply, or the server's error message.
    fn post(&self, path: &str, token: Option<&str>, body: &Value) -> anyhow::Result<Value> {
        let (status, text) = self.send(path, token, None, body).map_err(|e| self.unreachable(e))?;
        reply(status, &text)
    }

    /// Calls a tool, a write as change `change`, and returns `(text, data)`. Network failures,
    /// including a reply cut off on the way back, are retried, a write as the same change, so that one
    /// which reached the server applies once. In local mode that is the same run of the app: one
    /// restarted since has another certificate, and the change never goes to a database restored
    /// meanwhile.
    fn tool(&self, name: &str, token: &str, change: Option<&str>, args: &Value) -> anyhow::Result<(String, Value)> {
        let path = format!("/api/v1/tools/{name}");
        let mut attempt = 0;
        let (status, text) = loop {
            match self.send(&path, Some(token), change, args) {
                Ok(res) => break res,
                Err(e) if attempt < 2 && !self.mismatch(&e) => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(500 * attempt));
                    eprintln!("{}", printable(&format!("连接服务器失败（{e}），第 {attempt} 次重试")));
                }
                Err(e) => {
                    let e = self.unreachable(e);
                    // It may have gone in, the answers lost on the way back (docs/agent.md#重试).
                    return Err(match change {
                        Some(id) => anyhow!("{e:#}；改动可能已经生效，重发时加上 --request-id {id}，不会重复生效"),
                        None => e,
                    });
                }
            }
        };
        match reply(status, &text)? {
            Value::Object(mut reply) => match (reply.remove("text"), reply.remove("data")) {
                (Some(Value::String(text)), Some(data)) => Ok((text, data)),
                _ => bail!("服务器回复缺少 text 或 data"),
            },
            _ => bail!("服务器回复格式错误"),
        }
    }
}

/// The body of a successful reply, or the error the server described.
fn reply(status: StatusCode, text: &str) -> anyhow::Result<Value> {
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

/// Text for the terminal: control characters, which could clear the screen or fake other output,
/// are shown escaped; line breaks and tabs stay.
fn printable(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control() && !matches!(c, '\n' | '\t') {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

/// The day and time here, in a tool's arguments: starts come by them, and repeating tasks done move on
/// from the day (docs/agent.md#引用).
fn here(args: &mut Value) {
    let now = chrono::Local::now();
    args["today"] = now.format("%Y-%m-%d").to_string().into();
    args["time"] = now.format("%H:%M").to_string().into();
}

/// Calls read tool `name` and prints what it gives.
fn read(name: &str, args: Value, out: &Out) -> anyhow::Result<()> {
    call(name, None, args, out)
}

/// Calls write tool `name`, which makes one change, and prints what it did.
fn write(name: &str, args: Value, out: &Out) -> anyhow::Result<()> {
    call(name, Some(&out.change()), args, out)
}

fn call(name: &str, change: Option<&str>, mut args: Value, out: &Out) -> anyhow::Result<()> {
    here(&mut args);
    let (server, token) = connect()?;
    let (text, data) = server.tool(name, &token, change, &args)?;
    if out.json {
        println!("{}", serde_json::to_string_pretty(&data)?);
    } else {
        println!("{}", printable(&text));
    }
    Ok(())
}

/// Attaches each file to `task`, one change each, as `name` or the file's own. With `--json`, what each
/// change did, as one array: those made before one failed too.
fn attach(task: &str, files: &[PathBuf], name: Option<String>, out: &Out) -> anyhow::Result<()> {
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
fn download(id: &str, output: Option<PathBuf>, json_out: bool) -> anyhow::Result<()> {
    let (mut server, token) = connect()?;
    server.timeout = Duration::from_secs(600);
    let req = server.agent.get(format!("{}/api/v1/files/{id}", server.url)).config().timeout_global(Some(server.timeout)).build();
    let mut res = req.header("Authorization", format!("Bearer {token}")).call().map_err(|e| server.unreachable(e))?;
    if !res.status().is_success() {
        let text = res.body_mut().read_to_string().map_err(|e| server.unreachable(e))?;
        reply(res.status(), &text)?;
    }
    let disposition = res.headers().get("content-disposition").and_then(|v| v.to_str().ok()).unwrap_or_default();
    let name = disposition.split_once("filename*=UTF-8''").and_then(|(_, n)| decode(n)).context("服务器回复缺少附件的名称")?;
    let name = FileName::try_from(name).map_err(|e| anyhow!("附件的名称无效：{e}"))?;
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

fn login(url: Option<String>, name: String, read_only: bool) -> anyhow::Result<()> {
    let url = url.or_else(|| read_config().ok().flatten().map(|c| c.url)).context("请用 --url 指定服务器地址")?;
    if url == LOCAL {
        bail!("本机模式不用登录：在 App 的设置里生成令牌，设置 NEXTTODO_URL=local 和 NEXTTODO_TOKEN");
    }
    let server = Server::new(&url);
    print!("用户名：");
    std::io::stdout().flush()?;
    let mut username = String::new();
    std::io::stdin().read_line(&mut username)?;
    let password = if std::io::stdin().is_terminal() {
        rpassword::prompt_password("密码：")?
    } else {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        line.trim_end_matches(['\r', '\n']).to_owned()
    };
    let session = server.post(
        "/api/v1/auth/login",
        None,
        &json!({ "username": username.trim(), "password": password, "device": "nexttodo 命令行" }),
    )?;
    let session = session["secret"].as_str().ok_or_else(|| anyhow!("服务器回复中没有会话"))?.to_owned();
    let scope = if read_only { "read" } else { "write" };
    let made = server.post("/api/v1/tokens", Some(&session), &json!({ "name": name, "scope": scope }));
    // The session only existed to make the token.
    server.post("/api/v1/auth/logout", Some(&session), &json!({}))?;
    let token = made?["secret"].as_str().ok_or_else(|| anyhow!("服务器回复中没有令牌"))?.to_owned();
    let path = write_config(&Config { url: server.url.clone(), token })?;
    println!("{}", printable(&format!("已登录，令牌“{name}”保存在 {}", path.display())));
    println!("让 agent 学会使用 NextTodo：nexttodo skill install");
    Ok(())
}

/// Writes the skill into `<dir>/nexttodo/SKILL.md` for each skill directory: those given, or those
/// of the agents found here.
fn install_skill(dirs: Vec<PathBuf>) -> anyhow::Result<()> {
    let dirs = if dirs.is_empty() {
        let home = std::env::home_dir().context("无法找到用户主目录")?;
        let found: Vec<PathBuf> =
            [".claude", ".codex"].iter().map(|agent| home.join(agent)).filter(|a| a.is_dir()).map(|a| a.join("skills")).collect();
        if found.is_empty() {
            bail!("没有找到 ~/.claude 或 ~/.codex，请用 --dir 指定 agent 的 skill 目录");
        }
        found
    } else {
        dirs
    };
    for dir in dirs {
        let target = dir.join("nexttodo");
        // Linked to a copy kept up to date some other way, such as the repository's.
        if target.is_symlink() {
            println!("{}", printable(&format!("跳过 {}：它是链接", target.display())));
            continue;
        }
        std::fs::create_dir_all(&target).with_context(|| format!("无法创建 {}", target.display()))?;
        let file = target.join("SKILL.md");
        std::fs::write(&file, SKILL).with_context(|| format!("无法写入 {}", file.display()))?;
        println!("{}", printable(&format!("已装到 {}", file.display())));
    }
    Ok(())
}

/// An MCP server on stdin and stdout that hands each message to the server's `/mcp` and its reply
/// back: for local mode, whose address changes each time the app starts. The address is read again
/// for each message, so a restart of the app is no break. Nothing is retried here: the agent sends a
/// write again with its `request_id` (docs/agent.md#重试). Only messages go to stdout; what went
/// wrong goes to stderr, and as an error reply to a request.
fn mcp() -> anyhow::Result<()> {
    // Agreed on in `initialize`, and sent with every message after it.
    let mut version: Option<String> = None;
    for line in std::io::stdin().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let mut message: Value = match serde_json::from_str(&line) {
            Ok(m) => m,
            Err(e) => {
                answer(&json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("不是 JSON：{e}") } }))?;
                continue;
            }
        };
        // Agents seldom say what day and time it is: through here, they are this computer's.
        if message["method"] == "tools/call" {
            let args = &mut message["params"]["arguments"];
            if args.get("today").is_none() && args.get("time").is_none() {
                here(args);
            }
        }
        match forward(&message.to_string(), version.as_deref()) {
            Ok(Some(reply)) => {
                if message["method"] == "initialize" {
                    version = reply["result"]["protocolVersion"].as_str().map(str::to_owned);
                }
                answer(&reply)?;
            }
            // A notification, or a reply to the server.
            Ok(None) => {}
            Err(e) => {
                let e = printable(&format!("{e:#}"));
                eprintln!("错误：{e}");
                if let Some(id) = message.get("id").filter(|_| message.get("method").is_some()) {
                    answer(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32000, "message": e } }))?;
                }
            }
        }
    }
    Ok(())
}

/// Posts one message to the server's `/mcp`: its reply, or none when it accepted a notification.
fn forward(message: &str, version: Option<&str>) -> anyhow::Result<Option<Value>> {
    let (server, token) = connect()?;
    let mut req = server
        .agent
        .post(format!("{}/mcp", server.url))
        .config()
        .timeout_global(Some(server.timeout))
        .build()
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json, text/event-stream")
        .header("Content-Type", "application/json");
    if let Some(version) = version {
        req = req.header("MCP-Protocol-Version", version);
    }
    let mut res = req.send(message).map_err(|e| server.unreachable(e))?;
    if res.status() == StatusCode::ACCEPTED {
        return Ok(None);
    }
    let text = res.body_mut().read_to_string().map_err(|e| server.unreachable(e))?;
    reply(res.status(), &text).map(Some)
}

/// Writes a message to stdout, one line each.
fn answer(message: &Value) -> anyhow::Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{message}")?;
    out.flush()?;
    Ok(())
}

fn plan(file: &PathBuf) -> anyhow::Result<Value> {
    let mut text = String::new();
    if file.as_os_str() == "-" {
        std::io::stdin().read_to_string(&mut text)?;
    } else {
        text = std::fs::read_to_string(file).with_context(|| format!("无法读取 {}", file.display()))?;
    }
    serde_saphyr::from_str(&text).map_err(|e| anyhow!("计划不是有效的 YAML：{e}"))
}

/// How a command's result is put out, and the change it makes.
struct Out {
    json: bool,
    request_id: Option<String>,
}

impl Out {
    /// The id of the change a write makes: as given, or a new one.
    fn change(&self) -> String {
        self.request_id.clone().unwrap_or_else(|| uuid::Uuid::now_v7().to_string())
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let j = &Out { json: cli.json, request_id: cli.request_id };
    match cli.command {
        Command::Login { url, name, read_only } => login(url, name, read_only),
        Command::Tree => read("tree", json!({}), j),
        Command::Ls { folder, all, limit } => read("list", json!({ "folder": folder, "all": all, "limit": limit }), j),
        Command::Next { folders, limit } => {
            let mut args = json!({ "limit": limit });
            // Only with a filter: servers before it turn down arguments they don't know.
            if let Some(folders) = folders {
                args["folders"] = folders.into();
            }
            read("next", args, j)
        }
        Command::Show { task } => read("show", json!({ "task": task }), j),
        Command::Search { query, limit } => read("search", json!({ "query": query.join(" "), "limit": limit }), j),
        Command::Graph { folder, all, mermaid } => read("graph", json!({ "folder": folder, "all": all, "mermaid": mermaid }), j),
        Command::Log { limit } => read("log", json!({ "limit": limit }), j),
        Command::Jobs => read("jobs", json!({}), j),
        Command::Add { title, folder, description, start, due, remind, repeat, repeat_from, assign, after, before } => write(
            "add",
            json!({
                "title": title, "folder": folder, "description": description, "start": start, "due": due, "remind": remind,
                "repeat": repeat, "repeat_from": repeat_from, "assign": assign, "after": after, "before": before
            }),
            j,
        ),
        Command::Edit {
            task,
            title,
            description,
            replace,
            append,
            start,
            no_start,
            due,
            no_due,
            remind,
            no_remind,
            repeat,
            repeat_from,
            no_repeat,
            assign,
            unassign,
        } => {
            let mut args = Map::new();
            args.insert("task".into(), task.into());
            if let Some(title) = title {
                args.insert("title".into(), title.into());
            }
            if let Some(description) = description {
                args.insert("description".into(), description.into());
            }
            if !replace.is_empty() {
                args.insert("replace".into(), replace.chunks(2).map(|p| json!({ "old": p[0], "new": p[1] })).collect());
            }
            if let Some(append) = append {
                args.insert("append".into(), append.into());
            }
            if no_start || start.is_some() {
                args.insert("start".into(), start.into());
            }
            if no_due || due.is_some() {
                args.insert("due".into(), due.into());
            }
            if no_remind || remind.is_some() {
                args.insert("remind".into(), remind.into());
            }
            if no_repeat || repeat.is_some() {
                args.insert("repeat".into(), repeat.into());
            }
            if let Some(from) = repeat_from {
                args.insert("repeat_from".into(), from.into());
            }
            if !assign.is_empty() {
                args.insert("assign".into(), assign.into());
            }
            if !unassign.is_empty() {
                args.insert("unassign".into(), unassign.into());
            }
            write("edit", args.into(), j)
        }
        Command::Done { tasks, end_repeat } => write("done", json!({ "tasks": tasks, "end_repeat": end_repeat }), j),
        Command::Reopen { tasks } => write("reopen", json!({ "tasks": tasks }), j),
        Command::Mkdir { path } => write("mkdir", json!({ "path": path }), j),
        Command::Rename { target, name, attachment: true } => write("rename", json!({ "attachment": target, "name": name }), j),
        Command::Rename { target, name, attachment: false } => write("rename", json!({ "folder": target, "name": name }), j),
        Command::Attach { task, files, name } => attach(&task, &files, name, j),
        Command::Detach { attachments } => write("detach", json!({ "attachments": attachments }), j),
        Command::Download { id, output } => download(&id, output, j.json),
        Command::Mv { items: mut list } => {
            let to = list.pop().expect("at least two arguments");
            let (tasks, folders) = items(&list);
            write("move", json!({ "tasks": tasks, "folders": folders, "to": to }), j)
        }
        Command::Rm { items: list } => {
            let (tasks, folders) = items(&list);
            write("delete", json!({ "tasks": tasks, "folders": folders }), j)
        }
        Command::Dep { command: Dep::Add { before, after } } => write("dep_add", json!({ "before": before, "after": after }), j),
        Command::Dep { command: Dep::Rm { before, after } } => write("dep_remove", json!({ "before": before, "after": after }), j),
        Command::Apply { file, dry_run } => write("apply", json!({ "plan": plan(&file)?, "dry_run": dry_run }), j),
        Command::Share { folder, users } => write("share", json!({ "folder": folder, "users": users }), j),
        Command::Unshare { folder, users } => write("unshare", json!({ "folder": folder, "users": users }), j),
        Command::Undo { batch } => write("undo", json!({ "batch": batch }), j),
        Command::Mcp => mcp(),
        Command::Skill { command: Skill::Install { dir } } => install_skill(dir),
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("错误：{}", printable(&format!("{e:#}")));
            ExitCode::FAILURE
        }
    }
}
