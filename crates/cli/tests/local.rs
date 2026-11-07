//! The command line and its MCP bridge reaching the desktop app in local mode, through the address
//! file the app writes in its data.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nexttodo_server::local::Local;
use serde_json::{Value, json};

/// A home directory of its own in place of the user's, where the app's data goes.
struct Home(PathBuf);

impl Home {
    fn new() -> Self {
        Home(std::env::temp_dir().join(format!("nexttodo-cli-{}", uuid::Uuid::now_v7())))
    }

    /// Local mode's directory in the desktop app's data.
    fn local(&self) -> PathBuf {
        if cfg!(windows) {
            self.0.join("cn.juniexd.nexttodo/local")
        } else {
            self.0.join("Library/Application Support/cn.juniexd.nexttodo/local")
        }
    }

    /// The command line in local mode, started from a terminal that sets `proxy` for everything.
    fn nexttodo(&self, token: &str, proxy: &str) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_nexttodo"));
        c.env("HOME", &self.0).env("APPDATA", &self.0).env("NEXTTODO_URL", "local").env("NEXTTODO_TOKEN", token);
        for var in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
            c.env(var, proxy);
        }
        c.env_remove("NO_PROXY").env_remove("no_proxy");
        c
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A stand-in proxy, which counts the connections it gets.
fn proxy() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    std::thread::spawn(move || {
        for _ in listener.incoming() {
            counted.fetch_add(1, Ordering::SeqCst);
        }
    });
    (url, count)
}

/// Whether it succeeded, and what it printed and said on stderr.
fn run(command: &mut Command) -> (bool, String, String) {
    let out = command.output().unwrap();
    (out.status.success(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

/// The tasks ready to do, asked of the app the way its page does.
fn next(local: &Local, token: &str) -> String {
    let agent: ureq::Agent = ureq::Agent::config_builder().proxy(None).build().into();
    let mut res = agent
        .post(format!("{}/api/v1/tools/next", local.url))
        .header("Authorization", format!("Bearer {token}"))
        .send_json(json!({}))
        .unwrap();
    res.body_mut().read_json::<Value>().unwrap()["text"].as_str().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn commands_reach_the_app_and_nothing_else() {
    let home = Home::new();
    let local = Local::start(&home.local()).await.unwrap();
    let token = local.session().await.unwrap();
    let (proxy, proxied) = proxy();
    let (ok, _, err) = run(home.nexttodo(&token, &proxy).args(["add", "写测试"]));
    assert!(ok, "{err}");
    let (ok, out, err) = run(home.nexttodo(&token, &proxy).arg("next"));
    assert!(ok, "{err}");
    assert!(out.contains("写测试"), "{out}");
    assert_eq!(proxied.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_sent_where_the_app_no_longer_is() {
    let home = Home::new();
    let (proxy, proxied) = proxy();
    let (ok, _, err) = run(home.nexttodo("nt_none", &proxy).arg("next"));
    assert!(!ok);
    assert!(err.contains("无法连接本机模式的 NextTodo"), "{err}");

    let local = Local::start(&home.local()).await.unwrap();
    let token = local.session().await.unwrap();
    let file = home.local().join("address");
    let address: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let with_url = |url: String| std::fs::write(&file, json!({ "url": url, "ca": address["ca"] }).to_string()).unwrap();

    // The port of a run that ended, now another run's: the other one gets nothing.
    let other_home = Home::new();
    let other = Local::start(&other_home.local()).await.unwrap();
    let other_token = other.session().await.unwrap();
    let other_address: Value = serde_json::from_str(&std::fs::read_to_string(other_home.local().join("address")).unwrap()).unwrap();
    with_url(other_address["url"].as_str().unwrap().to_owned());
    let (ok, _, err) = run(home.nexttodo(&token, &proxy).args(["add", "不该出现"]));
    assert!(!ok);
    assert!(err.contains("证书不匹配"), "{err}");
    assert!(!next(&other, &other_token).contains("不该出现"));

    // A port nothing listens on any more.
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    with_url(format!("https://127.0.0.1:{closed}"));
    let (ok, _, err) = run(home.nexttodo(&token, &proxy).arg("next"));
    assert!(!ok);
    assert!(err.contains("无法连接本机模式的 NextTodo"), "{err}");

    // Anywhere but 127.0.0.1, or with more than a port.
    for url in ["https://localhost:443".to_owned(), format!("https://127.0.0.1:{closed}/x"), format!("http://127.0.0.1:{closed}")] {
        with_url(url);
        let (ok, _, err) = run(home.nexttodo(&token, &proxy).arg("next"));
        assert!(!ok);
        assert!(err.contains("格式错误"), "{err}");
    }
    assert_eq!(proxied.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn mcp_reads_the_address_again_for_each_message() {
    let home = Home::new();
    let local = Local::start(&home.local()).await.unwrap();
    let token = local.session().await.unwrap();
    let (proxy, _) = proxy();
    let mut mcp =
        home.nexttodo(&token, &proxy).arg("mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut input = mcp.stdin.take().unwrap();
    let mut output = BufReader::new(mcp.stdout.take().unwrap()).lines();
    let mut ask = |message: Value| {
        writeln!(input, "{message}").unwrap();
        message.get("id").map(|_| serde_json::from_str::<Value>(&output.next().unwrap().unwrap()).unwrap())
    };
    let init = json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } });
    let reply = ask(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init })).unwrap();
    assert!(reply["result"]["protocolVersion"].is_string(), "{reply}");
    // Accepted without a reply: the next line is the next request's.
    assert!(ask(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
    let add = json!({ "name": "add", "arguments": { "title": "从 MCP 添加" } });
    let reply = ask(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": add })).unwrap();
    assert_eq!(reply["id"], 2);
    assert_eq!(reply["result"]["isError"], false, "{reply}");

    // The app quit: the request gets an error of its own.
    let file = home.local().join("address");
    std::fs::rename(&file, file.with_extension("away")).unwrap();
    let reply = ask(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" })).unwrap();
    assert_eq!(reply["id"], 3);
    assert!(reply["error"]["message"].as_str().unwrap().contains("无法连接本机模式的 NextTodo"), "{reply}");

    // And it is back.
    std::fs::rename(file.with_extension("away"), &file).unwrap();
    let reply = ask(json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list" })).unwrap();
    assert!(reply["result"]["tools"].as_array().is_some_and(|t| !t.is_empty()), "{reply}");
    drop(input);
    assert!(mcp.wait().unwrap().success());
}
