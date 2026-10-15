//! Background jobs for the UI: saves, reloads, disk checks, searches and
//! recovery restores. Each runs on its own short-lived thread and reports
//! back through one channel; the UI thread never waits on disk I/O.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use ropey::Rope;

use std::sync::{Arc, Mutex};

use super::{AfterSave, TabKey};
use crate::editor::document::load;
use crate::editor::save::DiskStamp;
use crate::editor::search::{SearchQuery, SearchResult, find_all};
use crate::editor::{LoadedText, OpenError, SaveError, SaveJob};
use crate::http::Notify;
use crate::http::model::Variable;
use crate::http::secrets::{SecretBackend, secret_key};
use crate::storage::history::{self, HistoryEntry, UserState};
use crate::storage::project::{self, Collection, Environment, ProjectData};
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
    ProjectLoaded {
        root: PathBuf,
        data: ProjectData,
        /// Remembered secrets that could not be read from the OS store.
        secret_errors: Vec<String>,
    },
    UserDataLoaded {
        history: Result<Vec<HistoryEntry>, String>,
        state: UserState,
    },
    CollectionSaved {
        tab: Option<TabKey>,
        name: String,
        result: Result<Vec<String>, String>,
        then: AfterSave,
    },
    EnvironmentsSaved {
        tab: TabKey,
        result: Result<(), String>,
        /// Secrets that could not be stored/removed in the OS store.
        secret_errors: Vec<(String, String)>,
        then: AfterSave,
    },
    CollectionImported(Result<Collection, String>),
    Done {
        /// Message for the user (info or error).
        message: String,
        is_error: bool,
    },
}

/// Secret-store operations requested when saving environments.
pub enum SecretOp {
    Set {
        key: String,
        label: String,
        value: String,
    },
    Delete {
        key: String,
    },
}

pub struct Jobs {
    tx: Sender<JobResult>,
    rx: Receiver<JobResult>,
    notify: Notify,
    /// Serializes writes of user-data files; holds the last written
    /// generation so an older snapshot never overwrites a newer one.
    user_writes: Arc<Mutex<u64>>,
    next_generation: std::cell::Cell<u64>,
}

