//! `nexttodo`: the command line for NextTodo. Each command calls one tool on the server, which does
//! all the work; this only sends the arguments and prints the result. See `docs/agent.md`.

use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use ureq::http::StatusCode;

#[derive(Parser)]
#[command(name = "nexttodo", version, about = "NextTodo 命令行：查看和管理待办")]
struct Cli {
    #[arg(long, global = true, help = "输出 JSON 数据")]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "用用户名和密码登录，生成令牌存到配置文件")]
    Login {
        #[arg(long, help = "服务器地址，如 https://todo.juniexd.cn")]
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
    },
    #[command(about = "接下来：现在就能做的事项")]
    Next,
    #[command(about = "事项详情")]
    Show {
        #[arg(value_parser = number, help = "事项编号")]
        task: i64,
    },
    #[command(about = "搜索事项的标题和描述")]
    Search {
        #[arg(required = true, help = "关键词")]
        query: Vec<String>,
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
        #[arg(short = 'n', default_value_t = 20, help = "条数，最多 100")]
        limit: u32,
    },
    #[command(about = "添加一个事项")]
    Add {
        title: String,
        #[arg(long, help = "文件夹路径，不存在时一起新建")]
        folder: Option<String>,
        #[arg(short, long, help = "描述，Markdown")]
        description: Option<String>,
        #[arg(long, help = "截止时间：2026-11-20 或 \"2026-11-20 18:00\"")]
        due: Option<String>,
        #[arg(long, help = "提前多少分钟提醒，0 表示截止时")]
        remind: Option<u32>,
        #[arg(long, num_args = 1.., value_parser = number, help = "前置事项的编号")]
        after: Vec<i64>,
        #[arg(long, num_args = 1.., value_parser = number, help = "后续事项的编号")]
        before: Vec<i64>,
    },
    #[command(about = "修改事项")]
    Edit {
        #[arg(value_parser = number)]
        task: i64,
        #[arg(long)]
        title: Option<String>,
        #[arg(short, long, help = "描述，整段替换")]
        description: Option<String>,
        #[arg(long, conflicts_with = "no_due", help = "截止时间：2026-11-20 或 \"2026-11-20 18:00\"")]
        due: Option<String>,
        #[arg(long, help = "清除截止时间")]
        no_due: bool,
        #[arg(long, conflicts_with = "no_remind", help = "提前多少分钟提醒")]
        remind: Option<u32>,
        #[arg(long, help = "清除提醒")]
        no_remind: bool,
    },
    #[command(about = "标为完成")]
    Done {
        #[arg(required = true, value_parser = number)]
        tasks: Vec<i64>,
    },
    #[command(about = "重新打开")]
    Reopen {
        #[arg(required = true, value_parser = number)]
        tasks: Vec<i64>,
    },
    #[command(about = "新建文件夹，缺的上级一起建")]
    Mkdir { path: String },
    #[command(about = "文件夹改名")]
    Rename { folder: String, name: String },
    #[command(about = "把事项（编号）或文件夹（路径）移到另一个文件夹")]
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
    #[command(about = "按计划批量新建文件夹、事项和依赖")]
    Apply {
        #[arg(help = "YAML 计划文件，- 表示从标准输入读")]
        file: PathBuf,
        #[arg(long, help = "只预览，不保存")]
        dry_run: bool,
    },
    #[command(about = "撤销一个批次")]
    Undo { batch: i64 },
}

#[derive(Subcommand)]
enum Dep {
    #[command(about = "加依赖：前置完成后才能做后续")]
    Add {
        #[arg(value_parser = number)]
        before: i64,
        #[arg(value_parser = number)]
        after: i64,
    },
    #[command(about = "删依赖")]
    Rm {
        #[arg(value_parser = number)]
        before: i64,
        #[arg(value_parser = number)]
        after: i64,
    },
}

/// A task number, written `12` or `#12`.
fn number(s: &str) -> Result<i64, String> {
    s.strip_prefix('#').unwrap_or(s).parse().map_err(|_| format!("“{s}”不是事项编号"))
}

/// Splits arguments into task numbers and folder paths.
fn items(items: &[String]) -> (Vec<i64>, Vec<&str>) {
    let mut tasks = Vec::new();
    let mut folders = Vec::new();
    for item in items {
        match number(item) {
            Ok(n) => tasks.push(n),
            Err(_) => folders.push(item.as_str()),
        }
    }
    (tasks, folders)
}

