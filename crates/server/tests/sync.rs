mod common;

use std::net::SocketAddr;
use std::time::Duration;

use axum::http::StatusCode;
use common::{TestApp, id};
use futures_util::{SinkExt, StreamExt};
use nexttodo_server::{MUTATION_RETENTION_MS, db};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

async fn serve(t: &TestApp) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = t.app.clone();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

struct Client(WebSocketStream<MaybeTlsStream<TcpStream>>);

impl Client {
    /// Connects as the web app does, from a page of this server.
    async fn connect(addr: SocketAddr, cookie: Option<&str>) -> Self {
        Self::from_page(addr, cookie, Some(&format!("http://{addr}"))).await
    }

    /// Connects as a browser would from a page at `origin`.
    async fn from_page(addr: SocketAddr, cookie: Option<&str>, origin: Option<&str>) -> Self {
        let mut req = format!("ws://{addr}/api/v1/sync").into_client_request().unwrap();
        if let Some(secret) = cookie {
            req.headers_mut().insert("Cookie", format!("nt_session={secret}").parse().unwrap());
        }
        if let Some(origin) = origin {
            req.headers_mut().insert("Origin", origin.parse().unwrap());
        }
        Client(tokio_tungstenite::connect_async(req).await.unwrap().0)
    }

    async fn send(&mut self, msg: Value) {
        self.0.send(Message::text(msg.to_string())).await.unwrap();
    }

    /// The next JSON message, or `None` once the server closes the connection.
    async fn recv(&mut self) -> Option<Value> {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), self.0.next()).await.expect("a message within 5 s") {
                Some(Ok(Message::Text(text))) => return Some(serde_json::from_str(&text).unwrap()),
                Some(Ok(Message::Close(_))) | None => return None,
                Some(Ok(_)) => {}
                Some(Err(e)) => panic!("{e}"),
            }
        }
    }

    /// Says hello as `user` and returns the first reply.
    async fn hello(&mut self, user: &str, token: Option<&str>, since: i64) -> Value {
        self.send(json!({ "type": "hello", "token": token, "since": since, "user": user })).await;
        self.recv().await.unwrap()
    }
}

#[tokio::test]
async fn every_connection_gets_every_change() {
    let t = TestApp::new();
    let s = t.admin().await;
    let me = t.me(&s).await;
    let addr = serve(&t).await;
    let mut web = Client::connect(addr, Some(&s)).await;
    assert_eq!(web.hello(&me, None, 0).await, json!({ "type": "changes", "seq": 0, "changes": [] }), "synced, nothing new");
    let mut app = Client::connect(addr, None).await;
    app.hello(&me, Some(&s), 0).await;

    let (mutation, task) = (id(), id());
    web.send(json!({ "type": "mutate", "mutation": { "id": mutation, "ops": [{ "op": "task.create", "id": task, "title": "交电费" }] } }))
        .await;
    let mut replies = [web.recv().await.unwrap(), web.recv().await.unwrap()];
    replies.sort_by_key(|m| m["type"].as_str().unwrap().to_owned());
    assert_eq!(replies[0], json!({ "type": "ack", "id": mutation, "seq": 1 }));
    assert_eq!(replies[1]["changes"][0]["id"], mutation.as_str(), "the sender sees its own change too");
    let pushed = app.recv().await.unwrap();
    assert_eq!((pushed["seq"].as_i64(), pushed["changes"][0]["effects"][0]["after"]["title"].as_str()), (Some(1), Some("交电费")));

    t.mutate(&s, json!([{ "op": "task.update", "id": task, "done": true }])).await;
    for client in [&mut web, &mut app] {
        assert_eq!(client.recv().await.unwrap()["seq"], 2, "changes over HTTP are pushed as well");
    }

    let mut later = Client::connect(addr, Some(&s)).await;
    assert_eq!(later.hello(&me, None, 1).await["changes"].as_array().unwrap().len(), 1, "a reconnect gets only what it missed");
}

#[tokio::test]
async fn bad_mutations_are_rejected_and_bad_connections_closed() {
    let t = TestApp::new();
    let s = t.admin().await;
    let me = t.me(&s).await;
    let token = t.call("POST", "/tokens", Some(&s), Some(json!({ "name": "看板", "scope": "read" }))).await.body;
    let addr = serve(&t).await;

    let mut c = Client::connect(addr, Some(&s)).await;
    c.hello(&me, None, 0).await;
    let bad = id();
    c.send(json!({ "type": "mutate", "mutation": { "id": bad, "ops": [{ "op": "task.create", "id": id(), "title": "  " }] } })).await;
    let reply = c.recv().await.unwrap();
    assert_eq!(
        (reply["type"].as_str(), reply["id"].as_str(), reply["code"].as_str()),
        (Some("reject"), Some(bad.as_str()), Some("invalid"))
    );

    let mut reader = Client::connect(addr, None).await;
    reader.hello(&me, token["secret"].as_str(), 0).await;
    reader.send(json!({ "type": "mutate", "mutation": { "id": id(), "ops": [{ "op": "task.create", "id": id(), "title": "x" }] } })).await;
    assert_eq!(reader.recv().await.unwrap()["code"], "forbidden");

    let mut nobody = Client::connect(addr, None).await;
    assert_eq!(nobody.hello(&me, None, 0).await["code"], "unauthenticated");
    assert_eq!(nobody.recv().await, None);
    let mut rude = Client::connect(addr, Some(&s)).await;
    rude.send(json!({ "type": "mutate", "mutation": {} })).await;
    assert_eq!(rude.recv().await.unwrap()["type"], "error");
    assert_eq!(rude.recv().await, None);
}

