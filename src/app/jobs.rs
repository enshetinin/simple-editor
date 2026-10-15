//! Background jobs for the UI: saves, reloads, disk checks, searches and
//! recovery restores. Each runs on its own short-lived thread and reports
//! back through one channel; the UI thread never waits on disk I/O.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use ropey::Rope;

use super::{AfterSave, TabKey};
use crate::editor::document::load;
use crate::editor::save::DiskStamp;
use crate::editor::search::{SearchQuery, SearchResult, find_all};
use crate::editor::{LoadedText, OpenError, SaveError, SaveJob};
use crate::http::Notify;
use crate::storage::recovery::RecoveredEntry;

pub enum JobResult {
    Saved {
        tab: TabKey,
        job: SaveJob,
        result: Result<DiskStamp, SaveError>,
        then: AfterSave,
    },
    Reloaded {
        tab: TabKey,
        result: Result<LoadedText, OpenError>,
    },
    DiskChecked {
        tab: TabKey,
        stamp: Option<DiskStamp>,
    },
    Searched {
        tab: TabKey,
        generation: u64,
        revision: u64,
        query: SearchQuery,
        result: SearchResult,
    },
    Restored {
        entry: RecoveredEntry,
        loaded: Option<LoadedText>,
        text: std::io::Result<String>,
    },
}

pub struct Jobs {
    tx: Sender<JobResult>,
    rx: Receiver<JobResult>,
    notify: Notify,
}

impl Jobs {
    pub fn new(notify: Notify) -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, notify }
    }

    fn spawn(&self, name: &str, work: impl FnOnce() -> JobResult + Send + 'static) {
        let tx = self.tx.clone();
        let notify = self.notify.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("se-{name}"))
            .spawn(move || {
                let _ = tx.send(work());
                notify();
            });
        if let Err(e) = spawned {
            // Extremely unlikely; surface it instead of silently dropping work.
            eprintln!("could not start a background job: {e}");
        }
    }

    pub fn save(&self, tab: TabKey, job: SaveJob, then: AfterSave) {
        self.spawn("save", move || {
            let result = job.run();
            JobResult::Saved {
                tab,
                job,
                result,
                then,
            }
        });
    }

    pub fn reload(&self, tab: TabKey, path: PathBuf) {
        self.spawn("reload", move || JobResult::Reloaded {
            tab,
            result: load(&path),
        });
    }

    pub fn check_disk(&self, tabs: Vec<(TabKey, PathBuf)>) {
        if tabs.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        let notify = self.notify.clone();
        let _ = std::thread::Builder::new()
            .name("se-stat".to_owned())
            .spawn(move || {
                for (tab, path) in tabs {
                    let stamp = DiskStamp::of(&path).ok();
                    let _ = tx.send(JobResult::DiskChecked { tab, stamp });
                }
                notify();
            });
    }

    pub fn search(
        &self,
        tab: TabKey,
        generation: u64,
        revision: u64,
        rope: Rope,
        query: SearchQuery,
    ) {
        self.spawn("search", move || {
            let result = find_all(&rope, &query);
            JobResult::Searched {
                tab,
                generation,
                revision,
                query,
                result,
            }
        });
    }

    pub fn restore(&self, entry: RecoveredEntry) {
        self.spawn("restore", move || {
            let loaded = entry
                .meta
                .original_path
                .as_deref()
                .filter(|p| p.exists())
                .and_then(|p| load(p).ok());
            let text = entry.read_text();
            JobResult::Restored {
                entry,
                loaded,
                text,
            }
        });
    }

    pub fn poll(&self) -> Vec<JobResult> {
        self.rx.try_iter().collect()
    }
}
