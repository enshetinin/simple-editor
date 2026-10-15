//! Project folder: directory listings loaded on demand and file opening, both
//! performed on background threads so the UI never blocks on disk I/O.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::editor::{Document, OpenError};
use crate::http::Notify;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
}

/// Lists one directory (not recursive): folders first, then case-insensitive name.
pub fn list_dir(path: &Path) -> io::Result<Vec<DirEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let Ok(entry) = entry else { continue };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let entry_path = entry.path();
        let is_symlink = file_type.is_symlink();
        // Following a symlink here is a single stat; directories are only
        // listed when the user expands them, so link cycles cannot recurse.
        let is_dir = if is_symlink {
            entry_path.is_dir()
        } else {
            file_type.is_dir()
        };
        entries.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: entry_path,
            is_dir,
            is_symlink,
        });
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(entries)
}

#[derive(Debug)]
pub enum DirState {
    Loading,
    Loaded(Vec<DirEntry>),
    Failed(String),
}

pub enum WorkerEvent {
    Listed(PathBuf, io::Result<Vec<DirEntry>>),
    Opened(PathBuf, Result<Document, OpenError>),
}

/// Background I/O for the project: listings and file reads.
pub struct Workspace {
    pub root: Option<PathBuf>,
    pub dirs: HashMap<PathBuf, DirState>,
    pub expanded: HashSet<PathBuf>,
    pub opening: HashSet<PathBuf>,
    tx: Sender<WorkerEvent>,
    rx: Receiver<WorkerEvent>,
    notify: Notify,
}

impl Workspace {
    pub fn new(notify: Notify) -> Self {
        let (tx, rx) = channel();
        Self {
            root: None,
            dirs: HashMap::new(),
            expanded: HashSet::new(),
            opening: HashSet::new(),
            tx,
            rx,
            notify,
        }
    }

    pub fn set_root(&mut self, root: PathBuf) {
        self.dirs.clear();
        self.expanded.clear();
        self.expanded.insert(root.clone());
        self.request_listing(root.clone());
        self.root = Some(root);
    }

    pub fn project_name(&self) -> Option<String> {
        let root = self.root.as_deref()?;
        Some(
            root.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.display().to_string()),
        )
    }

    pub fn toggle(&mut self, dir: &Path) {
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_path_buf());
            if !matches!(self.dirs.get(dir), Some(DirState::Loaded(_))) {
                self.request_listing(dir.to_path_buf());
            }
        }
    }

    fn request_listing(&mut self, dir: PathBuf) {
        self.dirs.insert(dir.clone(), DirState::Loading);
        let tx = self.tx.clone();
        let notify = self.notify.clone();
        std::thread::spawn(move || {
            let result = list_dir(&dir);
            let _ = tx.send(WorkerEvent::Listed(dir, result));
            notify();
        });
    }

    /// Reads and decodes a file in the background. Returns false if it is
    /// already being opened.
    pub fn request_open(&mut self, path: PathBuf) -> bool {
        if !self.opening.insert(path.clone()) {
            return false;
        }
        let tx = self.tx.clone();
        let notify = self.notify.clone();
        std::thread::spawn(move || {
            let result = if path.exists() {
                Document::open(&path)
            } else {
                Ok(Document::new_at(path.clone()))
            };
            let _ = tx.send(WorkerEvent::Opened(path, result));
            notify();
        });
        true
    }

    /// Applies finished listings and returns opened files for the caller.
    pub fn poll(&mut self) -> Vec<(PathBuf, Result<Document, OpenError>)> {
        let mut opened = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            match event {
                WorkerEvent::Listed(dir, result) => {
                    let state = match result {
                        Ok(entries) => DirState::Loaded(entries),
                        Err(e) => DirState::Failed(e.to_string()),
                    };
                    self.dirs.insert(dir, state);
                }
                WorkerEvent::Opened(path, result) => {
                    self.opening.remove(&path);
                    opened.push((path, result));
                }
            }
        }
        opened
    }
}
