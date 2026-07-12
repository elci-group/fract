use crate::error::{Context, Result};
use git2::{Repository, StatusOptions};
use std::path::Path;

pub fn open_repo(root: &Path) -> Result<Repository> {
    Repository::discover(root).context("failed to discover git repository")
}

pub fn is_working_tree_clean(repo: &Repository) -> Result<bool> {
    let mut opts = StatusOptions::new();
    opts.include_ignored(false);
    let statuses = repo.statuses(Some(&mut opts))?;
    Ok(statuses.is_empty())
}

pub fn current_branch(repo: &Repository) -> Result<String> {
    let head = repo.head()?;
    let name = head
        .shorthand()
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "HEAD".to_string());
    Ok(name)
}

pub fn last_commit_sha(repo: &Repository) -> Result<String> {
    let head = repo.head()?;
    let oid = head.target().context("detached HEAD without oid")?;
    Ok(oid.to_string())
}

/// Returns true if `path` has uncommitted changes.
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

pub fn stability(root: &Path) -> Result<Stability> {
    let repo = open_repo(root)?;
    if is_working_tree_clean(&repo)? {
        Ok(Stability::Clean)
    } else {
        Ok(Stability::Dirty)
    }
}