#[tokio::test]
async fn a_client_too_far_behind_is_told_to_resync() {
    let t = TestApp::new();
    let s = t.admin().await;
    let me = t.me(&s).await;
    t.mutate(&s, json!([{ "op": "task.create", "id": id(), "title": "一" }])).await;
    t.advance(MUTATION_RETENTION_MS + 1);
    let cutoff = t.state.clock.now() - MUTATION_RETENTION_MS;
    t.state.db.call(move |c| Ok(db::prune(c, cutoff)?)).await.unwrap();
    let addr = serve(&t).await;

    let mut c = Client::connect(addr, Some(&s)).await;
    assert_eq!(c.hello(&me, None, 0).await, json!({ "type": "resync" }));
    assert_eq!(c.recv().await, None, "it reconnects after downloading the snapshot");
    let snap = t.call("GET", "/snapshot", Some(&s), None).await;
    assert_eq!(snap.status, StatusCode::OK);
    assert_eq!(snap.body["user"], me.as_str(), "it says whose data it is");
    let mut again = Client::connect(addr, Some(&s)).await;
    assert_eq!(again.hello(&me, None, snap.body["seq"].as_i64().unwrap()).await["changes"], json!([]));
}

#[tokio::test]
async fn a_revoked_token_stops_getting_changes() {
    let t = TestApp::new();
    let s = t.admin().await;
    let me = t.me(&s).await;
    let token = t.call("POST", "/tokens", Some(&s), Some(json!({ "name": "看板", "scope": "read" }))).await.body;
    let addr = serve(&t).await;
    let mut board = Client::connect(addr, None).await;
    board.hello(&me, token["secret"].as_str(), 0).await;

    let path = format!("/tokens/{}", token["token"]["id"].as_str().unwrap());
    assert_eq!(t.call("DELETE", &path, Some(&s), None).await.status, StatusCode::NO_CONTENT);
    assert_eq!(board.recv().await.unwrap()["code"], "unauthenticated", "told at once, before anything new happens");
    assert_eq!(board.recv().await, None);
}

#[tokio::test]
async fn a_change_after_the_session_ended_is_not_turned_down() {
    let t = TestApp::new();
    let s = t.admin().await;
    let me = t.me(&s).await;
    let addr = serve(&t).await;
    let mut web = Client::connect(addr, Some(&s)).await;
    web.hello(&me, None, 0).await;
    // Signed out a moment ago, before the connection heard of it.
    t.state.db.call(|c| Ok(c.execute("DELETE FROM tokens", [])?)).await.unwrap();
    web.send(json!({ "type": "mutate", "mutation": { "id": id(), "ops": [{ "op": "task.create", "id": id(), "title": "交电费" }] } }))
        .await;
    let reply = web.recv().await.unwrap();
    assert_eq!(
        (&reply["type"], &reply["code"]),
        (&json!("error"), &json!("unauthenticated")),
        "the change stays queued for the next session"
    );
    assert_eq!(web.recv().await, None);
}

#[tokio::test]
async fn the_cookie_only_counts_from_this_site() {
    let t = TestApp::new();
    let s = t.admin().await;
    let me = t.me(&s).await;
    let addr = serve(&t).await;
    let mut other = Client::from_page(addr, Some(&s), Some("https://evil.example.com")).await;
    assert_eq!(other.hello(&me, None, 0).await["code"], "unauthenticated");
    let mut no_page = Client::from_page(addr, Some(&s), None).await;
    assert_eq!(no_page.hello(&me, None, 0).await["code"], "unauthenticated", "browsers always say where the page is");
    let mut own = Client::from_page(addr, Some(&s), Some(&format!("http://{addr}"))).await;
    assert_eq!(own.hello(&me, None, 0).await["type"], "changes");
}

#[tokio::test]
async fn a_local_copy_never_meets_another_account() {
    let t = TestApp::new();
    let s = t.admin().await;
    let me = t.me(&s).await;
    // Another tab signed in as someone else: the cookie now belongs to them.
    let other = t.user(&s, "xiaoming").await;
    let addr = serve(&t).await;
    let mut c = Client::connect(addr, Some(&other)).await;
    assert_eq!(c.hello(&me, None, 0).await["code"], "unauthenticated");
    assert_eq!(c.recv().await, None);
}
