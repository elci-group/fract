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

/// Branch name for a proposal, e.g. `fract/7`.
pub fn branch_name(id: &str) -> String {
    let slug = id.strip_prefix("fract-").unwrap_or(id);
    format!("fract/{slug}")
}

/// Create (if needed) and check out a per-proposal branch from the current
/// HEAD using a *safe* checkout that refuses to clobber local edits. Returns
/// the branch name. Subsequent `apply`/`commit` happen on this branch, never
/// the caller's original branch.
pub fn checkout_branch(root: &Path, id: &str) -> Result<String> {
    let repo = crate::git::open_repo(root)?;
    let name = branch_name(id);
    let head = repo.head()?.peel_to_commit()?;
    let branch = match repo.branch(&name, &head, false) {
        Ok(b) => b,
        Err(e) if e.code() == git2::ErrorCode::Exists => {
            repo.find_branch(&name, git2::BranchType::Local)?
        }
        Err(e) => return Err(e.into()),
    };
    let refname = branch
        .get()
        .name()
        .ok_or_else(|| "invalid branch ref".to_string())?;
    repo.set_head(refname)?;
    // Safe (non-forced) checkout: abort rather than overwrite local edits.
    let mut checkout = git2::build::CheckoutBuilder::new();
    repo.checkout_head(Some(&mut checkout))
        .with_context(|| format!("checkout {name} (is the worktree clean?)"))?;
    Ok(name)
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

/// Apply the refactored files carried on the proposal to the working tree.
pub async fn apply(root: &Path, proposal: &mut Proposal) -> Result<()> {
    proposal.timeline.push(TimelineEvent {
        at: now(),
        message: "Applying refactored files".to_string(),
    });

    for cf in &proposal.changed_files {
        let full = root.join(&cf.path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&full, &cf.content)
            .with_context(|| format!("writing {}", full.display()))?;
    }

    proposal.timeline.push(TimelineEvent {
        at: now(),
        message: "Files written to working tree".to_string(),
    });

    Ok(())
}

/// Stage *only* the proposal's changed paths and commit on the current branch
/// (which `checkout_branch` pointed at a per-proposal branch). Guards against a
/// missing git signature. Returns the new commit id as a hex string.
pub async fn commit(root: &Path, proposal: &mut Proposal, message: &str) -> Result<String> {
    let repo = crate::git::open_repo(root)?;

    let signature = repo
        .signature()
        .context("git signature (user.name/user.email) is not configured; refusing to commit")?;

    let mut index = repo.index()?;
    // Scope staging to exactly the paths this proposal touched — never `add_all`.
    let mut staged = 0usize;
    for cf in &proposal.changed_files {
        if index.add_path(cf.path.as_path()).is_ok() {
            staged += 1;
        }
    }
    if staged == 0 {
        // changed_files empty (e.g. detect-only): at least stage the module.
        let _ = index.add_path(proposal.module.as_path());
    }
    index.write()?;

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
    let sha = oid.to_string();

    proposal.status = ProposalStatus::Merged;
    proposal.timeline.push(TimelineEvent {
        at: now(),
        message: format!("Committed as {sha}"),
    });
    info!(
        event = "merge.commit",
        proposal = %proposal.id,
        sha = %sha,
        "committed proposal"
    );

    Ok(sha)
}

/// Render a unified diff between the last commit and its parent — i.e. the
/// change the proposal just introduced. Returns an empty string when there is
/// no parent (initial commit) so rendering never fails the merge.
pub fn diff_last_commit(root: &Path) -> Result<String> {
    let repo = crate::git::open_repo(root)?;
    let commit = repo.head()?.peel_to_commit()?;
    let new_tree = commit.tree()?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&new_tree), None)?;

    let mut text = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        match line.origin() {
            '+' | '-' | ' ' => text.push(line.origin()),
            _ => {}
        }
        if let Ok(s) = std::str::from_utf8(line.content()) {
            text.push_str(s);
        }
        true
    })?;
    Ok(text)
}

fn last_modified(path: &Path) -> Result<SystemTime> {
    let meta = std::fs::metadata(path)?;
    Ok(meta.modified()?)
}

async fn has_conflicts(root: &Path, proposal: &Proposal) -> Result<bool> {
    let repo = crate::git::open_repo(root)?;
    for path in changed_paths(proposal).iter() {
        if crate::git::has_uncommitted_changes(&repo, path)? {
            warn!(
                event = "merge.conflict",
                path = %path.display(),
                "path has uncommitted changes"
            );
            return Ok(true);
        }
    }
    Ok(false)
}

