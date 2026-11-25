//! Attachments' files (docs/design.md#附件), on disk in the data directory's `files/`, each named by
//! its attachment's id. Nothing else reads, writes or deletes them: keeping them elsewhere, such as in
//! S3, changes only this module. Syncing through a drive, a file not here yet is fetched from it as it
//! is read (docs/local.md#附件).

use std::collections::HashSet;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, SystemTime};

use axum::body::Body;
use http_body_util::BodyExt;
use nexttodo_core::model::{AttachmentId, FileSize, MAX_FILE_SIZE};
use tokio::io::AsyncWriteExt;
use tower_http::services::ServeFile;
use uuid::Uuid;

/// Where the files are, beside the database.
pub const DIR: &str = "files";

/// What files being written are called until they are published.
const PART: &str = ".part-";

pub struct Files {
    dir: PathBuf,
    /// Where the files not here are, while syncing through a drive.
    origin: RwLock<Option<Arc<dyn Origin>>>,
}

/// Where files not here yet are kept.
pub trait Origin: Send + Sync {
    /// `id`'s file, none when there is none.
    fn fetch(&self, id: AttachmentId) -> io::Result<Option<Vec<u8>>>;
}

/// A file written in full, flushed to disk, and not yet anyone's: published under an id, or deleted
/// when dropped.
pub struct Part {
    path: PathBuf,
    pub size: u32,
}

