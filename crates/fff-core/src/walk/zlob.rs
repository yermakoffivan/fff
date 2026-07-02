//! Filesystem traversal backed by zlob's native parallel walker.
//! Active when the `zlob` feature is enabled (requires the Zig toolchain).

use crate::file_picker::is_known_binary_extension_basename;
use crate::ignore::IGNORED_DIRS;
use crate::types::FileItem;
use crate::walk::{WalkIgnoreRules, WalkOutput};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use zlob::walk::{WalkBuilder, WalkFlags, WalkMetadata, WalkState};

/// Publish the running file count every `PROGRESS_STEP` files so the UI's
/// "Indexing files N" status animates live. Odd so the ticking looks less
/// mechanical; the modulo is trivial next to the walk's syscall cost.
const PROGRESS_STEP: usize = 13;

/// Walk `base_path` and collect every non-ignored file as a
/// `(FileItem, relative_path)` pair, plus the reusable ignore rules zlob
/// assembled during the walk. zlob honors nested `.gitignore`/`.ignore`
/// natively; for non-git roots we hand the build-artifact / platform-noise
/// list to the walker via `extra_ignore` so those subtrees are pruned
/// *before openat* rather than filtered post-emit.
#[tracing::instrument(skip_all, name = "zlob walker", level = "info")]
pub(crate) fn walk_collect_files(
    base_path: &Path,
    is_git_repo: bool,
    follow_symlinks: bool,
    threads: usize,
    synced_files_count: &Arc<AtomicUsize>,
) -> crate::Result<WalkOutput> {
    // gitignore on; skip hidden on non-git roots (so `~/` doesn't recurse into
    // ~/.cache, ~/.config, etc.); optionally follow symlinks.
    let mut flags = WalkFlags::GITIGNORE;
    if !is_git_repo {
        flags |= WalkFlags::SKIP_HIDDEN;
    }
    if follow_symlinks {
        flags |= WalkFlags::FOLLOW_SYMLINKS;
    }

    // Constructing the builder is fallible in this zlob version (interior
    // NUL in the path).
    let mut builder = WalkBuilder::new(base_path)
        .map_err(|e| crate::Error::WalkFailed(format!("WalkBuilder::new: {e:?}")))?;
    builder
        .options(flags)
        .threads(threads)
        // Bulk-fetch the only metadata FileItem needs; zlob never stats more.
        .metadata(WalkMetadata::SIZE | WalkMetadata::MTIME);

    // Non-git roots: push the build-artifact / platform-noise list down to
    // the walker so those subtrees are pruned *before openat* (skips ~500k
    // getdirents on a typical home dir). Git roots derive the same exclusions
    // from the project's own .gitignore, so we leave extra_ignore empty there.
    // The list must reach the walker as `extra_ignore` — filtering these
    // paths post-emit inside the visitor is what we're moving *away* from.
    if !is_git_repo
        && !IGNORED_DIRS.is_empty()
        && let Err(e) = builder.extra_ignore(IGNORED_DIRS)
    {
        // Interior NUL in one of the extra_ignore patterns would fail
        // here — treat as if no extras were supplied rather than
        // aborting the whole walk.
        tracing::warn!(?e, "zlob extra_ignore rejected; walking without it");
    }

    let pairs = parking_lot::Mutex::new(Vec::<(FileItem, String)>::new());

    let outcome = match builder.run(|entry| {
        if !entry.is_file() {
            return WalkState::Continue;
        }
        let rel_bytes = entry.relative_path_bytes();

        // `basename()` returns `&str` for files only.
        let basename = entry.basename().unwrap_or("");
        let is_binary = is_known_binary_extension_basename(basename);

        let size = entry.size().unwrap_or(0);
        // zlob reports mtime in ns since the Unix epoch; FileItem wants secs.
        let modified = entry
            .modified_ns()
            .map(|ns| (ns / 1_000_000_000).max(0) as u64)
            .unwrap_or(0);

        let basename_offset = entry.basename_offset_in_relative();
        let rel_str = String::from_utf8_lossy(rel_bytes).into_owned();
        let item = FileItem::new_raw(basename_offset, size, modified, None, is_binary);

        let mut guard = pairs.lock();
        guard.push((item, rel_str));
        let n = guard.len();
        drop(guard);

        if n % PROGRESS_STEP == 0 {
            synced_files_count.store(n, Ordering::Relaxed);
        }

        WalkState::Continue
    }) {
        Ok(outcome) => outcome,
        Err(e) => {
            // Preserve whatever we collected before the failure so the caller
            // can still surface a partial index instead of nothing.
            tracing::error!(?e, "zlob walk failed");
            return Err(crate::Error::WalkFailed(format!("{e:?}")));
        }
    };

    let pairs = pairs.into_inner();
    // Always report the exact final total regardless of the last step.
    synced_files_count.store(pairs.len(), Ordering::Relaxed);

    // Retain the ignore rules only when the walk actually gathered some
    // (git roots with .gitignore/.ignore). Otherwise callers fall back.
    let ignore_rules = outcome
        .rules()
        .is_some()
        .then(|| WalkIgnoreRules { inner: outcome });

    Ok(WalkOutput {
        pairs,
        ignore_rules,
    })
}
