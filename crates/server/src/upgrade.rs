//! Upgrading the server from the settings (docs/deploy.md#在设置里升级): a release's image downloaded
//! from the registry into `data/versions/<version>/`, then run in this process's place. The server in
//! the image, which the container starts, hands over to the newest version there, and puts the
//! database back after an upgrade that did not start.

use std::fs::{self, File};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::extract::State;
use serde::{Deserialize, Serialize};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tar::EntryType;
use tokio::sync::watch;
use ts_rs::TS;

use crate::auth::Auth;
use crate::error::{ApiError, Json, for_user};
use crate::{AppState, db};

/// Where `deploy/release.sh` publishes the image.
pub const REGISTRY: &str = "https://ghcr.io";
const REPOSITORY: &str = "juniexd/nexttodo";

const VERSIONS: &str = "versions";
/// The version being switched to and the copy of the database from before: there until it listens.
const TRYING: &str = "trying";
/// The version that did not start, for the settings to tell.
const FAILED: &str = "failed";

/// The longest a switch waits for requests under way, such as those waiting for a model: no one can
/// connect meanwhile. As long as Compose gives a container to stop.
pub const GRACE: Duration = Duration::from_secs(10);

/// A release's version, `0.1.170`: three numbers without leading zeros, so that each has one name, as
/// a directory too.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Version([u32; 3]);

impl std::str::FromStr for Version {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        let bad = || format!("版本号不对：{s}");
        let number = |p: &str| if p.len() > 1 && p.starts_with('0') { None } else { p.parse().ok().filter(|_| p.bytes().all(|b| b.is_ascii_digit())) };
        let parts: Vec<u32> = s.split('.').map(number).collect::<Option<_>>().ok_or_else(bad)?;
        parts.try_into().map(Version).map_err(|_| bad())
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [a, b, c] = self.0;
        write!(f, "{a}.{b}.{c}")
    }
}

impl From<Version> for String {
    fn from(v: Version) -> String {
        v.to_string()
    }
}

impl TryFrom<String> for Version {
    type Error = String;

    fn try_from(s: String) -> Result<Self, String> {
        s.parse()
    }
}

/// This server's version, when it can upgrade itself: a release's, built for Linux on an architecture
/// the image is published for.
pub fn own() -> Option<Version> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    platform()?;
    option_env!("NEXTTODO_VERSION")?.parse().ok()
}

/// The image's name for this architecture.
fn platform() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("amd64"),
        "aarch64" => Some("arm64"),
        _ => None,
    }
}

/// What upgrading is doing, as the settings show it.
#[derive(Clone, Serialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
#[ts(export, rename = "UpgradeStatus")]
pub enum Status {
    /// This server cannot upgrade itself: a development build, say.
    Unsupported,
    Idle,
    /// Of the image's `total` bytes, `received`.
    Downloading {
        #[ts(type = "string")]
        version: Version,
        #[ts(type = "number")]
        received: u64,
        #[ts(type = "number")]
        total: u64,
    },
    /// Downloaded; stopping, to run it.
    Switching {
        #[ts(type = "string")]
        version: Version,
    },
    /// `message` says why, to the user.
    Failed {
        #[ts(type = "string")]
        version: Version,
        message: String,
    },
}

/// What the settings say of a switch to `version` that did not get to listening, the server still on
/// `now`: gone back from it, or stopped before it.
fn not_switched(version: impl std::fmt::Display, now: impl std::fmt::Display) -> String {
    format!("升级到 {version} 失败：没能换上新版本，服务器仍是 {now}，原因见服务器的日志（docker compose logs）")
}

/// Why a download failed: what the network, the image or the disk did, for the log.
enum Failure {
    Unreachable(String),
    Invalid(String),
    Unrunnable(io::Error),
    Storage(io::Error),
}

