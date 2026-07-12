use super::notify::recompute_health;
use super::Daemon;
use crate::error::{Context, Result};
use crate::scratch;
use crate::time::now;
use crate::{
    confidence, config::Mode, merge, refactor, validation, Module, Proposal, ProposalStatus,
    RefactorKind, TimelineEvent, ValidationReport,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
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

        if let Err(e) = self.store.append_health_async(&health_snapshot).await {
            warn!(
                event = "store.append_failed",
                error = %e,
                "failed to persist health snapshot"
            );
        }

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
            let _ = self.store.append_proposal_async(&persisted).await;

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

            if self.config.mode == Mode::Autonomous {
                let message = crate::pr::conventional_commit_message(&proposal);
                let sha = merge::commit(&self.config.project_root, &mut proposal, &message).await?;
                let diff = merge::diff_last_commit(&self.config.project_root).unwrap_or_default();
                proposal.pr_body = Some(crate::pr::render_pr_body(&proposal, &diff, &branch, &sha));

                let mut health = self.project_health.write().await;
                health.refactors_today.completed += 1;
                health.refactors_today.loc_removed += proposal.diff_summary.lines_removed;
                health.refactors_today.complexity_reduced +=
                    proposal.diff_summary.lines_removed as f64 / 100.0;
            } else {
                // Assisted: branch created and files written, left uncommitted.
                proposal.status = ProposalStatus::Accepted;
                proposal.pr_body = Some(crate::pr::render_pr_body(&proposal, "", &branch, ""));
                proposal.timeline.push(TimelineEvent {
                    at: now(),
                    message: format!("Branch {branch} prepared (assisted mode)"),
                });
            }

            self.queue
                .update_proposal(&proposal.id, |p| *p = proposal.clone())
                .await;

            let _ = self.store.append_proposal_async(&proposal).await;
            let health_snapshot = self.project_health.read().await.clone();
            let _ = self.store.append_health_async(&health_snapshot).await;
        }
        Ok(())
    }

    /// Build (and enqueue) a proposal for each currently-indexed module whose
    /// entropy is at or above `config.entropy_threshold`. Confidence is derived
    /// from a nominal passing validation report so the result is deterministic.
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
            let _ = self.store.append_proposal_async(&proposal).await;
            produced.push(proposal);
        }
        Ok(produced)
    }
}

fn classify_kind(module: &Module) -> RefactorKind {
    if module.lines > 1500 || module.functions > 40 {
        RefactorKind::SplitModule
    } else if module.duplicates > 10 {
        RefactorKind::RemoveDuplication
    } else if module.public_api_size > 30 {
        RefactorKind::ReduceSurface
    } else if module.fan_out > 15 {
        RefactorKind::ReorderDependencies
    } else {
        RefactorKind::ExtractFunction
    }
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