#[derive(Serialize, Deserialize)]
struct Config {
    url: String,
    token: String,
}

fn config_path() -> anyhow::Result<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::home_dir().context("找不到用户主目录")?.join(".config"),
    };
    Ok(base.join("nexttodo").join("config.json"))
}

fn read_config() -> anyhow::Result<Option<Config>> {
    match std::fs::read_to_string(config_path()?) {
        Ok(text) => Ok(Some(serde_json::from_str(&text).context("配置文件格式不对")?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("读不了配置文件"),
    }
}

fn write_config(config: &Config) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let path = config_path()?;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path.parent().expect("has a parent"))?;
    let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&path)?;
    // Set on the open file before the token goes in, also when the file was already there.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(serde_json::to_string_pretty(config)?.as_bytes())?;
    Ok(path)
}

/// The server and token: environment variables first, then the configuration file.
fn credentials() -> anyhow::Result<(String, String)> {
    let (url, token) = (std::env::var("NEXTTODO_URL").ok(), std::env::var("NEXTTODO_TOKEN").ok());
    let config = if url.is_some() && token.is_some() { None } else { read_config()? };
    let url = url.or_else(|| config.as_ref().map(|c| c.url.clone()));
    let token = token.or_else(|| config.map(|c| c.token));
    match (url, token) {
        (Some(url), Some(token)) => Ok((url, token)),
        _ => bail!("还没有登录：先运行 nexttodo login --url <服务器地址>，或者设置 NEXTTODO_URL 和 NEXTTODO_TOKEN"),
    }
}

struct Server {
    agent: ureq::Agent,
    url: String,
}

impl Server {
    fn new(url: &str) -> Self {
        let config = ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(30))).build();
        Server { agent: config.into(), url: url.trim_end_matches('/').to_owned() }
    }

    /// Sends once and reads the whole reply.
    fn send(&self, path: &str, token: Option<&str>, key: Option<&str>, body: &Value) -> Result<(StatusCode, String), ureq::Error> {
        let mut req = self.agent.post(format!("{}{path}", self.url));
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
        let (status, text) = self.send(path, token, None, body).context("连接不上服务器")?;
        reply(status, &text)
    }

    /// Calls a tool and returns `(text, data)`. Network failures, including a reply cut off on the
    /// way back, are retried with the same idempotency key, so a change that reached the server
    /// applies once.
    fn tool(&self, name: &str, token: &str, args: &Value) -> anyhow::Result<(String, Value)> {
        let (path, key) = (format!("/api/v1/tools/{name}"), uuid::Uuid::now_v7().to_string());
        let mut attempt = 0;
        let (status, text) = loop {
            match self.send(&path, Some(token), Some(&key), args) {
                Ok(res) => break res,
                Err(e) if attempt < 2 => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(500 * attempt));
                    eprintln!("{}", printable(&format!("连接服务器出错（{e}），重试第 {attempt} 次")));
                }
                Err(e) => return Err(e).context("连接不上服务器"),
            }
        };
        match reply(status, &text)? {
            Value::Object(mut reply) => match (reply.remove("text"), reply.remove("data")) {
                (Some(Value::String(text)), Some(data)) => Ok((text, data)),
                _ => bail!("服务器的回复里缺少 text 或 data"),
            },
            _ => bail!("服务器的回复格式不对"),
        }
    }
}

/// The body of a successful reply, or the error the server described.
fn reply(status: StatusCode, text: &str) -> anyhow::Result<Value> {
    if status == StatusCode::NO_CONTENT {
        return Ok(Value::Null);
    }
    if status.is_success() {
        return serde_json::from_str(text).context("服务器的回复不是 JSON");
    }
    // A proxy in between may answer with a page of its own.
    let reply: Value = serde_json::from_str(text).unwrap_or_default();
    let message = reply["message"].as_str().map_or_else(|| format!("服务器返回 {status}"), str::to_owned);
    match reply["details"]["field"].as_str() {
        Some(field) => bail!("{message}（{field}）"),
        None => bail!("{message}"),
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

fn call(name: &str, args: Value, json_out: bool) -> anyhow::Result<()> {
    let (url, token) = credentials()?;
    let (text, data) = Server::new(&url).tool(name, &token, &args)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&data)?);
    } else {
        println!("{}", printable(&text));
    }
    Ok(())
}