fn changed_paths(proposal: &Proposal) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = proposal
        .changed_files
        .iter()
        .map(|cf| cf.path.clone())
        .collect();
    if paths.is_empty() {
        paths.push(proposal.module.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::now;
    use crate::{ChangedFile, DiffSummary, ProposalStatus, RefactorKind};
    use git2::{Repository, Signature};
    use std::path::PathBuf;

    fn unique_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "fract-merge-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        dir
    }

    /// Init a repo with one committed file. If `configure_sig` is set, the
    /// identity lives in repo config (so `repo.signature()` works); otherwise
    /// the baseline commit is made with an ad-hoc signature and the config is
    /// left empty (so `repo.signature()` fails — for the guard test).
    fn temp_repo(label: &str, configure_sig: bool) -> (PathBuf, Repository, String) {
        let dir = unique_dir(label);
        let repo = Repository::init(&dir).unwrap();
        if configure_sig {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.name", "fract-test").unwrap();
            cfg.set_str("user.email", "fract@example.test").unwrap();
        }
        std::fs::write(dir.join("src/lib.rs"), "pub fn a() -> i32 { 1 }\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("src/lib.rs")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = Signature::now("fract-test", "fract@example.test").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        // git2 objects borrow the repo until dropped; end those borrows before
        // moving `repo` into the return value.
        drop(index);
        drop(tree);
        let orig = repo.head().unwrap().shorthand().unwrap().to_string();
        (dir, repo, orig)
    }

    fn proposal(content: &str) -> Proposal {
        Proposal {
            id: "fract-7".to_string(),
            created_at: now(),
            module: PathBuf::from("src/lib.rs"),
            kind: RefactorKind::ExtractFunction,
            confidence: 0.95,
            status: ProposalStatus::Accepted,
            validation: None,
            diff_summary: DiffSummary::default(),
            migration_notes: vec![],
            changed_files: vec![ChangedFile {
                path: PathBuf::from("src/lib.rs"),
                content: content.to_string(),
            }],
            pr_body: None,
            timeline: vec![],
        }
    }

    #[tokio::test]
    async fn commit_lands_on_proposal_branch_not_original() {
        let (dir, _repo, orig) = temp_repo("branch", true);
        let mut p = proposal("pub fn b() -> i32 { 2 }\n");

        let branch = checkout_branch(&dir, &p.id).unwrap();
        assert_eq!(branch, "fract/7");

        // An unrelated dirty file must NOT be swept into the commit.
        std::fs::write(dir.join("src/unrelated.rs"), "// do not stage me\n").unwrap();

        apply(&dir, &mut p).await.unwrap();
        let msg = crate::pr::conventional_commit_message(&p);
        let sha = commit(&dir, &mut p, &msg).await.unwrap();
        assert!(!sha.is_empty());

        let repo = Repository::open(&dir).unwrap();
        assert_eq!(repo.head().unwrap().shorthand().unwrap(), "fract/7");

        // Working tree now shows the refactor on the fract branch.
        let on_branch = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        assert!(on_branch.contains("pub fn b"));

        // The unrelated file stayed untracked (scoped staging, no add_all).
        let status = repo.status_file(Path::new("src/unrelated.rs")).unwrap();
        assert!(
            status.is_wt_new(),
            "unrelated file must remain untracked, got {status:?}"
        );

        // A real unified diff is available for the PR body.
        let diff = diff_last_commit(&dir).unwrap();
        assert!(diff.contains("diff --git"), "expected patch header: {diff}");

        // The original branch is byte-for-byte untouched.
        repo.set_head(&format!("refs/heads/{orig}")).unwrap();
        let mut cb = git2::build::CheckoutBuilder::new();
        cb.force();
        repo.checkout_head(Some(&mut cb)).unwrap();
        let on_orig = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        assert!(
            on_orig.contains("pub fn a"),
            "original branch must keep baseline, got: {on_orig}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn commit_without_signature_is_refused() {
        // Baseline committed with an ad-hoc signature, but repo config has no
        // identity, so `repo.signature()` fails and our guard must trip.
        let (dir, _repo, _orig) = temp_repo("nosig", false);
        let mut p = proposal("pub fn c() -> i32 { 3 }\n");
        checkout_branch(&dir, &p.id).unwrap();
        apply(&dir, &mut p).await.unwrap();
        let err = commit(&dir, &mut p, "msg").await.unwrap_err();
        assert!(
            err.to_string().contains("signature"),
            "expected signature guard error, got: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn branch_name_strips_fract_prefix() {
        assert_eq!(branch_name("fract-42"), "fract/42");
        assert_eq!(branch_name("abc"), "fract/abc");
    }
}