impl Jobs {
    pub fn new(notify: Notify) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            notify,
            user_writes: Arc::new(Mutex::new(0)),
            next_generation: std::cell::Cell::new(1),
        }
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

    /// Loads `.simpleeditor/` and any remembered secrets of the project.
    pub fn load_project(&self, root: PathBuf, secrets: Arc<dyn SecretBackend>) {
        self.spawn("project", move || {
            let mut data = project::load(&root);
            let mut secret_errors = Vec::new();
            let mut fill = |scope: &str, vars: &mut Vec<Variable>| {
                for v in vars.iter_mut().filter(|v| v.secret && v.remember) {
                    match secrets.get(&secret_key(&root, scope, &v.name)) {
                        Ok(Some(value)) => v.value = value,
                        Ok(None) => {}
                        Err(e) => secret_errors.push(format!("{{{{{}}}}}: {e}", v.name)),
                    }
                }
            };
            fill("project", &mut data.variables);
            for env in &mut data.environments {
                let scope = format!("env:{}", env.id);
                fill(&scope, &mut env.variables);
            }
            JobResult::ProjectLoaded {
                root,
                data,
                secret_errors,
            }
        });
    }

    pub fn load_user_data(&self, dir: PathBuf) {
        self.spawn("user-data", move || JobResult::UserDataLoaded {
            history: history::load_history(&history::history_path(&dir)),
            state: history::load_state(&history::state_path(&dir)),
        });
    }

    fn next_gen(&self) -> u64 {
        let g = self.next_generation.get();
        self.next_generation.set(g + 1);
        g
    }

    /// Writes history and user state snapshots, newest wins.
    pub fn save_user_data(&self, dir: PathBuf, entries: Vec<HistoryEntry>, state: UserState) {
        let generation = self.next_gen();
        let lock = self.user_writes.clone();
        self.spawn("user-data-save", move || {
            let Ok(mut last) = lock.lock() else {
                return JobResult::Done {
                    message: "history was not saved (internal lock error)".to_owned(),
                    is_error: true,
                };
            };
            if *last > generation {
                return JobResult::Done {
                    message: String::new(),
                    is_error: false,
                };
            }
            *last = generation;
            let result = history::save_history(&history::history_path(&dir), &entries)
                .and_then(|()| history::save_state(&history::state_path(&dir), &state));
            JobResult::Done {
                message: match &result {
                    Ok(()) => String::new(),
                    Err(e) => format!("Could not save request history: {e}"),
                },
                is_error: result.is_err(),
            }
        });
    }

    /// Same as [`Self::save_user_data`] but synchronous (app exit).
    pub fn write_user_data_blocking(
        &self,
        dir: PathBuf,
        entries: Vec<HistoryEntry>,
        state: UserState,
    ) -> std::io::Result<()> {
        let generation = self.next_gen();
        let mut last = self
            .user_writes
            .lock()
            .map_err(|_| std::io::Error::other("user data lock poisoned"))?;
        *last = (*last).max(generation);
        history::save_history(&history::history_path(&dir), &entries)?;
        history::save_state(&history::state_path(&dir), &state)
    }

    pub fn save_collection(
        &self,
        root: PathBuf,
        collection: Collection,
        tab: Option<TabKey>,
        then: AfterSave,
    ) {
        self.spawn("collection", move || JobResult::CollectionSaved {
            tab,
            name: collection.name.clone(),
            result: project::save_collection(&root, &collection).map_err(|e| e.to_string()),
            then,
        });
    }

    pub fn delete_collection(&self, root: PathBuf, id: String, name: String) {
        self.spawn("collection-delete", move || {
            let result = project::delete_collection(&root, &id);
            let is_error = result.is_err();
            JobResult::Done {
                is_error,
                message: match result {
                    Ok(()) => format!("Collection \"{name}\" deleted."),
                    Err(e) => format!("Could not delete collection \"{name}\": {e}"),
                },
            }
        });
    }

    #[expect(clippy::too_many_arguments)]
    pub fn save_environments(
        &self,
        tab: TabKey,
        root: PathBuf,
        variables: Vec<Variable>,
        environments: Vec<Environment>,
        deleted: Vec<String>,
        ops: Vec<SecretOp>,
        secrets: Arc<dyn SecretBackend>,
        then: AfterSave,
    ) {
        self.spawn("environments", move || {
            let mut secret_errors = Vec::new();
            for op in ops {
                match op {
                    SecretOp::Set { key, label, value } => {
                        if let Err(e) = secrets.set(&key, &value) {
                            secret_errors.push((label, e));
                        }
                    }
                    SecretOp::Delete { key } => {
                        let _ = secrets.delete(&key);
                    }
                }
            }
            let result = (|| -> std::io::Result<()> {
                project::save_variables(&root, &variables)?;
                for env in &environments {
                    project::save_environment(&root, env)?;
                }
                for id in &deleted {
                    match project::delete_environment(&root, id) {
                        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                        _ => {}
                    }
                }
                Ok(())
            })()
            .map_err(|e| e.to_string());
            JobResult::EnvironmentsSaved {
                tab,
                result,
                secret_errors,
                then,
            }
        });
    }

    pub fn import_collection(&self, root: PathBuf, source: PathBuf, names: Vec<String>) {
        self.spawn("import", move || {
            JobResult::CollectionImported(project::import_collection(&root, &source, &names))
        });
    }

    pub fn export_collection(&self, collection: Collection, path: PathBuf, include_secrets: bool) {
        self.spawn("export", move || {
            let result = project::export_collection(&collection, &path, include_secrets);
            let is_error = result.is_err();
            JobResult::Done {
                is_error,
                message: match result {
                    Ok(()) if include_secrets => format!(
                        "Exported \"{}\" to {} including secrets.",
                        collection.name,
                        path.display()
                    ),
                    Ok(()) => format!(
                        "Exported \"{}\" to {} (secrets left out).",
                        collection.name,
                        path.display()
                    ),
                    Err(e) => format!("Could not export \"{}\": {e}", collection.name),
                },
            }
        });
    }

    pub fn poll(&self) -> Vec<JobResult> {
        self.rx.try_iter().collect()
    }
}
