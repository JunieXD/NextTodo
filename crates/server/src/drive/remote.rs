//! The drive as a store of files by path, each written only where none is yet
//! (docs/local/drive.md#只在不存在时写入). Calls block; the sync task makes them on blocking threads.

/// How writing a file where none may be went.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Created {
    Written,
    /// One was there already, and is left as it is.
    Exists,
}

/// A request that did not go through.
#[derive(Debug)]
pub(crate) enum Failed {
    /// It never reached the drive: nothing was done.
    Unsent(String),
    /// It was sent, and may or may not have been done.
    Unknown(String),
}

impl Failed {
    pub(crate) fn message(&self) -> &str {
        match self {
            Failed::Unsent(m) | Failed::Unknown(m) => m,
        }
    }
}

pub(crate) trait Remote: Send + Sync {
    /// The file at `path`, `None` when there is none.
    fn get(&self, path: &str) -> Result<Option<Vec<u8>>, Failed>;
    /// Writes `body` at `path` unless a file is there.
    fn create(&self, path: &str, body: &[u8]) -> Result<Created, Failed>;
    /// Deletes the file or folder at `path`; one already gone is no error.
    fn delete(&self, path: &str) -> Result<(), Failed>;
    /// The names of the files and folders in folder `dir`.
    fn list(&self, dir: &str) -> Result<Vec<String>, Failed>;
    /// Makes folder `dir`, whose parent is there; one there already is no error. WebDAV writes into
    /// folders that are there only.
    fn mkdir(&self, dir: &str) -> Result<(), Failed>;
}

/// Deletes `path` with everything under it: on an object store a folder is only what the names of
/// its files begin with, and goes once they do.
pub(crate) fn remove(r: &dyn Remote, path: &str) -> Result<(), Failed> {
    for name in r.list(path)? {
        remove(r, &format!("{path}/{name}"))?;
    }
    r.delete(path)
}

/// A drive in memory, for tests: one store several devices share, each through a `View` that can be
/// taken offline, told to lose its next writes, or hold one until the test lets it go on.
#[cfg(test)]
pub(crate) mod memory {
    use std::collections::btree_map::Entry;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, mpsc};

    use super::{Created, Failed, Remote};

    #[derive(Default)]
    pub(crate) struct Store(Mutex<BTreeMap<String, Vec<u8>>>);

    impl Store {
        pub(crate) fn paths(&self) -> Vec<String> {
            self.0.lock().unwrap().keys().cloned().collect()
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub(crate) enum Fault {
        /// The write is done, but its answer is lost.
        AnswerLost,
        /// The write is lost on the way: nothing is done, and no answer comes.
        RequestLost,
    }

    pub(crate) struct View {
        store: Arc<Store>,
        writes: Mutex<VecDeque<Fault>>,
        offline: AtomicBool,
        /// Requests turned away while offline.
        refused: AtomicUsize,
        gate: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
    }

    /// A write held at the gate: `arrived` once it is there, and it goes on once `open` is sent to.
    pub(crate) struct Gate {
        pub arrived: mpsc::Receiver<()>,
        pub open: mpsc::Sender<()>,
    }

    impl View {
        pub(crate) fn new(store: Arc<Store>) -> View {
            View { store, writes: Mutex::default(), offline: AtomicBool::new(false), refused: AtomicUsize::new(0), gate: Mutex::default() }
        }

        /// The next writes go so.
        pub(crate) fn fail_writes(&self, faults: impl IntoIterator<Item = Fault>) {
            self.writes.lock().unwrap().extend(faults);
        }

        /// Turns every request away, or no longer.
        pub(crate) fn set_offline(&self, offline: bool) {
            self.offline.store(offline, Ordering::SeqCst);
        }

        pub(crate) fn refused(&self) -> usize {
            self.refused.load(Ordering::SeqCst)
        }

        /// Holds the next write until the gate is opened.
        pub(crate) fn gate_next_write(&self) -> Gate {
            let (arrived_tx, arrived) = mpsc::channel();
            let (open, open_rx) = mpsc::channel();
            *self.gate.lock().unwrap() = Some((arrived_tx, open_rx));
            Gate { arrived, open }
        }

        /// Runs request `f`: as `fault` says, or turned away while offline.
        fn request<T>(&self, fault: Option<Fault>, f: impl FnOnce(&mut BTreeMap<String, Vec<u8>>) -> T) -> Result<T, Failed> {
            let mut files = self.store.0.lock().unwrap();
            match fault {
                Some(Fault::RequestLost) => Err(Failed::Unknown("no answer".into())),
                Some(Fault::AnswerLost) => {
                    f(&mut files);
                    Err(Failed::Unknown("no answer".into()))
                }
                None if self.offline.load(Ordering::SeqCst) => {
                    self.refused.fetch_add(1, Ordering::SeqCst);
                    Err(Failed::Unsent("offline".into()))
                }
                None => Ok(f(&mut files)),
            }
        }
    }

    impl Remote for View {
        fn get(&self, path: &str) -> Result<Option<Vec<u8>>, Failed> {
            self.request(None, |files| files.get(path).cloned())
        }

        fn create(&self, path: &str, body: &[u8]) -> Result<Created, Failed> {
            let gate = self.gate.lock().unwrap().take();
            if let Some((arrived, open)) = gate {
                arrived.send(()).unwrap();
                open.recv().unwrap();
            }
            let fault = self.writes.lock().unwrap().pop_front();
            self.request(fault, |files| match files.entry(path.to_owned()) {
                Entry::Occupied(_) => Created::Exists,
                Entry::Vacant(v) => {
                    v.insert(body.to_vec());
                    Created::Written
                }
            })
        }

        fn delete(&self, path: &str) -> Result<(), Failed> {
            self.request(None, |files| files.retain(|p, _| p != path && !p.starts_with(&format!("{path}/"))))
        }

        fn mkdir(&self, _: &str) -> Result<(), Failed> {
            self.request(None, |_| ())
        }

        fn list(&self, dir: &str) -> Result<Vec<String>, Failed> {
            let prefix = super::super::http::folder(dir);
            self.request(None, |files| {
                let mut names: Vec<String> = files
                    .keys()
                    .filter_map(|p| p.strip_prefix(&prefix))
                    .map(|rest| rest.split('/').next().unwrap_or(rest).to_owned())
                    .collect();
                names.dedup();
                names
            })
        }
    }
}
