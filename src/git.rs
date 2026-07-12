//! Thin `git2` wrappers: repository discovery, working-tree cleanliness,
//! current branch name, and HEAD SHA.

use crate::error::{Context, Result};
use git2::{Repository, StatusOptions};
use std::path::Path;

/// Discover the git repository containing `root`.
///
/// # Errors
/// Returns an error if no repository is found at or above `root`.
pub fn open_repo(root: &Path) -> Result<Repository> {
    Repository::discover(root).context("failed to discover git repository")
}

/// Check whether the working tree has no uncommitted changes.
///
/// # Errors
/// Returns an error if the repository status cannot be computed.
pub fn is_working_tree_clean(repo: &Repository) -> Result<bool> {
    let mut opts = StatusOptions::new();
    opts.include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts))?;
    Ok(statuses.is_empty())
}

/// Name of the current branch (`"HEAD"` when the shorthand is unavailable).
///
/// # Errors
/// Returns an error if `HEAD` cannot be resolved.
pub fn current_branch(repo: &Repository) -> Result<String> {
    let head = repo.head()?;
    let name = head
        .shorthand()
        .map_or_else(|_| "HEAD".to_string(), ToString::to_string);
    Ok(name)
}

/// SHA of the commit `HEAD` points at.
///
/// # Errors
/// Returns an error if `HEAD` cannot be resolved or is detached without a
/// target oid.
pub fn last_commit_sha(repo: &Repository) -> Result<String> {
    let head = repo.head()?;
    let oid = head.target().context("detached HEAD without oid")?;
    Ok(oid.to_string())
}

/// Returns true if `path` has uncommitted changes.
///
/// # Errors
/// Returns an error if the repository status cannot be computed.
pub fn has_uncommitted_changes(repo: &Repository, path: &Path) -> Result<bool> {
    let mut opts = StatusOptions::new();
    opts.include_ignored(false);
    opts.pathspec(path);
    let statuses = repo.statuses(Some(&mut opts))?;
    Ok(!statuses.is_empty())
}

/// Snapshot of repository stability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stability {
    Clean,
    Dirty,
}

/// Classify the repository at `root` as clean or dirty.
///
/// # Errors
/// Returns an error if the repository cannot be discovered or its status
/// cannot be computed.
pub fn stability(root: &Path) -> Result<Stability> {
    let repo = open_repo(root)?;
    if is_working_tree_clean(&repo)? {
        Ok(Stability::Clean)
    } else {
        Ok(Stability::Dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        // Rust runs the test binary's tests in parallel threads within one
        // process, so a pid-only name would collide. Mix in a per-call counter.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("fract-git-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Fixed signature so commits never depend on the wall clock.
    fn sig() -> git2::Signature<'static> {
        git2::Signature::new(
            "Fract Test",
            "test@example.com",
            &git2::Time::new(1_700_000_000, 0),
        )
        .unwrap()
    }

    fn commit_file(repo: &Repository, root: &Path, rel: &str, content: &str, message: &str) {
        let full = root.join(rel);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(rel)).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = sig();
        match repo.head().and_then(|h| h.peel_to_commit()) {
            Ok(parent) => {
                repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])
                    .unwrap();
            }
            Err(_) => {
                repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])
                    .unwrap();
            }
        }
    }

    /// Repository on a fixed branch so `current_branch` does not depend on the
    /// host's `init.defaultBranch` configuration.
    fn temp_repo() -> (PathBuf, Repository) {
        let dir = temp_dir();
        let repo = Repository::init(&dir).unwrap();
        repo.set_head("refs/heads/fract-test").unwrap();
        (dir, repo)
    }

    #[test]
    fn open_repo_discovers_from_nested_dir() {
        let (dir, _repo) = temp_repo();
        let nested = dir.join("src/deep");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(open_repo(&nested).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_repo_outside_any_repository_errors() {
        let dir = temp_dir();
        assert!(open_repo(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn working_tree_clean_after_commit_dirty_after_edit() {
        let (dir, repo) = temp_repo();
        commit_file(&repo, &dir, "src/lib.rs", "pub fn a() {}\n", "initial");
        assert!(is_working_tree_clean(&repo).unwrap());
        std::fs::write(dir.join("src/lib.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();
        assert!(!is_working_tree_clean(&repo).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn untracked_files_are_ignored_by_status() {
        // `StatusOptions` never enables `include_untracked`, so an untracked
        // file alone does not dirty the tree. Lock in that behavior.
        let (dir, repo) = temp_repo();
        commit_file(&repo, &dir, "a.txt", "x\n", "initial");
        std::fs::write(dir.join("new.txt"), "y\n").unwrap();
        assert!(is_working_tree_clean(&repo).unwrap());
        assert!(!has_uncommitted_changes(&repo, Path::new("new.txt")).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn current_branch_reports_fixed_branch_name() {
        let (dir, repo) = temp_repo();
        commit_file(&repo, &dir, "a.txt", "x\n", "initial");
        assert_eq!(current_branch(&repo).unwrap(), "fract-test");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn last_commit_sha_matches_head_target() {
        let (dir, repo) = temp_repo();
        commit_file(&repo, &dir, "a.txt", "x\n", "initial");
        let expected = repo.head().unwrap().target().unwrap().to_string();
        assert_eq!(last_commit_sha(&repo).unwrap(), expected);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn has_uncommitted_changes_is_path_scoped() {
        let (dir, repo) = temp_repo();
        commit_file(&repo, &dir, "a.txt", "x\n", "initial");
        commit_file(&repo, &dir, "b.txt", "y\n", "second");
        assert!(!has_uncommitted_changes(&repo, Path::new("a.txt")).unwrap());
        std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
        assert!(has_uncommitted_changes(&repo, Path::new("a.txt")).unwrap());
        assert!(!has_uncommitted_changes(&repo, Path::new("b.txt")).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stability_classifies_clean_and_dirty() {
        let (dir, repo) = temp_repo();
        commit_file(&repo, &dir, "a.txt", "x\n", "initial");
        assert_eq!(stability(&dir).unwrap(), Stability::Clean);
        std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
        assert_eq!(stability(&dir).unwrap(), Stability::Dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