impl Part {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Part {
    fn drop(&mut self) {
        // Gone once published; one that can't be deleted now is left to the sweep.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Why a file was not received.
#[derive(Debug)]
pub enum ReceiveError {
    /// More than `MAX_FILE_SIZE` bytes.
    TooLarge,
    /// The body broke off: the client went away, say.
    Body(axum::Error),
    Io(io::Error),
}

impl From<io::Error> for ReceiveError {
    fn from(e: io::Error) -> Self {
        ReceiveError::Io(e)
    }
}

impl Files {
    pub fn new(dir: PathBuf) -> Self {
        Files { dir, origin: RwLock::default() }
    }

    /// Fetches the files not here from `origin` from now on, or, with none, no longer.
    pub fn set_origin(&self, origin: Option<Arc<dyn Origin>>) {
        *self.origin.write().unwrap_or_else(PoisonError::into_inner) = origin;
    }

    fn path(&self, id: AttachmentId) -> PathBuf {
        self.dir.join(id.to_string())
    }

    /// A new file of its own, beside the others, to be moved in place once complete: a file is
    /// there in full or not at all.
    fn part(&self) -> io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        Ok(self.dir.join(format!("{PART}{}", Uuid::now_v7())))
    }

    /// A place for a file of the server's own for a while, such as an export being packed: deleted
    /// when dropped, or by the sweep when the server stopped first.
    pub fn scratch(&self) -> io::Result<Part> {
        Ok(Part { path: self.part()?, size: 0 })
    }

    /// Writes `body` to a part as it comes, at most `MAX_FILE_SIZE` bytes.
    pub async fn receive(&self, mut body: Body) -> Result<Part, ReceiveError> {
        let mut part = Part { path: self.part()?, size: 0 };
        let mut file = tokio::fs::File::create(&part.path).await?;
        let mut size = 0usize;
        while let Some(frame) = body.frame().await {
            let Ok(data) = frame.map_err(ReceiveError::Body)?.into_data() else { continue };
            size += data.len();
            if size > MAX_FILE_SIZE as usize {
                return Err(ReceiveError::TooLarge);
            }
            file.write_all(&data).await?;
        }
        file.sync_all().await?;
        part.size = u32::try_from(size).expect("at most MAX_FILE_SIZE");
        Ok(part)
    }

    /// Makes `part` `id`'s file, in place of any there, which no one has: whoever publishes checks
    /// that first, holding the database.
    pub fn publish(&self, part: Part, id: AttachmentId) -> io::Result<()> {
        std::fs::rename(&part.path, self.path(id))?;
        // The rename lasts through a power cut once the directory is flushed too. Windows flushes
        // no directory without unsafe calls; NTFS's journal has to do.
        #[cfg(unix)]
        std::fs::File::open(&self.dir)?.sync_all()?;
        Ok(())
    }

    /// Writes `bytes` as `id`'s file, as `publish` does.
    pub fn write(&self, id: AttachmentId, bytes: &[u8]) -> io::Result<()> {
        let part = Part { path: self.part()?, size: u32::try_from(bytes.len()).expect("at most MAX_FILE_SIZE") };
        let mut file = std::fs::File::create(&part.path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        self.publish(part, id)
    }

    /// A copy of `from`'s file, of `size` bytes, fetched first when it is not here: to be published as
    /// another id's.
    pub fn copy(&self, from: AttachmentId, size: FileSize) -> io::Result<Part> {
        self.fetch(from, size)?;
        let part = Part { path: self.part()?, size: size.get() };
        std::fs::copy(self.path(from), &part.path)?;
        std::fs::File::open(&part.path)?.sync_all()?;
        Ok(part)
    }

    /// The whole of `id`'s file, of `size` bytes.
    pub fn read(&self, id: AttachmentId, size: FileSize) -> io::Result<Vec<u8>> {
        self.fetch(id, size)?;
        std::fs::read(self.path(id))
    }

    /// `id`'s file when it is here, without fetching it.
    pub fn here(&self, id: AttachmentId) -> io::Result<Option<Vec<u8>>> {
        match std::fs::read(self.path(id)) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            read => read.map(Some),
        }
    }

    /// Brings `id`'s file, of `size` bytes, here from the origin, when it is not here and is there.
    /// Blocks meanwhile.
    pub fn fetch(&self, id: AttachmentId, size: FileSize) -> io::Result<()> {
        let origin = self.origin.read().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(origin) = origin else { return Ok(()) };
        if self.path(id).exists() {
            return Ok(());
        }
        match origin.fetch(id)? {
            // Cut short or replaced there: not kept, so that it is fetched again.
            Some(bytes) if bytes.len() != size.get() as usize => Err(io::Error::other("网盘里附件的文件和记录的大小不一样")),
            Some(bytes) => self.write(id, &bytes),
            None => Ok(()),
        }
    }

    /// Serves `id`'s file, answering ranges and conditional requests; `fetch` it first.
    pub fn serve(&self, id: AttachmentId) -> ServeFile {
        ServeFile::new(self.path(id))
    }

    /// Deletes `id`'s file; one already gone is fine.
    pub fn remove(&self, id: AttachmentId) -> io::Result<()> {
        match std::fs::remove_file(self.path(id)) {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Deletes what was left behind more than `age` ago: parts, of uploads cut short, and files
    /// `known` does not have, published by a server that stopped before recording them. The caller
    /// holds the database, where files are recorded, so `known` stays true while this runs.
    pub fn sweep(&self, known: &HashSet<AttachmentId>, age: Duration) -> io::Result<usize> {
        let before = SystemTime::now() - age;
        let mut swept = 0;
        for entry in read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let stray = match name.parse::<Uuid>().ok().and_then(|u| AttachmentId::try_from(u).ok()) {
                Some(id) => !known.contains(&id),
                None => name.starts_with(PART),
            };
            if stray && entry.metadata()?.modified()? < before {
                std::fs::remove_file(entry.path())?;
                swept += 1;
            }
        }
        Ok(swept)
    }
}

/// The entries of `dir`, none when there is no such directory yet: nothing has been uploaded.
fn read_dir(dir: &Path) -> io::Result<Box<dyn Iterator<Item = io::Result<std::fs::DirEntry>>>> {
    match std::fs::read_dir(dir) {
        Ok(entries) => Ok(Box::new(entries)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Box::new(std::iter::empty())),
        Err(e) => Err(e),
    }
}
