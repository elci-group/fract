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