impl Failure {
    fn message(&self) -> String {
        match self {
            Failure::Unreachable(_) => "升级失败：无法从 GHCR 下载新版本，请检查服务器的网络，或用命令升级".into(),
            Failure::Invalid(_) => "升级失败：下载的内容不完整，请重试".into(),
            Failure::Unrunnable(_) => "升级失败：新版本无法在这台服务器上运行，请用命令升级".into(),
            Failure::Storage(e) => for_user("升级", e),
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Unreachable(why) | Failure::Invalid(why) => f.write_str(why),
            Failure::Unrunnable(e) | Failure::Storage(e) => e.fmt(f),
        }
    }
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Failure::Storage(e)
    }
}

fn unreachable(e: impl std::fmt::Display) -> Failure {
    Failure::Unreachable(e.to_string())
}

fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::Invalid(e.to_string())
}

/// The registry's answer, read as JSON.
fn json<T: serde::de::DeserializeOwned>(res: ureq::http::Response<ureq::Body>) -> Result<T, Failure> {
    serde_json::from_slice(&res.into_body().read_to_vec().map_err(unreachable)?).map_err(invalid)
}

/// The switch under way: where to, and the copy of the database to go back to.
#[derive(Serialize, Deserialize)]
struct Trying {
    version: Version,
    before: PathBuf,
}

/// Upgrading the server of version `own`, with its data in `data`.
pub struct Upgrade {
    data: PathBuf,
    own: Version,
    registry: String,
    status: Mutex<Status>,
    /// The version downloaded, once it is.
    ready: watch::Sender<Option<Version>>,
}

