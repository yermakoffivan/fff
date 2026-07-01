//! Filesystem traversal backed by zlob's native parallel walker.
//! Active when the `zlob` feature is enabled (requires the Zig toolchain).

use crate::background_watcher::is_git_file;
use crate::file_picker::is_known_binary_extension;
use crate::ignore::{NON_GIT_IGNORED_DIRS, PLATFORM_IGNORED_DIRS};
use crate::types::FileItem;
use crate::walk::{WalkIgnoreRules, WalkOutput};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use zlob::walk::{IgnoreRules, WalkBuilder, WalkFlags, WalkMetadata, WalkResults};

/// Owns the walk storage so its ignore rules stay valid for the lifetime of
/// the picker/watcher. `IgnoreRules` is a cheap re-derivable handle into this
/// storage, so we re-fetch it per query instead of holding a self-reference.
pub(crate) struct OwnedIgnoreRules {
    results: WalkResults,
}

// WalkResults is Send + Sync; the derived handle only reads immutable storage.
unsafe impl Send for OwnedIgnoreRules {}
unsafe impl Sync for OwnedIgnoreRules {}

impl OwnedIgnoreRules {
    #[inline]
    pub(crate) fn rules(&self) -> IgnoreRules<'_> {
        // Safe to unwrap: only constructed when `ignore_rules()` was `Some`.
        self.results
            .ignore_rules()
            .expect("ignore rules present for the retained walk results")
    }
}

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
) -> WalkOutput {
    // gitignore on; skip hidden on non-git roots (so `~/` doesn't recurse into
    // ~/.cache, ~/.config, etc.); optionally follow symlinks.
    let mut flags = WalkFlags::GITIGNORE;
    if !is_git_repo {
        flags |= WalkFlags::SKIP_HIDDEN;
    }
    if follow_symlinks {
        flags |= WalkFlags::FOLLOW_SYMLINKS;
    }

    let mut builder = WalkBuilder::new(base_path);
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
    if !is_git_repo {
        let extras: Vec<&str> = NON_GIT_IGNORED_DIRS
            .iter()
            .chain(PLATFORM_IGNORED_DIRS)
            .copied()
            .collect();
        if !extras.is_empty() {
            builder.extra_ignore(&extras);
        }
    }

    // `build()` materializes every entry lock-free in one FFI call (the fastest
    // consumption path) and retains the assembled ignore rules for reuse.
    let results = match builder.build() {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(?e, "zlob walk failed");
            return WalkOutput {
                pairs: Vec::new(),
                ignore_rules: None,
            };
        }
    };

    // Convert entries -> (FileItem, rel_path). `WalkResults::iter` yields
    // borrowed FFI entries that aren't `Send`, so build serially; the
    // profiled cost is ~160 ms on a 500k-entry tree.
    let mut pairs: Vec<(FileItem, String)> = Vec::with_capacity(results.len());
    for entry in results.iter() {
        if !entry.is_file() {
            continue;
        }
        let path = entry.path();

        // zlob can surface files inside `.git/` when the base is itself a
        // git repo — skip them.
        if is_git_file(path) {
            continue;
        }

        if !is_git_repo && is_known_binary_extension(path) {
            continue;
        }

        let size = entry.size().unwrap_or(0);
        // zlob reports mtime in ns since the Unix epoch; FileItem wants secs.
        let modified = entry
            .modified_ns()
            .map(|ns| (ns / 1_000_000_000).max(0) as u64)
            .unwrap_or(0);

        pairs.push(FileItem::new_from_walk_parts(
            path, base_path, None, size, modified,
        ));
    }

    synced_files_count.store(pairs.len(), Ordering::Relaxed);

    // Retain the ignore rules only when the walk actually gathered some
    // (git roots with .gitignore/.ignore). Otherwise callers fall back.
    let ignore_rules = results.ignore_rules().is_some().then(|| WalkIgnoreRules {
        inner: OwnedIgnoreRules { results },
    });

    WalkOutput {
        pairs,
        ignore_rules,
    }
}
