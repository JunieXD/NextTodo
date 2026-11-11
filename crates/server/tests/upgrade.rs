//! Upgrading from the settings, against a registry that works as GHCR does: a token asked for after
//! a 401, blobs redirected to where no token goes. Unix only: the server in the image is a script.
#![cfg(unix)]

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use common::TestApp;
use flate2::Compression;
use flate2::write::GzEncoder;
use nexttodo_server::upgrade::{self, Upgrade, Version};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const TOKEN: &str = "t0k";
const LAYER: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
const MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

fn v(s: &str) -> Version {
    s.parse().unwrap()
}

fn temp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nexttodo-upgrade-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// A gzipped tar file of `(path, contents, mode)`; a path ending in `/` is a directory. The paths are
/// written as they are: `tar` itself refuses those an image must not have.
fn layer(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (path, data, mode) in files {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(if path.ends_with('/') { tar::EntryType::Directory } else { tar::EntryType::Regular });
        h.set_mode(*mode);
        h.set_size(data.len() as u64);
        h.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
        h.set_cksum();
        tar.append(&h, *data).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

/// The server's layer: a script that says it is `version`.
fn server(version: &str) -> Vec<u8> {
    layer(&[("nexttodo-server", format!("#!/bin/sh\necho nexttodo-server {version}\n").as_bytes(), 0o755)])
}

fn web() -> Vec<u8> {
    layer(&[("web/", b"", 0o755), ("web/index.html", b"<p>new</p>", 0o644)])
}

/// What the registry holds of a release: its index under the tag, the rest by digest.
#[derive(Clone)]
struct Image {
    tag: String,
    index: Vec<u8>,
    blobs: HashMap<String, Vec<u8>>,
}

/// The release `tag` with `layers`, for both architectures, served as `blobs` says: as they are,
/// unless a test changes them.
fn image(tag: &str, layers: &[Vec<u8>]) -> Image {
    let mut blobs = HashMap::new();
    let descriptors: Vec<Value> = layers
        .iter()
        .map(|l| {
            blobs.insert(digest(l), l.clone());
            json!({ "mediaType": LAYER, "digest": digest(l), "size": l.len() })
        })
        .collect();
    let manifest = serde_json::to_vec(&json!({ "schemaVersion": 2, "mediaType": MANIFEST, "layers": descriptors })).unwrap();
    let platforms = ["amd64", "arm64"].map(|arch| {
        json!({ "mediaType": MANIFEST, "digest": digest(&manifest), "size": manifest.len(), "platform": { "os": "linux", "architecture": arch } })
    });
    let index = serde_json::to_vec(&json!({ "schemaVersion": 2, "manifests": platforms })).unwrap();
    blobs.insert(digest(&manifest), manifest);
    Image { tag: tag.into(), index, blobs }
}

type Served = State<(Arc<Image>, String)>;

/// Serves `image` as a registry; returns its address.
async fn registry(image: Image) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/token", get(token))
        .route("/v2/juniexd/nexttodo/manifests/{reference}", get(manifests))
        .route("/v2/juniexd/nexttodo/blobs/{digest}", get(blobs))
        .route("/cdn/{digest}", get(cdn))
        .with_state((Arc::new(image), base.clone()));
    tokio::spawn(axum::serve(listener, app).into_future());
    base
}

async fn token(Query(q): Query<HashMap<String, String>>) -> Response {
    if q.get("scope").map(String::as_str) != Some("repository:juniexd/nexttodo:pull")
        || q.get("service").map(String::as_str) != Some("test")
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    axum::Json(json!({ "token": TOKEN })).into_response()
}

/// Without the token, the challenge that says where to get it.
fn unauthorized(headers: &HeaderMap, base: &str) -> Option<Response> {
    if headers.get(header::AUTHORIZATION).is_some_and(|v| v == &*format!("Bearer {TOKEN}")) {
        return None;
    }
    let challenge = format!(r#"Bearer realm="{base}/token",service="test",scope="repository:juniexd/nexttodo:pull""#);
    Some((StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, challenge)]).into_response())
}

