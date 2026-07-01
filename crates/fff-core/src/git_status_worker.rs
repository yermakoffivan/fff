use crate::shared::{SharedFrecency, WeakFilePicker};
use ahash::AHashSet;
use parking_lot::{Condvar, Mutex};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use tracing::{debug, error, info};

#[derive(Default)]
struct Pending {
    paths: AHashSet<PathBuf>,
    full_rescan: bool,
    shutdown: bool,
}

impl Pending {
    fn has_work(&self) -> bool {
        self.full_rescan || !self.paths.is_empty()
    }
}

/// Single-consumer coalescing queue for git-status updates. Producers (the
/// debouncer + owner threads) push cheaply and return; a dedicated consumer
/// drains the whole accumulated batch at once. A single ordered consumer means
/// a stale pre-commit snapshot can never be applied after a fresh post-commit
/// one — the race the watcher fuzz test caught.
pub(crate) struct GitStatusMailbox {
    state: Mutex<Pending>,
    cv: Condvar,
}

impl GitStatusMailbox {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(Pending::default()),
            cv: Condvar::new(),
        })
    }

    /// Accumulate paths that need a targeted git-status refresh. Cheap: a lock,
    /// a set-extend, and a single wakeup. Never blocks on git.
    pub(crate) fn enqueue_paths<I>(&self, paths: I)
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let mut guard = self.state.lock();
        guard.paths.extend(paths);
        drop(guard);
        self.cv.notify_one();
    }

    /// Request a full git rescan. Supersedes any queued per-path work — a full
    /// rescan re-reads every tracked path, so the drained paths are irrelevant.
    pub(crate) fn request_full_rescan(&self) {
        let mut guard = self.state.lock();
        guard.full_rescan = true;
        drop(guard);
        self.cv.notify_one();
    }

    pub(crate) fn signal_shutdown(&self) {
        let mut guard = self.state.lock();
        guard.shutdown = true;
        drop(guard);
        self.cv.notify_one();
    }

    /// Block until there is work or shutdown, then drain the whole batch.
    /// Returns `None` once shutdown was requested (consumer should exit).
    fn wait_and_take(&self) -> Option<Pending> {
        let mut guard = self.state.lock();
        while !guard.shutdown && !guard.has_work() {
            self.cv.wait(&mut guard);
        }
        if guard.shutdown {
            return None;
        }
        Some(std::mem::take(&mut *guard))
    }
}

/// Spawn the dedicated git-status consumer. It sleeps on the condvar while
/// idle (no polling), wakes to drain the accumulated batch, and applies it via
/// the serialized `SharedFilePicker` methods. Detached on teardown via
/// `signal_shutdown`; it also exits if the picker has been dropped.
pub(crate) fn spawn_consumer(
    weak_picker: WeakFilePicker,
    frecency: SharedFrecency,
    mailbox: Arc<GitStatusMailbox>,
    span: tracing::Span,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("fff-git-status".into())
        .spawn(move || {
            let _g = span.enter();
            while let Some(work) = mailbox.wait_and_take() {
                // Upgrade only for one batch; if the picker is gone teardown is
                // underway, so exit cleanly.
                let Some(picker) = weak_picker.upgrade() else {
                    break;
                };

                if work.full_rescan {
                    debug!("git-status worker: full rescan");
                    if let Err(e) = picker.refresh_git_status(&frecency) {
                        error!("git-status worker: full rescan failed: {e:?}");
                    }
                } else if !work.paths.is_empty() {
                    let paths: Vec<PathBuf> = work.paths.into_iter().collect();
                    if let Err(e) = picker.update_git_status_for_paths(&paths, &frecency) {
                        error!("git-status worker: path update failed: {e:?}");
                    }
                }
            }

            info!("git-status worker stopped");
        })
        .expect("failed to spawn fff-git-status thread")
}