impl Upgrade {
    /// From the image in `registry`; telling of the last upgrade, should it have gone back.
    pub fn new(data: PathBuf, own: Version, registry: String) -> anyhow::Result<Self> {
        let failed = data.join(VERSIONS).join(FAILED);
        let version = match fs::read_to_string(&failed) {
            Ok(v) => Some(v.parse::<Version>().map_err(anyhow::Error::msg)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let status = match version {
            Some(version) if version > own => Status::Failed { version, message: not_switched(version, own) },
            // Since upgraded otherwise, with Compose say.
            Some(_) => {
                fs::remove_file(&failed)?;
                Status::Idle
            }
            None => Status::Idle,
        };
        Ok(Upgrade { data, own, registry, status: Mutex::new(status), ready: watch::channel(None).0 })
    }

    fn status(&self) -> std::sync::MutexGuard<'_, Status> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The version downloaded, to stop serving and switch to: resolves once there is one.
    pub async fn ready(&self) -> Version {
        let mut ready = self.ready.subscribe();
        let version = *ready.wait_for(Option::is_some).await.expect("the sender lives in self");
        version.expect("waited for one")
    }

    /// The version downloaded, to switch to, if there is one.
    pub fn downloaded(&self) -> Option<Version> {
        *self.ready.borrow()
    }

    /// Downloads `version` in the background, unless a download or switch is under way already.
    fn begin(self: &Arc<Self>, version: Version) -> Status {
        let mut status = self.status();
        if !matches!(*status, Status::Downloading { .. } | Status::Switching { .. }) {
            *status = Status::Downloading { version, received: 0, total: 0 };
            let upgrade = self.clone();
            tokio::task::spawn_blocking(move || {
                match upgrade.download(version) {
                    Ok(()) => {
                        *upgrade.status() = Status::Switching { version };
                        upgrade.ready.send_replace(Some(version));
                    }
                    Err(e) => {
                        tracing::warn!(%version, "upgrade failed: {e}");
                        *upgrade.status() = Status::Failed { version, message: e.message() };
                    }
                }
            });
        }
        status.clone()
    }

    /// Downloads the image of `version` into `data/versions/<version>.part/`: from the registry, which
    /// tells where to ask for a token (the OCI distribution spec), each part checked against its
    /// digest. It keeps that name until `switch` has noted the switch: under its own, the image's
    /// server would hand over to it, and with nothing to go back to.
    fn download(&self, version: Version) -> Result<(), Failure> {
        let versions = self.data.join(VERSIONS);
        let part = versions.join(format!("{version}.part"));
        remove(&versions.join(FAILED))?;
        remove(&part)?;
        fs::create_dir_all(&part)?;
        if let Err(e) = self.image(version, &part) {
            remove(&part)?;
            return Err(e);
        }
        Ok(())
    }

    /// Unpacks the image of `version` into `part`, checks that its server runs, and syncs it all: the
    /// switch is noted only after.
    fn image(&self, version: Version, part: &Path) -> Result<(), Failure> {
        let mut registry = Registry { base: self.registry.clone(), token: None, agent: agent() };

        let index: Index = json(registry.get(&format!("manifests/{version}"), INDEX)?)?;
        let arch = platform().expect("upgrades only where there is an image");
        let entry = index
            .manifests
            .iter()
            .find(|m| m.platform.os == "linux" && m.platform.architecture == arch)
            .ok_or_else(|| invalid(format!("no image for linux/{arch}")))?;
        let body = registry.get(&format!("manifests/{}", checked(&entry.digest)?), MANIFEST)?.into_body().read_to_vec().map_err(unreachable)?;
        if !matches(&entry.digest, Sha256::new_with_prefix(&body)) {
            return Err(Failure::Invalid("manifest digest mismatch".into()));
        }
        let manifest: Manifest = serde_json::from_slice(&body).map_err(invalid)?;
        if let Some(l) = manifest.layers.iter().find(|l| l.media_type != LAYER) {
            return Err(Failure::Invalid(format!("layer of type {}", l.media_type)));
        }
        if let Status::Downloading { total, .. } = &mut *self.status() {
            *total = manifest.layers.iter().map(|l| l.size).sum();
        }

        let blob = part.join("layer");
        for layer in &manifest.layers {
            self.fetch(&mut registry, layer, &blob)?;
            unpack(&blob, part)?;
        }
        fs::remove_file(&blob)?;

        let server = part.join("nexttodo-server");
        for needed in [&server, &part.join("web/index.html")] {
            if !needed.try_exists()? {
                return Err(Failure::Invalid(format!("no {} in the image", needed.display())));
            }
        }
        // Where `data/` is on a disk that runs no programs, say, it would not start.
        let said = Command::new(&server).arg("--version").output().map_err(Failure::Unrunnable)?;
        let said = String::from_utf8_lossy(&said.stdout);
        if said.trim() != format!("nexttodo-server {version}") {
            return Err(Failure::Invalid(format!("the server says it is {}", said.trim())));
        }
        Ok(sync_tree(part)?)
    }

    /// Writes the blob of `layer` to `to`, counting what comes, and checks it.
    fn fetch(&self, registry: &mut Registry, layer: &Descriptor, to: &Path) -> Result<(), Failure> {
        let mut body = registry.get(&format!("blobs/{}", checked(&layer.digest)?), "*/*")?.into_body().into_reader();
        let mut file = File::create(to)?;
        let mut hash = Sha256::new();
        let mut buf = vec![0; 64 * 1024];
        loop {
            let n = body.read(&mut buf).map_err(unreachable)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            file.write_all(&buf[..n])?;
            if let Status::Downloading { received, .. } = &mut *self.status() {
                *received += n as u64;
            }
        }
        if matches(&layer.digest, hash) { Ok(()) } else { Err(Failure::Invalid(format!("layer {} digest mismatch", layer.digest))) }
    }
}

const INDEX: &str = "application/vnd.oci.image.index.v1+json";
const MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const LAYER: &str = "application/vnd.oci.image.layer.v1.tar+gzip";

#[derive(Deserialize)]
struct Index {
    manifests: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    digest: String,
    platform: Platform,
}

#[derive(Deserialize)]
struct Platform {
    os: String,
    architecture: String,
}

#[derive(Deserialize)]
struct Manifest {
    layers: Vec<Descriptor>,
}

#[derive(Deserialize)]
struct Descriptor {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
    size: u64,
}

/// A digest from the registry, which goes into a URL: `sha256:` and 64 hex digits.
fn checked(digest: &str) -> Result<&str, Failure> {
    let hex = digest.strip_prefix("sha256:").filter(|h| h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
    hex.map(|_| digest).ok_or_else(|| Failure::Invalid(format!("digest {digest}")))
}

fn matches(digest: &str, hash: Sha256) -> bool {
    let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    digest.strip_prefix("sha256:") == Some(hex.as_str())
}

/// Long downloads on slow lines are fine; one that stops is not.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_per_read(Some(Duration::from_secs(60)))
        .build()
        .into()
}

struct Registry {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl Registry {
    /// `path` of the repository, with a token once the registry asks for one. Blobs come from where it
    /// redirects, which gets no token.
    fn get(&mut self, path: &str, accept: &str) -> Result<ureq::http::Response<ureq::Body>, Failure> {
        let url = format!("{}/v2/{REPOSITORY}/{path}", self.base);
        let mut res = self.send(&url, accept)?;
        if res.status() == 401 && self.token.is_none() {
            let challenge = res.headers().get("www-authenticate").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
            self.token = Some(self.authorize(&challenge)?);
            res = self.send(&url, accept)?;
        }
        if res.status().is_success() { Ok(res) } else { Err(Failure::Unreachable(format!("{url}: {}", res.status()))) }
    }

    fn send(&self, url: &str, accept: &str) -> Result<ureq::http::Response<ureq::Body>, Failure> {
        let mut req = self.agent.get(url).header("Accept", accept);
        if let Some(token) = &self.token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req.call().map_err(unreachable)
    }

    /// An anonymous token for pulling, from the realm a `Bearer realm="…",service="…",scope="…"`
    /// challenge names.
    fn authorize(&self, challenge: &str) -> Result<String, Failure> {
        let params = challenge.strip_prefix("Bearer ").ok_or_else(|| Failure::Unreachable(format!("challenge {challenge}")))?;
        let params: Vec<(&str, &str)> = params.split(',').filter_map(|p| p.split_once('=')).map(|(k, v)| (k.trim(), v.trim().trim_matches('"'))).collect();
        let realm = params.iter().find(|(k, _)| *k == "realm").ok_or_else(|| Failure::Unreachable(format!("challenge {challenge}")))?.1;
        let mut req = self.agent.get(realm);
        for (k, v) in params.iter().filter(|(k, _)| *k != "realm") {
            req = req.query(*k, *v);
        }
        let res = req.call().map_err(unreachable)?;
        if !res.status().is_success() {
            return Err(Failure::Unreachable(format!("{realm}: {}", res.status())));
        }
        #[derive(Deserialize)]
        struct Token {
            token: String,
        }
        Ok(json::<Token>(res)?.token)
    }
}

/// Unpacks the gzipped tar file `blob` into `dir`: only the server and the web app's files and
/// directories, as the image has them. Anything else, and a path going out of `dir`, voids it.
fn unpack(blob: &Path, dir: &Path) -> Result<(), Failure> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(File::open(blob)?));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let kind = entry.header().entry_type();
        let mut parts = path.components();
        let ours = match parts.next() {
            Some(Component::Normal(p)) if p == "nexttodo-server" => kind == EntryType::Regular && parts.next().is_none(),
            Some(Component::Normal(p)) if p == "web" => {
                matches!(kind, EntryType::Regular | EntryType::Directory) && parts.all(|c| matches!(c, Component::Normal(_)))
            }
            _ => false,
        };
        if !ours {
            return Err(Failure::Invalid(format!("{} in the image", path.display())));
        }
        if kind == EntryType::Directory {
            fs::create_dir_all(dir.join(&path))?;
        } else {
            entry.unpack(dir.join(&path))?;
        }
    }
    Ok(())
}

