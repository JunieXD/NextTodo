//! `nexttodo`: the command line for NextTodo. Each command calls one tool on the server, which does
//! all the work; this only sends the arguments and prints the result. See `docs/agent.md`.

mod args;
mod files;
mod mcp;
mod server;

use crate::args::{Cli, Command, Dep, Skill, items};
use crate::files::{attach, download, export, import};
use crate::mcp::mcp;
use crate::server::{Config, LOCAL, Server, connect, read_config, write_config};
use anyhow::{Context, anyhow, bail};
use clap::Parser;
use serde_json::{Map, Value, json};
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

/// The release this was built for, which `deploy/build.sh` gives; the crate's own version otherwise.
const VERSION: &str = match option_env!("NEXTTODO_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

/// The skill of this version, which `skill install` puts where agents find it.
const SKILL: &str = include_str!("../../../skills/nexttodo/SKILL.md");

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

fn plan(file: &PathBuf) -> anyhow::Result<Value> {
    let mut text = String::new();
    if file.as_os_str() == "-" {
        std::io::stdin().read_to_string(&mut text)?;
    } else {
        text = std::fs::read_to_string(file).with_context(|| format!("无法读取 {}", file.display()))?;
    }
    serde_saphyr::from_str(&text).map_err(|e| anyhow!("计划不是有效的 YAML：{e}"))
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
        Command::Agenda { days, folders } => read("agenda", json!({ "days": days, "folders": folders }), j),
        Command::Show { task } => read("show", json!({ "task": task }), j),
        Command::Search { query, limit } => read("search", json!({ "query": query.join(" "), "limit": limit }), j),
        Command::Graph { folder, all, mermaid } => read("graph", json!({ "folder": folder, "all": all, "mermaid": mermaid }), j),
        Command::Log { batch, limit, before, deleted } => {
            let mut args = json!({ "limit": limit, "before": before, "deleted": deleted });
            // Only for one batch: servers before it turn down arguments they don't know.
            if let Some(batch) = batch {
                args["batch"] = batch.into();
            }
            read("log", args, j)
        }
        Command::Jobs => read("jobs", json!({}), j),
        Command::Prefs {
            remind_timed,
            no_remind_timed,
            remind_dated,
            no_remind_dated,
            accent,
            no_accent,
            time_grid,
            holidays,
            keep_done,
            add_to,
            no_add_to,
        } => {
            let mut args = json!({});
            let cleared = [
                ("remind_timed", remind_timed.map(Value::from), no_remind_timed),
                ("remind_dated", remind_dated.map(Value::from), no_remind_dated),
                ("accent", accent.map(Value::from), no_accent),
                ("add_to", add_to.map(Value::from), no_add_to),
            ];
            let set = [
                ("time_grid", time_grid.map(Value::from)),
                ("holidays", holidays.map(Value::from)),
                ("keep_done", keep_done.map(Value::from)),
            ];
            for (key, value, clear) in cleared.into_iter().chain(set.into_iter().map(|(k, v)| (k, v, false))) {
                if clear {
                    args[key] = Value::Null;
                } else if let Some(v) = value {
                    args[key] = v;
                }
            }
            if args == json!({}) { read("prefs", args, j) } else { write("set_prefs", args, j) }
        }
        Command::Sync => read("sync", json!({}), j),
        Command::Add {
            title,
            folder,
            description,
            start,
            due,
            due_after,
            remind,
            repeat,
            repeat_from,
            repeat_until,
            assign,
            after,
            before,
        } => write(
            "add",
            json!({
                "title": title, "folder": folder, "description": description, "start": start, "due": due, "due_after": due_after,
                "remind": (!remind.is_empty()).then_some(remind), "repeat": repeat, "repeat_from": repeat_from, "repeat_until": repeat_until, "assign": assign, "after": after, "before": before
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
            due_after,
            no_due,
            remind,
            no_remind,
            repeat,
            repeat_from,
            repeat_until,
            no_repeat,
            assign,
            unassign,
            done_at,
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
            if let Some(after) = due_after {
                args.insert("due_after".into(), after.into());
            }
            if no_remind || !remind.is_empty() {
                args.insert("remind".into(), (!no_remind).then_some(remind).into());
            }
            if no_repeat || repeat.is_some() {
                args.insert("repeat".into(), repeat.into());
            }
            if let Some(from) = repeat_from {
                args.insert("repeat_from".into(), from.into());
            }
            if let Some(until) = repeat_until {
                args.insert("repeat_until".into(), until.into());
            }
            if !assign.is_empty() {
                args.insert("assign".into(), assign.into());
            }
            if !unassign.is_empty() {
                args.insert("unassign".into(), unassign.into());
            }
            if !done_at.is_empty() {
                args.insert("done_at".into(), done_at.join(" ").into());
            }
            write("edit", args.into(), j)
        }
        Command::Done { tasks, end_repeat, at } => {
            let mut args = json!({ "tasks": tasks, "end_repeat": end_repeat });
            if !at.is_empty() {
                args["at"] = at.join(" ").into();
            }
            write("done", args, j)
        }
        Command::Drop { tasks, with_after } => write("drop", json!({ "tasks": tasks, "with_after": with_after }), j),
        Command::Reopen { tasks } => write("reopen", json!({ "tasks": tasks }), j),
        Command::Skip { tasks } => write("skip", json!({ "tasks": tasks }), j),
        Command::Mkdir { path } => write("mkdir", json!({ "path": path }), j),
        Command::Rename { target, name, attachment: true } => write("rename", json!({ "attachment": target, "name": name }), j),
        Command::Rename { target, name, attachment: false } => write("rename", json!({ "folder": target, "name": name }), j),
        Command::Attach { task, files, name } => attach(&task, &files, name, j),
        Command::Detach { attachments } => write("detach", json!({ "attachments": attachments }), j),
        Command::Download { id, output } => download(&id, output, j.json),
        Command::Export { file } => export(file, j.json),
        Command::Import { file } => import(&file, j),
        Command::Mv { items: mut list } => {
            let to = list.pop().expect("at least two arguments");
            let (tasks, folders) = items(&list);
            write("move", json!({ "tasks": tasks, "folders": folders, "to": to }), j)
        }
        Command::Style { folder, color, no_color, icon, no_icon } => {
            let mut args = json!({ "folder": folder });
            for (key, value, clear) in [("color", color, no_color), ("icon", icon, no_icon)] {
                if clear {
                    args[key] = Value::Null;
                } else if let Some(v) = value {
                    args[key] = v.into();
                }
            }
            write("style", args, j)
        }
        Command::Cp { folder, to, name } => write("copy", json!({ "folder": folder, "to": to, "name": name }), j),
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
