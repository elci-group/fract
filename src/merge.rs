use crate::error::{Context, Result};
use crate::time::now;
use crate::{DiffSummary, Proposal, ProposalStatus, TimelineEvent};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tracing::{info, warn};

/// Safety check before merging.
pub struct MergeSafety {
    pub quiet_period: Duration,
    pub stable: bool,
    pub last_edit: Option<SystemTime>,
    pub conflicts: bool,
}

impl MergeSafety {
    pub fn can_merge(&self) -> bool {
        if self.conflicts {
            return false;
        }
        if self.stable {
            return true;
        }
        if let Some(last) = self.last_edit {
            let elapsed = SystemTime::now()
                .duration_since(last)
                .unwrap_or(Duration::MAX);
            return elapsed >= self.quiet_period;
        }
        false
    }
}

/// Assess whether it is safe to merge a proposal.
pub async fn assess(
    root: &Path,
    proposal: &Proposal,
    quiet_period: Duration,
) -> Result<MergeSafety> {
    let module_path = root.join(&proposal.module);
    let last_edit = last_modified(&module_path).ok();
    let stable = crate::git::stability(root)? == crate::git::Stability::Clean;
    let conflicts = has_conflicts(root, proposal).await?;

    Ok(MergeSafety {
        quiet_period,
        stable,
        last_edit,
        conflicts,
    })
}

/// Apply the refactored files to the project tree.
pub async fn apply(
    root: &Path,
    proposal: &mut Proposal,
    files: &HashMap<PathBuf, String>,
) -> Result<()> {
    proposal.timeline.push(TimelineEvent {
        at: now(),
        message: "Applying refactored files".to_string(),
    });

    for (rel_path, content) in files {
        let full = root.join(rel_path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&full, content).with_context(|| format!("writing {}", full.display()))?;
    }

    proposal.timeline.push(TimelineEvent {
        at: now(),
        message: "Files written to working tree".to_string(),
    });

    Ok(())
}

/// Commit the applied changes if the mode is autonomous.
pub async fn commit(root: &Path, proposal: &mut Proposal, message: &str) -> Result<()> {
    let repo = crate::git::open_repo(root)?;
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;

    let signature = repo.signature()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let parent = repo.head()?.peel_to_commit()?;

    let oid = repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &[&parent],
    )?;

    proposal.status = ProposalStatus::Merged;
    proposal.timeline.push(TimelineEvent {
        at: now(),
        message: format!("Committed as {}", oid),
    });
    info!("committed proposal {} as {}", proposal.id, oid);

    Ok(())
}

fn last_modified(path: &Path) -> Result<SystemTime> {
    let meta = std::fs::metadata(path)?;
    Ok(meta.modified()?)
}

async fn has_conflicts(root: &Path, proposal: &Proposal) -> Result<bool> {
    let repo = crate::git::open_repo(root)?;
    for path in changed_paths(proposal).iter() {
        if crate::git::has_uncommitted_changes(&repo, path)? {
            warn!("conflict: {} has uncommitted changes", path.display());
            return Ok(true);
        }
    }
    Ok(false)
}

fn changed_paths(proposal: &Proposal) -> Vec<PathBuf> {
    let paths = vec![proposal.module.clone()];
    if proposal.diff_summary.files_added > 0 {
        // We don't know exact added file names here; the caller passes them.
    }
    paths
}

/// Convert refactor output files into a map for application.
pub fn file_map(files: &[(PathBuf, String)], diff: &mut DiffSummary) -> HashMap<PathBuf, String> {
    let mut map = HashMap::new();
    for (path, content) in files {
        map.insert(path.clone(), content.clone());
    }
    diff.files_added = map.len().saturating_sub(1);
    map
}
