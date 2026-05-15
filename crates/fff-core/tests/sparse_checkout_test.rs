//! Regression test: indexing a sparse-checked-out git repository must not
//! crash and must only surface paths that are actually materialized on
//! disk. libgit2 happily reports statuses for paths that were excluded by
//! `core.sparseCheckout`; `update_git_statuses` already silently skips
//! those (see `file_picker.rs::1346`), this test pins that contract.

use std::fs;
use std::path::Path;
use std::process::Command;

use fff_search::file_picker::FilePicker;
use fff_search::{FilePickerOptions, FuzzySearchOptions, PaginationArgs, QueryParser};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .expect("git binary must be installed to run this test");
    assert!(
        out.status.success(),
        "git {args:?} failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

fn fuzzy_search_paths(picker: &FilePicker, query: &str) -> Vec<String> {
    let parser = QueryParser::default();
    let parsed = parser.parse(query);
    let result = picker.fuzzy_search(
        &parsed,
        None,
        FuzzySearchOptions {
            max_threads: 1,
            pagination: PaginationArgs {
                offset: 0,
                limit: 200,
            },
            ..Default::default()
        },
    );
    result
        .items
        .iter()
        .map(|f| f.relative_path(picker))
        .collect()
}

/// End-to-end: build a "remote" repo with files in two top-level dirs,
/// clone it with a cone-mode sparse-checkout that materializes only one
/// dir, then index the worktree. The picker must:
///   * see the materialized files,
///   * NOT see the sparse-excluded files,
///   * not panic when libgit2 hands it statuses for non-materialized paths.
#[test]
fn sparse_checkout_indexes_only_materialized_files() {
    let upstream_tmp = TempDir::new().unwrap();
    let upstream = upstream_tmp.path();

    // Create a small repo with two top-level dirs.
    git(upstream, &["init", "-b", "main"]);
    fs::create_dir_all(upstream.join("included/sub")).unwrap();
    fs::create_dir_all(upstream.join("excluded/deep")).unwrap();
    fs::write(upstream.join("README.md"), "root readme\n").unwrap();
    fs::write(upstream.join("included/in_a.rs"), "// a\n").unwrap();
    fs::write(upstream.join("included/sub/in_b.rs"), "// b\n").unwrap();
    fs::write(upstream.join("excluded/ex_a.rs"), "// ea\n").unwrap();
    fs::write(upstream.join("excluded/deep/ex_b.rs"), "// eb\n").unwrap();
    git(upstream, &["add", "-A"]);
    git(upstream, &["commit", "-m", "seed", "--no-gpg-sign"]);

    // Clone with cone-mode sparse-checkout that only materializes /included.
    let work_tmp = TempDir::new().unwrap();
    let work = work_tmp.path();
    git(
        work,
        &[
            "clone",
            "--no-local",
            "--filter=blob:none",
            "--no-checkout",
            upstream.to_str().unwrap(),
            ".",
        ],
    );
    git(work, &["sparse-checkout", "init", "--cone"]);
    git(work, &["sparse-checkout", "set", "included"]);
    git(work, &["checkout", "main"]);

    // Sanity: only the included dir should exist on disk (cone-mode keeps
    // top-level files like README.md too).
    assert!(work.join("included/in_a.rs").is_file());
    assert!(work.join("included/sub/in_b.rs").is_file());
    assert!(!work.join("excluded").exists());

    let mut picker = FilePicker::new(FilePickerOptions {
        base_path: work.to_string_lossy().to_string(),
        enable_mmap_cache: false,
        enable_content_indexing: false,
        watch: false,
        ..Default::default()
    })
    .expect("failed to create FilePicker");
    picker
        .collect_files()
        .expect("collect_files must not panic on a sparse-checked repo");

    let indexed: Vec<String> = picker
        .get_files()
        .iter()
        .map(|f| f.relative_path(&picker))
        .collect();

    assert!(
        indexed.iter().any(|p| p.ends_with("in_a.rs")),
        "expected to find included/in_a.rs in index, got {:?}",
        indexed
    );
    assert!(
        indexed.iter().any(|p| p.ends_with("in_b.rs")),
        "expected to find included/sub/in_b.rs in index, got {:?}",
        indexed
    );
    assert!(
        indexed
            .iter()
            .all(|p| !p.contains("ex_a.rs") && !p.contains("ex_b.rs")),
        "sparse-excluded files leaked into the index: {:?}",
        indexed
    );

    let hits = fuzzy_search_paths(&picker, "in_b");
    assert!(
        hits.iter().any(|p| p.ends_with("in_b.rs")),
        "fuzzy search must surface materialized files, got {:?}",
        hits
    );
}
