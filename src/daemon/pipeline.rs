//! Refactor-pipeline driving: full-tree index refresh, queue processing
//! (refactor a candidate, then validate it), and quiet-period-gated
//! auto-merge of validated proposals.

use super::{notify::recompute_health, Daemon};
use crate::{
    confidence,
    config::Mode,
    error::{Context, Result},
    merge, refactor, scratch,
    time::now,
    validation, Module, ProjectHealth, Proposal, ProposalStatus, RefactorKind, TimelineEvent,
    ValidationReport,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tracing::{info, warn};

impl Daemon {
    #[tracing::instrument(skip(self))]
    pub(crate) async fn refresh_index(self: &Arc<Self>) -> Result<()> {
        let indexer = crate::indexer::Indexer::new(
            self.config.project_root.clone(),
            self.config.ignore_patterns.clone(),
        );
        // The full-tree std::fs walk must not block a tokio worker thread.
        let modules = tokio::task::spawn_blocking(move || indexer.index()).await??;
        self.queue
            .refresh(&modules, self.config.entropy_threshold)
            .await;

        let mut health = self.project_health.write().await;
        *health = recompute_health(&modules, &health);
        let total = health.total_modules;
        let healthy = health.healthy;
        let warning = health.warning;
        let critical = health.critical;
        let score = health.score;
        let health_snapshot = health.clone();
        drop(health);

        persist_health_snapshot(&self.store, &health_snapshot).await;

        let mut stored = self.modules.write().await;
        *stored = modules;

        info!(
            event = "index.refresh",
            total, healthy, warning, critical, score, "index refreshed"
        );
        Ok(())
    }

    #[tracing::instrument(skip(self), fields(candidate = tracing::field::Empty))]
    pub(crate) async fn process_queue(self: &Arc<Self>) -> Result<()> {
        if let Some(path) = self.queue.next_candidate().await {
            tracing::Span::current().record("candidate", tracing::field::display(path.display()));
            let modules = self.modules.read().await;
            let module = modules
                .iter()
                .find(|m| m.path == path)
                .cloned()
                .context("candidate disappeared")?;
            drop(modules);

            let kind = classify_kind(&module);
            let mut proposal = crate::queue::proposal_for(&module, kind);
            info!(
                event = "candidate.process",
                module = %path.display(),
                entropy = module.entropy,
                kind = %kind,
                "processing candidate"
            );

            // Build and execute refactor.
            let output = refactor::execute_proposal(
                self.engine.as_ref(),
                &self.config.project_root,
                &mut proposal,
                &module,
            )
            .await?;

            // Validate in a scratch copy.
            let scratch = self.prepare_scratch(&output).await?;
            let mut proposal = proposal.clone();
            let report = validation::validate(&scratch, &mut proposal).await;
            let confidence = confidence::score(&module, &proposal, &report);
            proposal.confidence = confidence;

            if report.all_passed() && confidence >= self.config.confidence_threshold {
                proposal.status = ProposalStatus::Accepted;
            } else {
                proposal.status = ProposalStatus::Rejected;
            }

            let persisted = proposal.clone();
            self.queue.enqueue_proposal(proposal).await;
            persist_proposal(&self.store, &persisted).await;

            // Clean up scratch.
            let _ = tokio::fs::remove_dir_all(&scratch).await;
        }
        Ok(())
    }

    async fn prepare_scratch(&self, output: &refactor::RefactorOutput) -> Result<PathBuf> {
        let root = tokio::task::spawn_blocking(|| scratch::temp_dir("fract")).await??;
        // Copy project into scratch.
        copy_dir_all(self.config.project_root.clone(), root.clone()).await?;
        // Apply refactored files.
        for (rel_path, content) in &output.files {
            let full = root.join(rel_path);
            if let Some(parent) = full.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&full, content).await?;
        }
        Ok(root)
    }

    #[tracing::instrument(skip(self))]
    pub(crate) async fn attempt_merges(self: &Arc<Self>) -> Result<()> {
        if self.config.mode == Mode::Passive {
            return Ok(());
        }

        let proposals = self.queue.proposals().await;
        for mut proposal in proposals {
            if proposal.status != ProposalStatus::Accepted {
                continue;
            }
            let safety = merge::assess(
                &self.config.project_root,
                &proposal,
                Duration::from_secs(self.config.quiet_period_secs),
            )
            .await?;

            if !safety.can_merge() {
                if safety.conflicts {
                    self.queue
                        .update_proposal(&proposal.id, |p| {
                            p.status = ProposalStatus::Conflicts;
                            p.timeline.push(TimelineEvent {
                                at: now(),
                                message: "Conflicts detected; would create PR".to_string(),
                            });
                        })
                        .await;
                }
                continue;
            }

            // Work on a per-proposal branch so the caller's branch is never
            // modified, and stage only the paths this proposal touched.
            let branch = match merge::checkout_branch(&self.config.project_root, &proposal.id) {
                Ok(b) => b,
                Err(e) => {
                    warn!(
                        event = "merge.checkout_failed",
                        proposal = %proposal.id,
                        error = %e,
                        "checkout failed; treating as conflict"
                    );
                    continue;
                }
            };
            merge::apply(&self.config.project_root, &mut proposal).await?;
            self.finalize_merge(&mut proposal, &branch).await?;

            self.queue
                .update_proposal(&proposal.id, |p| *p = proposal.clone())
                .await;

            persist_proposal(&self.store, &proposal).await;
            let health_snapshot = self.project_health.read().await.clone();
            persist_health_snapshot(&self.store, &health_snapshot).await;
        }
        Ok(())
    }

    /// Finish an applied merge: commit and render the PR body in autonomous
    /// mode, or leave the branch staged with a note in assisted mode.
    async fn finalize_merge(&self, proposal: &mut Proposal, branch: &str) -> Result<()> {
        if self.config.mode == Mode::Autonomous {
            let message = crate::pr::conventional_commit_message(proposal);
            let sha = merge::commit(&self.config.project_root, proposal, &message).await?;
            let diff = match merge::diff_last_commit(&self.config.project_root) {
                Ok(d) => d,
                Err(e) => {
                    warn!(
                        event = "merge.diff_failed",
                        proposal = %proposal.id,
                        error = %e,
                        "failed to compute commit diff; using empty diff"
                    );
                    String::new()
                }
            };
            proposal.pr_body = Some(crate::pr::render_pr_body(proposal, &diff, branch, &sha));

            let mut health = self.project_health.write().await;
            health.refactors_today.completed += 1;
            health.refactors_today.loc_removed += proposal.diff_summary.lines_removed;
            health.refactors_today.complexity_reduced +=
                proposal.diff_summary.lines_removed as f64 / 100.0;
        } else {
            // Assisted: branch created and files written, left uncommitted.
            proposal.status = ProposalStatus::Accepted;
            proposal.pr_body = Some(crate::pr::render_pr_body(proposal, "", branch, ""));
            proposal.timeline.push(TimelineEvent {
                at: now(),
                message: format!("Branch {branch} prepared (assisted mode)"),
            });
        }
        Ok(())
    }

    /// Build (and enqueue) a proposal for each currently-indexed module whose
    /// entropy is at or above `config.entropy_threshold`. Confidence is derived
    /// from a nominal passing validation report so the result is deterministic.
    ///
    /// # Errors
    /// Currently infallible in practice — persistence failures are logged, not
    /// propagated — but the signature is `Result` for parity with the other
    /// pipeline stages.
    #[tracing::instrument(skip(self))]
    pub async fn detect_proposals(self: &Arc<Self>) -> Result<Vec<Proposal>> {
        let modules = self.modules.read().await.clone();
        let threshold = self.config.entropy_threshold;
        let report = ValidationReport {
            fmt_ok: true,
            clippy_ok: true,
            check_ok: true,
            test_ok: true,
            api_compatible: true,
            coverage_delta: 0.0,
            complexity_delta: 0.0,
            logs: Vec::new(),
        };
        let mut produced = Vec::new();
        for module in modules.into_iter().filter(|m| m.entropy >= threshold) {
            let kind = classify_kind(&module);
            let mut proposal = crate::queue::proposal_for(&module, kind);
            proposal.confidence = confidence::score(&module, &proposal, &report);
            proposal.status = ProposalStatus::Detected;
            self.queue.enqueue_proposal(proposal.clone()).await;
            persist_proposal(&self.store, &proposal).await;
            produced.push(proposal);
        }
        Ok(produced)
    }
}