async fn manifests(State((image, base)): Served, UrlPath(reference): UrlPath<String>, headers: HeaderMap) -> Response {
    if let Some(no) = unauthorized(&headers, &base) {
        return no;
    }
    let body = if reference == image.tag { Some(image.index.clone()) } else { image.blobs.get(&reference).cloned() };
    body.map_or(StatusCode::NOT_FOUND.into_response(), IntoResponse::into_response)
}

async fn blobs(State((_, base)): Served, UrlPath(digest): UrlPath<String>, headers: HeaderMap) -> Response {
    unauthorized(&headers, &base).unwrap_or_else(|| Redirect::temporary(&format!("/cdn/{digest}")).into_response())
}

/// As GHCR's storage, which takes no token besides its own in the address.
async fn cdn(State((image, _)): Served, UrlPath(digest): UrlPath<String>, headers: HeaderMap) -> Response {
    if headers.contains_key(header::AUTHORIZATION) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    image.blobs.get(&digest).cloned().map_or(StatusCode::NOT_FOUND.into_response(), IntoResponse::into_response)
}

/// Starts upgrading a server of 0.1.900 to `image`'s 0.1.901 and waits until the download is over.
async fn upgrade_to(image: Image) -> (Value, PathBuf) {
    let data = temp();
    let upgrade = Arc::new(Upgrade::new(data.clone(), v("0.1.900"), registry(image).await).unwrap());
    let app = TestApp::with_upgrade(upgrade);
    let admin = app.admin().await;
    let started = app.call("POST", "/admin/upgrade", Some(&admin), Some(json!({ "version": "0.1.901" }))).await;
    assert_eq!(started.body["state"], "downloading", "{}", started.body);
    for _ in 0..200 {
        let status = app.call("GET", "/admin/upgrade", Some(&admin), None).await.body;
        if status["state"] != "downloading" {
            return (status, data);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the download did not end");
}

/// Only the version, nothing half-done.
fn versions(data: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        std::fs::read_dir(data.join("versions")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    names
}

#[tokio::test(flavor = "multi_thread")]
async fn a_release_is_downloaded_checked_and_ready_to_switch_to() {
    let (status, data) = upgrade_to(image("0.1.901", &[server("0.1.901"), web()])).await;
    assert_eq!(status, json!({ "state": "switching", "version": "0.1.901" }));
    // Under this name until the switch is noted.
    assert_eq!(versions(&data), ["0.1.901.part"]);
    let dir = data.join("versions/0.1.901.part");
    assert_eq!(std::fs::read_to_string(dir.join("web/index.html")).unwrap(), "<p>new</p>");
    let out = std::process::Command::new(dir.join("nexttodo-server")).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "nexttodo-server 0.1.901");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_release_that_does_not_check_out_is_not_kept() {
    let mut changed = image("0.1.901", &[server("0.1.901"), web()]);
    let web_digest = digest(&web());
    changed.blobs.insert(web_digest, layer(&[("web/", b"", 0o755), ("web/index.html", b"<p>other</p>", 0o644)]));
    let escaping = image("0.1.901", &[server("0.1.901"), layer(&[("web/../../escaped", b"x", 0o644)])]);
    let elsewhere = image("0.1.901", &[server("0.1.901"), layer(&[("etc/passwd", b"x", 0o644)])]);
    let misnamed = image("0.1.901", &[server("0.1.902"), web()]);
    let without_web = image("0.1.901", &[server("0.1.901")]);
    let missing = image("0.1.902", &[server("0.1.901"), web()]);
    let cases = [
        ("changed", changed),
        ("escaping", escaping),
        ("elsewhere", elsewhere),
        ("misnamed", misnamed),
        ("without web", without_web),
        ("missing", missing),
    ];
    for (what, image) in cases {
        let (status, data) = upgrade_to(image).await;
        assert_eq!(status["state"], "failed", "{what}: {status}");
        assert_eq!(status["version"], "0.1.901", "{what}");
        assert!(versions(&data).is_empty(), "{what}: {:?}", versions(&data));
    }
}

#[tokio::test]
async fn only_an_administrator_signed_in_upgrades_and_only_to_a_newer_version() {
    let upgrade = Arc::new(Upgrade::new(temp(), v("0.1.900"), "http://127.0.0.1:9".into()).unwrap());
    let app = TestApp::with_upgrade(upgrade);
    let admin = app.admin().await;
    let user = app.user(&admin, "bob").await;
    let token = app.call("POST", "/tokens", Some(&admin), Some(json!({ "name": "agent", "scope": "write" }))).await.body["secret"]
        .as_str()
        .unwrap()
        .to_owned();
    let to = json!({ "version": "0.1.901" });
    for who in [&user, &token] {
        assert_eq!(app.call("GET", "/admin/upgrade", Some(who), None).await.status, StatusCode::FORBIDDEN);
        assert_eq!(app.call("POST", "/admin/upgrade", Some(who), Some(to.clone())).await.status, StatusCode::FORBIDDEN);
    }
    for old in ["0.1.900", "0.1.89", "0.1.0901"] {
        let r = app.call("POST", "/admin/upgrade", Some(&admin), Some(json!({ "version": old }))).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{old}");
    }
    assert_eq!(app.call("GET", "/admin/upgrade", Some(&admin), None).await.body, json!({ "state": "idle" }));

    // Where the server can't upgrade itself, it says so.
    let plain = TestApp::new();
    let admin = plain.admin().await;
    assert_eq!(plain.call("GET", "/admin/upgrade", Some(&admin), None).await.body, json!({ "state": "unsupported" }));
}

/// The note a version writes before switching, which another, older one may read: its form stays.
#[tokio::test]
async fn a_switch_that_did_not_start_goes_back_to_the_database_from_before() {
    let data = temp();
    let database = data.join("nexttodo.db");
    std::fs::write(&database, "migrated by 0.1.901").unwrap();
    std::fs::write(data.join("nexttodo.db-wal"), "0.1.901's log").unwrap();
    std::fs::create_dir_all(data.join("snapshots")).unwrap();
    let before = data.join("snapshots/before-0.1.901.db");
    std::fs::write(&before, "as 0.1.900 left it").unwrap();
    std::fs::create_dir_all(data.join("versions/0.1.901")).unwrap();
    std::fs::write(data.join("versions/trying"), json!({ "version": "0.1.901", "before": before }).to_string()).unwrap();

    upgrade::recover(&data, &database).unwrap();

    assert_eq!(std::fs::read_to_string(&database).unwrap(), "as 0.1.900 left it");
    assert!(!data.join("nexttodo.db-wal").exists());
    assert_eq!(versions(&data), ["failed"]);
    // The settings of the version gone back to tell; once on that version or a newer, no more.
    let app = TestApp::with_upgrade(Arc::new(Upgrade::new(data.clone(), v("0.1.900"), String::new()).unwrap()));
    let admin = app.admin().await;
    let status = app.call("GET", "/admin/upgrade", Some(&admin), None).await.body;
    assert_eq!((&status["state"], &status["version"]), (&json!("failed"), &json!("0.1.901")));
    Upgrade::new(data.clone(), v("0.1.901"), String::new()).unwrap();
    assert!(versions(&data).is_empty());
    // Nothing to go back from: nothing changes.
    upgrade::recover(&data, &database).unwrap();
    assert_eq!(std::fs::read_to_string(&database).unwrap(), "as 0.1.900 left it");
}

/// Downloaded but not switched to, as when the server stopped in between: nothing to go back to, so
/// the image's server keeps to itself.
#[test]
fn a_version_not_switched_to_is_not_run() {
    let data = temp();
    std::fs::create_dir_all(data.join("versions/0.1.901.part/web")).unwrap();
    std::fs::write(data.join("versions/0.1.901.part/nexttodo-server"), "").unwrap();
    upgrade::hand_over(&data, v("0.1.900"), false).unwrap();
    assert_eq!(versions(&data), ["0.1.901.part"]);
    upgrade::hand_over(&data, v("0.1.900"), true).unwrap();
    assert!(versions(&data).is_empty());
}
