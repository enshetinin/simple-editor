//! Serializes recovery snapshots on one background thread.

use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use ropey::Rope;

use crate::http::Notify;
use crate::storage::recovery::{OrphanSession, RecoveryMeta, RecoverySession};

enum Task {
    Write {
        key: u64,
        meta: RecoveryMeta,
        rope: Rope,
    },
    Remove {
        key: u64,
    },
    Finish,
}

pub enum RecoveryEvent {
    Orphans(Vec<OrphanSession>),
    Failed(String),
}

pub struct RecoveryWorker {
    tx: Sender<Task>,
    rx: Receiver<RecoveryEvent>,
    thread: Option<JoinHandle<()>>,
}

impl RecoveryWorker {
    /// Starts a session under `root` and looks for previous sessions to
    /// recover, all off the UI thread.
    pub fn start(root: &Path, notify: Notify) -> Self {
        let (tx, task_rx) = channel::<Task>();
        let (event_tx, rx) = channel();
        let root = root.to_path_buf();
        let thread = std::thread::Builder::new()
            .name("se-recovery".to_owned())
            .spawn(move || {
                let session = match RecoverySession::start(&root) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = event_tx.send(RecoveryEvent::Failed(format!(
                            "Crash recovery is unavailable ({}): {e}",
                            root.display()
                        )));
                        notify();
                        return;
                    }
                };
                let orphans = session.find_orphans();
                if !orphans.is_empty() {
                    let _ = event_tx.send(RecoveryEvent::Orphans(orphans));
                    notify();
                }
                let mut reported = false;
                for task in task_rx {
                    let result = match task {
                        Task::Write { key, meta, rope } => session.write(key, &meta, &rope),
                        Task::Remove { key } => session.remove(key),
                        Task::Finish => {
                            session.finish();
                            return;
                        }
                    };
                    if let Err(e) = result
                        && !reported
                    {
                        reported = true;
                        let _ = event_tx.send(RecoveryEvent::Failed(format!(
                            "Could not write the crash-recovery copy: {e}"
                        )));
                        notify();
                    }
                }
            })
            .ok();
        Self { tx, rx, thread }
    }

    pub fn write(&self, key: u64, meta: RecoveryMeta, rope: Rope) {
        let _ = self.tx.send(Task::Write { key, meta, rope });
    }

    pub fn remove(&self, key: u64) {
        let _ = self.tx.send(Task::Remove { key });
    }

    pub fn poll(&self) -> Vec<RecoveryEvent> {
        self.rx.try_iter().collect()
    }

    /// Clean shutdown: deletes this session's snapshots and waits briefly.
    pub fn finish(mut self) {
        let _ = self.tx.send(Task::Finish);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