/// Line count above which a module is split rather than refactored in place.
const SPLIT_LINES_THRESHOLD: usize = 1500;
/// Function count above which a module is split rather than refactored in place.
const SPLIT_FUNCTIONS_THRESHOLD: usize = 40;
/// Duplicate-line count that triggers a deduplication proposal.
const DUPLICATES_THRESHOLD: usize = 10;
/// Public-API size that triggers a surface-reduction proposal.
const API_SIZE_THRESHOLD: usize = 30;
/// Fan-out that triggers a dependency-reordering proposal.
const FAN_OUT_THRESHOLD: usize = 15;

/// Persist a proposal to the journal, logging (not propagating) failure: the
/// store is a cache, so a failed append must never abort the pipeline.
async fn persist_proposal(store: &crate::store::Store, proposal: &Proposal) {
    if let Err(e) = store.append_proposal_async(proposal).await {
        warn!(
            event = "store.append_failed",
            proposal = %proposal.id,
            error = %e,
            "failed to persist proposal"
        );
    }
}

/// Persist a health snapshot to the journal, logging (not propagating)
/// failure — same rationale as [`persist_proposal`].
async fn persist_health_snapshot(store: &crate::store::Store, health: &ProjectHealth) {
    if let Err(e) = store.append_health_async(health).await {
        warn!(
            event = "store.append_failed",
            error = %e,
            "failed to persist health snapshot"
        );
    }
}