fn login(url: Option<String>, name: String, read_only: bool) -> anyhow::Result<()> {
    let url = url.or_else(|| read_config().ok().flatten().map(|c| c.url)).context("用 --url 指定服务器地址")?;
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
    let session = session["secret"].as_str().ok_or_else(|| anyhow!("服务器的回复里没有会话"))?.to_owned();
    let scope = if read_only { "read" } else { "write" };
    let made = server.post("/api/v1/tokens", Some(&session), &json!({ "name": name, "scope": scope }));
    // The session only existed to make the token.
    server.post("/api/v1/auth/logout", Some(&session), &json!({}))?;
    let token = made?["secret"].as_str().ok_or_else(|| anyhow!("服务器的回复里没有令牌"))?.to_owned();
    let path = write_config(&Config { url: server.url.clone(), token })?;
    println!("{}", printable(&format!("已登录，令牌“{name}”保存在 {}", path.display())));
    Ok(())
}

fn plan(file: &PathBuf) -> anyhow::Result<Value> {
    let mut text = String::new();
    if file.as_os_str() == "-" {
        std::io::stdin().read_to_string(&mut text)?;
    } else {
        text = std::fs::read_to_string(file).with_context(|| format!("读不了 {}", file.display()))?;
    }
    serde_saphyr::from_str(&text).map_err(|e| anyhow!("计划不是合法的 YAML：{e}"))
}

fn run(cli: Cli) -> anyhow::Result<()> {
    let j = cli.json;
    match cli.command {
        Command::Login { url, name, read_only } => login(url, name, read_only),
        Command::Tree => call("tree", json!({}), j),
        Command::Ls { folder, all } => call("list", json!({ "folder": folder, "all": all }), j),
        Command::Next => call("next", json!({}), j),
        Command::Show { task } => call("show", json!({ "task": task }), j),
        Command::Search { query } => call("search", json!({ "query": query.join(" ") }), j),
        Command::Graph { folder, all, mermaid } => call("graph", json!({ "folder": folder, "all": all, "mermaid": mermaid }), j),
        Command::Log { limit } => call("log", json!({ "limit": limit }), j),
        Command::Add { title, folder, description, due, remind, after, before } => call(
            "add",
            json!({ "title": title, "folder": folder, "description": description, "due": due, "remind": remind, "after": after, "before": before }),
            j,
        ),
        Command::Edit { task, title, description, due, no_due, remind, no_remind } => {
            let mut args = Map::new();
            args.insert("task".into(), task.into());
            if let Some(title) = title {
                args.insert("title".into(), title.into());
            }
            if let Some(description) = description {
                args.insert("description".into(), description.into());
            }
            if no_due || due.is_some() {
                args.insert("due".into(), due.into());
            }
            if no_remind || remind.is_some() {
                args.insert("remind".into(), remind.into());
            }
            call("edit", args.into(), j)
        }
        Command::Done { tasks } => call("done", json!({ "tasks": tasks }), j),
        Command::Reopen { tasks } => call("reopen", json!({ "tasks": tasks }), j),
        Command::Mkdir { path } => call("mkdir", json!({ "path": path }), j),
        Command::Rename { folder, name } => call("rename", json!({ "folder": folder, "name": name }), j),
        Command::Mv { items: mut list } => {
            let to = list.pop().expect("at least two arguments");
            let (tasks, folders) = items(&list);
            call("move", json!({ "tasks": tasks, "folders": folders, "to": to }), j)
        }
        Command::Rm { items: list } => {
            let (tasks, folders) = items(&list);
            call("delete", json!({ "tasks": tasks, "folders": folders }), j)
        }
        Command::Dep { command: Dep::Add { before, after } } => call("dep_add", json!({ "before": before, "after": after }), j),
        Command::Dep { command: Dep::Rm { before, after } } => call("dep_remove", json!({ "before": before, "after": after }), j),
        Command::Apply { file, dry_run } => call("apply", json!({ "plan": plan(&file)?, "dry_run": dry_run }), j),
        Command::Undo { batch } => call("undo", json!({ "batch": batch }), j),
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
