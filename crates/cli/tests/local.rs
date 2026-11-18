//! The command line and its MCP bridge reaching the desktop app in local mode, through the address
//! file the app writes in its data.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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

/// A server that reads each request and gives `answer`, raw, closing the connection unanswered when
/// it is empty: as when the answer is lost on the way back, or a gateway answers in its place. The
/// change ids the requests came with.
fn answering(answer: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let keys = Arc::new(Mutex::new(Vec::new()));
    let heard = keys.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut reader = BufReader::new(stream.unwrap());
            let (mut line, mut length) = (String::new(), 0);
            while reader.read_line(&mut line).unwrap() > 2 {
                match line.split_once(':') {
                    Some((name, value)) if name.eq_ignore_ascii_case("idempotency-key") => {
                        heard.lock().unwrap().push(value.trim().to_owned())
                    }
                    Some((name, value)) if name.eq_ignore_ascii_case("content-length") => length = value.trim().parse().unwrap(),
                    _ => {}
                }
                line.clear();
            }
            reader.read_exact(&mut vec![0; length]).unwrap();
            reader.into_inner().write_all(answer.as_bytes()).unwrap();
        }
    });
    (url, keys)
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
async fn files_attached_download_under_a_name_the_disk_takes_and_never_over_another() {
    let home = Home::new();
    let local = Local::start(&home.local()).await.unwrap();
    let token = local.session().await.unwrap();
    let (proxy, _) = proxy();
    let dir = home.0.join("files");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("notes.txt");
    std::fs::write(&file, "第一稿").unwrap();
    run(home.nexttodo(&token, &proxy).args(["add", "写测试"]));
    let (ok, out, err) = run(home.nexttodo(&token, &proxy).args(["attach", "1"]).arg(&file).args(["--name", "会议 10:30.txt"]));
    assert!(ok && out.contains("添加附件 会议 10:30.txt"), "{out}{err}");

    let (_, shown, _) = run(home.nexttodo(&token, &proxy).args(["--json", "show", "1"]));
    let shown: Value = serde_json::from_str(&shown).unwrap();
    let id = shown["attachments"][0]["id"].as_str().unwrap().to_owned();
    let into = home.0.join("downloads");
    std::fs::create_dir_all(&into).unwrap();
    let (ok, out, err) = run(home.nexttodo(&token, &proxy).args(["--json", "download", &id, "-o"]).arg(&into));
    assert!(ok, "{err}");
    let path = into.join("会议 10_30.txt");
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), json!({ "id": id, "name": "会议 10:30.txt", "path": path }));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "第一稿");
    let (ok, _, err) = run(home.nexttodo(&token, &proxy).args(["download", &id, "-o"]).arg(&into));
    assert!(!ok && err.contains("无法写入"), "{err}");

    let (ok, _, err) = run(home.nexttodo(&token, &proxy).args(["rename", "--attachment", &id, "纪要.txt"]));
    assert!(ok, "{err}");
    // With one file not there, what the one before it did, and a failure.
    let (ok, out, err) =
        run(home.nexttodo(&token, &proxy).args(["--json", "attach", "1"]).arg(&file).arg(dir.join("没有这个.txt")).arg(&file));
    let made: Value = serde_json::from_str(&out).unwrap();
    assert!(!ok && err.contains("没有这个.txt"), "{err}");
    assert_eq!(made.as_array().unwrap().len(), 1, "{made}");
    let (_, shown, _) = run(home.nexttodo(&token, &proxy).args(["--json", "show", "1"]));
    let names: Vec<Value> =
        serde_json::from_str::<Value>(&shown).unwrap()["attachments"].as_array().unwrap().iter().map(|a| a["name"].clone()).collect();
    assert_eq!(names, [json!("纪要.txt"), json!("notes.txt")]);
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

#[test]
fn a_change_whose_answer_is_lost_is_tried_as_one_and_named_to_send_again() {
    // No answer, tried three times; a gateway's, once, with a body of its own or none.
    let gateway = "HTTP/1.1 504 Gateway Timeout\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let with_json = "HTTP/1.1 504 Gateway Timeout\r\nContent-Type: application/json\r\nContent-Length: 40\r\nConnection: close\r\n\r\n{\"code\":504,\"message\":\"Gateway Timeout\"}";
    for (answer, tries) in [("", 3), (gateway, 1), (with_json, 1)] {
        let home = Home::new();
        let (url, keys) = answering(answer);
        let mut command = Command::new(env!("CARGO_BIN_EXE_nexttodo"));
        command.env("HOME", &home.0).env("NEXTTODO_URL", &url).env("NEXTTODO_TOKEN", "nt_test");
        for var in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"] {
            command.env_remove(var);
        }
        let (ok, _, err) = run(command.args(["add", "写测试"]));
        let keys = keys.lock().unwrap().clone();
        assert!(!ok);
        assert_eq!(keys.len(), tries, "{err}");
        assert!(keys.iter().all(|k| *k == keys[0]), "{keys:?}");
        assert!(err.contains(&format!("--request-id {}", keys[0])), "{err}");
    }
}