fn classify_kind(module: &Module) -> RefactorKind {
    // First matching rule wins; the comparisons are cheap and side-effect free,
    // so evaluating them eagerly keeps the priority order table-shaped.
    let rules = [
        (
            module.lines > SPLIT_LINES_THRESHOLD || module.functions > SPLIT_FUNCTIONS_THRESHOLD,
            RefactorKind::SplitModule,
        ),
        (
            module.duplicates > DUPLICATES_THRESHOLD,
            RefactorKind::RemoveDuplication,
        ),
        (
            module.public_api_size > API_SIZE_THRESHOLD,
            RefactorKind::ReduceSurface,
        ),
        (
            module.fan_out > FAN_OUT_THRESHOLD,
            RefactorKind::ReorderDependencies,
        ),
    ];
    rules
        .iter()
        .find(|(applies, _)| *applies)
        .map_or(RefactorKind::ExtractFunction, |(_, kind)| *kind)
}

async fn copy_dir_all(src: PathBuf, dst: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || copy_dir_all_sync(&src, &dst)).await?
}

fn copy_dir_all_sync(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let dst_path = dst.join(&name);
        if entry.file_type()?.is_dir() {
            copy_dir_all_sync(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Health, Language};
    use std::time::SystemTime;

    fn temp_dir() -> PathBuf {
        // Rust runs the test binary's tests in parallel threads within one
        // process, so a pid-only name would collide. Mix in a per-call counter.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("fract-pipeline-test-{}-{n}", std::process::id()));
        cleanup(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Remove a temp project dir, ignoring errors (cleanup must not fail tests).
    fn cleanup(dir: &std::path::Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    fn module(path: &str) -> Module {
        Module {
            path: PathBuf::from(path),
            language: Language::Rust,
            lines: 0,
            functions: 0,
            cyclomatic_complexity: 0,
            public_api_size: 0,
            fan_out: 0,
            fan_in: 0,
            duplicates: 0,
            edit_frequency: 0.0,
            confidence: None,
            churn: 0,
            test_coverage: 0.0,
            entropy: 0.9,
            health: Health::from_entropy(0.9),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn classify_kind_prefers_split_for_oversized_modules() {
        let mut oversized_lines = module("src/big.rs");
        oversized_lines.lines = 1_501;
        assert_eq!(classify_kind(&oversized_lines), RefactorKind::SplitModule);

        let mut many_functions = module("src/many.rs");
        many_functions.functions = 41;
        assert_eq!(classify_kind(&many_functions), RefactorKind::SplitModule);
    }

    #[test]
    fn classify_kind_boundary_values_fall_through() {
        // Exact thresholds are NOT over the limit: each boundary value must
        // fall through to the next rule, ending at ExtractFunction.
        let mut edge = module("src/edge.rs");
        edge.lines = 1_500;
        edge.functions = 40;
        edge.duplicates = 10;
        edge.public_api_size = 30;
        edge.fan_out = 15;
        assert_eq!(classify_kind(&edge), RefactorKind::ExtractFunction);
    }

    #[test]
    fn classify_kind_picks_first_matching_rule() {
        let mut duplicated = module("src/dup.rs");
        duplicated.duplicates = 11;
        assert_eq!(classify_kind(&duplicated), RefactorKind::RemoveDuplication);

        let mut wide_api = module("src/api.rs");
        wide_api.public_api_size = 31;
        assert_eq!(classify_kind(&wide_api), RefactorKind::ReduceSurface);

        let mut tangled = module("src/fan.rs");
        tangled.fan_out = 16;
        assert_eq!(classify_kind(&tangled), RefactorKind::ReorderDependencies);

        assert_eq!(
            classify_kind(&module("src/plain.rs")),
            RefactorKind::ExtractFunction
        );
    }

    #[test]
    fn classify_kind_split_beats_duplication_when_both_match() {
        let mut both = module("src/both.rs");
        both.functions = 41;
        both.duplicates = 99;
        assert_eq!(classify_kind(&both), RefactorKind::SplitModule);
    }

    #[tokio::test]
    async fn detect_proposals_gates_on_entropy_threshold() {
        let root = temp_dir();
        let daemon = Arc::new(Daemon::new(crate::config::Config::default_for(
            root.clone(),
        )));
        let threshold = daemon.config().entropy_threshold;
        {
            let mut low = module("src/low.rs");
            low.entropy = threshold - 0.01;
            let mut edge = module("src/edge.rs");
            edge.entropy = threshold;
            let mut high = module("src/high.rs");
            high.entropy = 0.99;
            let mut modules = daemon.modules.write().await;
            *modules = vec![low, edge, high];
        }
        let produced = daemon.detect_proposals().await.unwrap();
        let paths: Vec<_> = produced.iter().map(|p| p.module.clone()).collect();
        assert_eq!(
            paths,
            vec![PathBuf::from("src/edge.rs"), PathBuf::from("src/high.rs")]
        );
        assert!(produced
            .iter()
            .all(|p| p.status == ProposalStatus::Detected));
        assert!(produced.iter().all(|p| p.confidence > 0.0));
        // Enqueued copies carry the queue-entry status.
        let queued = daemon.proposals().await;
        assert_eq!(queued.len(), 2);
        assert!(queued.iter().all(|p| p.status == ProposalStatus::Queued));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn detect_proposals_empty_when_nothing_exceeds_threshold() {
        let root = temp_dir();
        let daemon = Arc::new(Daemon::new(crate::config::Config::default_for(
            root.clone(),
        )));
        {
            let mut calm = module("src/calm.rs");
            calm.entropy = 0.1;
            let mut modules = daemon.modules.write().await;
            *modules = vec![calm];
        }
        let produced = daemon.detect_proposals().await.unwrap();
        assert!(produced.is_empty());
        assert!(daemon.proposals().await.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_dir_all_sync_copies_tree_and_skips_dot_git() {
        let src = temp_dir();
        std::fs::create_dir_all(src.join("src")).unwrap();
        std::fs::create_dir_all(src.join(".git/objects")).unwrap();
        std::fs::write(src.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(src.join(".git/objects/blob"), "gitdata").unwrap();
        std::fs::write(src.join("Cargo.toml"), "[package]\n").unwrap();

        let dst = temp_dir();
        copy_dir_all_sync(&src, &dst).unwrap();
        assert_eq!(
            std::fs::read_to_string(dst.join("src/lib.rs")).unwrap(),
            "pub fn a() {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(dst.join("Cargo.toml")).unwrap(),
            "[package]\n"
        );
        assert!(!dst.join(".git").exists());
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    /// Hand-rolled git project (git2, no shell-out) with a configured identity
    /// and one committed file, for the merge-path tests.
    fn git_project() -> PathBuf {
        let dir = temp_dir();
        let repo = git2::Repository::init(&dir).unwrap();
        {
            let mut cfg = repo.config().unwrap();
            cfg.set_str("user.name", "fract-test").unwrap();
            cfg.set_str("user.email", "fract-test@example.com").unwrap();
        }
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn a() -> i32 { 1 }\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("src/lib.rs")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::new(
            "fract-test",
            "fract-test@example.com",
            &git2::Time::new(1_700_000_000, 0),
        )
        .unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        dir
    }

    fn accepted_proposal(id: &str, content: &str) -> Proposal {
        Proposal {
            id: id.to_string(),
            created_at: SystemTime::UNIX_EPOCH,
            module: PathBuf::from("src/lib.rs"),
            kind: RefactorKind::ExtractFunction,
            confidence: 0.95,
            status: ProposalStatus::Accepted,
            validation: None,
            diff_summary: crate::DiffSummary::default(),
            migration_notes: Vec::new(),
            changed_files: vec![crate::ChangedFile {
                path: PathBuf::from("src/lib.rs"),
                content: content.to_string(),
            }],
            pr_body: None,
            timeline: Vec::new(),
        }
    }

    /// Enqueue a proposal and promote it to `Accepted` (enqueue forces Queued).
    async fn enqueue_accepted(daemon: &Arc<Daemon>, proposal: Proposal) {
        let id = proposal.id.clone();
        daemon.queue.enqueue_proposal(proposal).await;
        daemon
            .queue
            .update_proposal(&id, |p| p.status = ProposalStatus::Accepted)
            .await;
    }

    /// Fresh git project plus a daemon in `mode` with one accepted proposal
    /// enqueued; callers run `attempt_merges` and assert on the outcome.
    async fn daemon_with_accepted(mode: Mode, id: &str) -> (PathBuf, Arc<Daemon>) {
        let root = git_project();
        let mut cfg = crate::config::Config::default_for(root.clone());
        cfg.mode = mode;
        cfg.quiet_period_secs = 0;
        let daemon = Arc::new(Daemon::new(cfg));
        enqueue_accepted(&daemon, accepted_proposal(id, "pub fn b() -> i32 { 2 }\n")).await;
        (root, daemon)
    }

    #[tokio::test]
    async fn attempt_merges_passive_mode_is_a_noop() {
        let (root, daemon) = daemon_with_accepted(Mode::Passive, "fract-1").await;
        daemon.attempt_merges().await.unwrap();
        let p = daemon.proposals().await.pop().unwrap();
        assert_eq!(p.status, ProposalStatus::Accepted);
        assert!(p.pr_body.is_none());
        cleanup(&root);
    }

    #[tokio::test]
    async fn attempt_merges_autonomous_commits_and_marks_merged() {
        let (root, daemon) = daemon_with_accepted(Mode::Autonomous, "fract-2").await;
        daemon.attempt_merges().await.unwrap();

        let p = daemon.proposals().await.pop().unwrap();
        assert_eq!(p.status, ProposalStatus::Merged);
        let body = p.pr_body.expect("autonomous merge renders a PR body");
        assert!(body.contains("fract/2"), "body: {body}");
        // The commit landed on the per-proposal branch, never the original.
        let repo = git2::Repository::open(&root).unwrap();
        assert_eq!(repo.head().unwrap().shorthand().unwrap(), "fract/2");
        let health = daemon.project_health().await;
        assert_eq!(health.refactors_today.completed, 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn attempt_merges_assisted_prepares_branch_without_commit() {
        let root = git_project();
        let mut cfg = crate::config::Config::default_for(root.clone());
        cfg.mode = Mode::Assisted;
        cfg.quiet_period_secs = 0;
        let daemon = Arc::new(Daemon::new(cfg));
        enqueue_accepted(
            &daemon,
            accepted_proposal("fract-3", "pub fn b() -> i32 { 2 }\n"),
        )
        .await;
        daemon.attempt_merges().await.unwrap();

        let p = daemon.proposals().await.pop().unwrap();
        assert_eq!(p.status, ProposalStatus::Accepted);
        let body = p.pr_body.expect("assisted merge renders a PR body");
        assert!(body.contains("fract/3"), "body: {body}");
        assert!(p
            .timeline
            .iter()
            .any(|e| e.message.contains("assisted mode")));
        // Files were applied on the branch, but HEAD is still the init commit.
        let repo = git2::Repository::open(&root).unwrap();
        assert_eq!(repo.head().unwrap().shorthand().unwrap(), "fract/3");
        let message = repo
            .head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .message()
            .unwrap()
            .to_string();
        assert_eq!(message, "init");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn attempt_merges_marks_conflicting_proposal() {
        let root = git_project();
        let mut cfg = crate::config::Config::default_for(root.clone());
        cfg.mode = Mode::Autonomous;
        cfg.quiet_period_secs = 0;
        let daemon = Arc::new(Daemon::new(cfg));
        enqueue_accepted(
            &daemon,
            accepted_proposal("fract-4", "pub fn b() -> i32 { 2 }\n"),
        )
        .await;
        // Dirty the target path so the conflict scan trips.
        std::fs::write(root.join("src/lib.rs"), "pub fn local_edit() {}\n").unwrap();
        daemon.attempt_merges().await.unwrap();

        let p = daemon.proposals().await.pop().unwrap();
        assert_eq!(p.status, ProposalStatus::Conflicts);
        assert!(p.timeline.iter().any(|e| e.message.contains("Conflicts")));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// End-to-end queue processing against a real cargo project. Intentionally
    /// slow (runs cargo fmt/clippy/check/test in a scratch copy) — the single
    /// gated pipeline test, mirroring `validation`'s `validate_pipeline_*`.
    #[tokio::test]
    async fn process_queue_refactors_validates_and_accepts_candidate() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"fract-pipeline-scratch\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/lib.rs"),
            "//! Scratch lib.\n\n/// Adds one.\n#[must_use]\npub fn add_one(x: i32) -> i32 {\n    x + 1\n}\n",
        )
        .unwrap();
        // ~600-line Python module: entropy ≈ 0.67 (measured via `fract index`),
        // safely above the 0.6 threshold while lib.rs (≈ 0.50) stays below.
        let mut big = String::new();
        for i in 0..120 {
            use std::fmt::Write as _;
            let _ = writeln!(
                big,
                "def func_{i}(x):\n    if x > {i}:\n        return x + {i}\n    return x\n"
            );
        }
        std::fs::write(root.join("src/big.py"), &big).unwrap();

        let mut cfg = crate::config::Config::default_for(root.clone());
        cfg.entropy_threshold = 0.6;
        let daemon = Arc::new(Daemon::new(cfg));
        daemon.refresh_index().await.unwrap();
        daemon.process_queue().await.unwrap();

        let proposals = daemon.proposals().await;
        assert_eq!(proposals.len(), 1);
        let p = &proposals[0];
        assert_eq!(p.module, PathBuf::from("src/big.py"));
        let report = p.validation.as_ref().expect("validation report recorded");
        assert!(report.all_passed(), "logs: {:?}", report.logs);
        // The mock engine only rewrites the Python file, which cargo never
        // checks, so the scratch crate still builds and confidence clears the
        // 0.90 default threshold. The queue entry is forced to `Queued` by
        // `enqueue_proposal`; the real status lives on the persisted clone.
        assert!(p.confidence >= 0.9, "confidence {}", p.confidence);
        let persisted = daemon.store.load().proposals;
        let stored = persisted
            .iter()
            .find(|p| p.module == Path::new("src/big.py"))
            .expect("persisted proposal");
        assert_eq!(stored.status, ProposalStatus::Accepted);
        let _ = std::fs::remove_dir_all(&root);
    }
}