/// The versions downloaded completely.
fn downloaded(versions: &Path) -> io::Result<Vec<Version>> {
    let mut found = Vec::new();
    for entry in entries(versions)? {
        let entry = entry?;
        if let Some(v) = entry.file_name().to_str().and_then(|n| n.parse().ok())
            && entry.file_type()?.is_dir()
        {
            found.push(v);
        }
    }
    Ok(found)
}

/// Removes the directories in `versions` but `keep`'s: the versions not run, and downloads cut short.
fn tidy(versions: &Path, keep: Option<Version>) -> io::Result<()> {
    for entry in entries(versions)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && keep.is_none_or(|k| entry.file_name() != *k.to_string()) {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

fn entries(dir: &Path) -> io::Result<Vec<io::Result<fs::DirEntry>>> {
    match fs::read_dir(dir) {
        Ok(r) => Ok(r.collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Removes the file or directory at `path`, if there is one.
fn remove(path: &Path) -> io::Result<()> {
    let removed = if path.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
    match removed {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

/// The directory this server runs from, when it is a version in `data/versions/` and not the image's.
/// It serves the web app beside it.
pub fn installed(data: &Path, own: Version) -> io::Result<Option<PathBuf>> {
    let dir = data.join(VERSIONS).join(own.to_string());
    let here = match dir.canonicalize() {
        Ok(d) => std::env::current_exe()?.parent() == Some(&d),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    Ok(here.then_some(dir))
}

/// Whether a switch was noted that has not got to listening: `recover` to go back from it, with the
/// data to itself.
pub fn interrupted(data: &Path) -> io::Result<bool> {
    data.join(VERSIONS).join(TRYING).try_exists()
}

/// Puts the database back after a switch that did not get to listening, before the image's server
/// does anything with it. The copy is from the version switched from, which may be newer than this
/// one: it goes back as it is, not through `db::restore`, which would bring it to this version. Each
/// step is synced before the next, so that cut short, at a power cut say, it is done again.
pub fn recover(data: &Path, database: &Path) -> anyhow::Result<()> {
    let versions = data.join(VERSIONS);
    let trying: Trying = match fs::read(versions.join(TRYING)) {
        Ok(t) => serde_json::from_slice(&t)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    tracing::error!(version = %trying.version, "the upgrade did not start; going back to the database from before it");
    let back = database.with_extension("db.back");
    fs::copy(&trying.before, &back)?;
    File::open(&back)?.sync_all()?;
    // The new version's log goes first: on the copy it would corrupt it.
    for suffix in ["db-wal", "db-shm"] {
        remove(&database.with_extension(suffix))?;
    }
    fs::rename(&back, database)?;
    sync_dir(data)?;
    remove(&versions.join(trying.version.to_string()))?;
    write_synced(&versions.join(FAILED), trying.version.to_string().as_bytes())?;
    fs::remove_file(versions.join(TRYING))?;
    sync_dir(&versions)?;
    Ok(())
}

/// The image's server hands over to the newest version downloaded, should it be newer, for whatever
/// command it was given; otherwise returns, the versions there of no more use removed when serving.
#[cfg(unix)]
pub fn hand_over(data: &Path, own: Version, serving: bool) -> anyhow::Result<()> {
    let versions = data.join(VERSIONS);
    match downloaded(&versions)?.into_iter().max() {
        Some(newest) if newest > own => Err(run(data, newest).context(format!("删掉 {} 后重启，可以回到镜像里的版本", versions.display()))),
        _ if serving => Ok(tidy(&versions, None)?),
        _ => Ok(()),
    }
}

/// Once a version from `data/versions/` listens, before it serves: the switch to it is done, and the
/// other versions go. Synced first: the note back after a power cut would undo what it serves.
pub fn confirm(data: &Path, own: Version) -> io::Result<()> {
    let versions = data.join(VERSIONS);
    remove(&versions.join(TRYING))?;
    sync_dir(&versions)?;
    tidy(&versions, Some(own))
}

/// Runs `to`, downloaded, in this process's place, once serving has stopped. Given the database's
/// connection `c`, held until the process ends, so that nothing else writes after the copy taken
/// first, to go back to should `to` not start: requests still under way, sync connections and jobs
/// wait for it. Each step is synced before the next. Returns only when it could not, for the process
/// to end, the connection still held.
#[cfg(unix)]
pub fn switch(c: &Connection, data: &Path, to: Version) -> anyhow::Error {
    let noted = (|| -> anyhow::Result<()> {
        let snapshots = data.join("snapshots");
        fs::create_dir_all(&snapshots)?;
        let before = snapshots.join(format!("before-{to}.db"));
        db::copy(c, &before)?;
        File::open(&before)?.sync_all()?;
        // Only the last upgrade's is kept; files put there by hand stay.
        for entry in entries(&snapshots)? {
            let entry = entry?;
            let name = entry.file_name();
            let earlier = name.to_str().and_then(|n| n.strip_prefix("before-")?.strip_suffix(".db")).is_some_and(|v| v.parse::<Version>().is_ok());
            if earlier && entry.path() != before {
                fs::remove_file(entry.path())?;
            }
        }
        sync_dir(&snapshots)?;
        // Made by the first upgrade, both: their entries in `data` too.
        sync_dir(data)?;
        let versions = data.join(VERSIONS);
        write_synced(&versions.join(TRYING), &serde_json::to_vec(&Trying { version: to, before })?)?;
        // Noted: now one the image's server hands over to, as it can go back.
        fs::rename(versions.join(format!("{to}.part")), versions.join(to.to_string()))?;
        sync_dir(&versions)?;
        Ok(())
    })();
    let e = match noted {
        Ok(()) => run(data, to),
        Err(e) => e,
    };
    // For the settings to tell after the restart, also when it stopped before the note, with nothing
    // to go back from; on a full disk it may not get written either.
    match write_synced(&data.join(VERSIONS).join(FAILED), to.to_string().as_bytes()) {
        Ok(()) => e,
        Err(f) => e.context(format!("无法记下升级失败：{f}")),
    }
}

/// Writes `bytes` to `path` whole or not at all, synced, the directory too.
fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let part = path.with_extension("part");
    let mut file = File::create(&part)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&part, path)?;
    sync_dir(path.parent().expect("a file in a directory"))
}

/// Syncs the directory `dir`'s entries: that files in it were made, renamed or removed.
fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// Syncs every file and directory under `dir`, and `dir`.
fn sync_tree(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            sync_tree(&entry.path())?;
        } else {
            File::open(entry.path())?.sync_all()?;
        }
    }
    sync_dir(dir)
}

/// Runs the server of `version` in `data/versions/` in this process's place, with the arguments this
/// one was given. Returns only when it could not.
#[cfg(unix)]
fn run(data: &Path, version: Version) -> anyhow::Error {
    let server = data.join(VERSIONS).join(version.to_string()).join("nexttodo-server");
    let e = Command::new(&server).args(std::env::args_os().skip(1)).exec();
    anyhow::Error::new(e).context(format!("无法运行 {}", server.display()))
}

/// Only the administrator, and from a device signed in: it is the server's, not the user's data.
fn admin(auth: &Auth) -> Result<(), ApiError> {
    auth.require_device()?;
    if auth.is_admin { Ok(()) } else { Err(ApiError::forbidden("仅管理员可以升级服务器")) }
}

pub async fn status(State(st): State<AppState>, auth: Auth) -> Result<Json<Status>, ApiError> {
    admin(&auth)?;
    Ok(Json(st.upgrade.as_ref().map_or(Status::Unsupported, |u| u.status().clone())))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct To {
    version: Version,
}

pub async fn start(State(st): State<AppState>, auth: Auth, Json(to): Json<To>) -> Result<Json<Status>, ApiError> {
    admin(&auth)?;
    let Some(upgrade) = &st.upgrade else { return Ok(Json(Status::Unsupported)) };
    if to.version <= upgrade.own {
        return Err(ApiError::invalid(format!("服务器已经是 {} 了", upgrade.own), Some("version".into())));
    }
    Ok(Json(upgrade.begin(to.version)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_have_one_name() {
        assert!("0.1.170".parse::<Version>().unwrap() > "0.1.99".parse().unwrap());
        for bad in ["0.1", "0.1.2.3", "0.01.2", "0.1.-2", "0.1.+2", "0.1.x", "0.1.2/..", ""] {
            assert!(bad.parse::<Version>().is_err(), "{bad}");
        }
    }
}
