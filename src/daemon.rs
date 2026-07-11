use crate::error::{Context, Result};
use crate::scratch;
use crate::time::now;
use crate::{
    confidence, config::Config, config::Mode, events::EventBus, merge, queue::RefactorQueue,
    refactor, store::Store, validation, Event, EventKind, Health, Module, ProjectHealth, Proposal,
    ProposalStatus, RefactorKind, RefactorStats, TimelineEvent, ValidationReport,
};
use notify::{
    Config as NotifyConfig, Event as NotifyEvent, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{error, info, warn};

pub struct Daemon {
    config: Config,
    event_bus: EventBus,
    queue: RefactorQueue,
    modules: Arc<RwLock<Vec<Module>>>,
    project_health: Arc<RwLock<ProjectHealth>>,
    engine: Arc<dyn refactor::RefactorEngine>,
    store: Store,
}

impl Daemon {
    pub fn new(config: Config) -> Self {
        let engine = build_engine(&config.llm);
        let store = Store::open(&config.project_root);
        Self {
            config,
            event_bus: EventBus::new(),
            queue: RefactorQueue::new(),
            modules: Arc::new(RwLock::new(Vec::new())),
            project_health: Arc::new(RwLock::new(ProjectHealth {
                score: 0.0,
                total_modules: 0,
                healthy: 0,
                warning: 0,
                critical: 0,
                entropy_trend: Vec::new(),
                refactors_today: RefactorStats::default(),
            })),
            engine,
            store,
        }
    }

    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Load persisted proposals, events, and health from the journal and seed
    /// the in-memory buses. Infallible: malformed journal lines are skipped.
    async fn restore_from_store(self: &Arc<Self>) {
        let loaded = self.store.load();
        self.event_bus.restore(loaded.events).await;
        self.queue.restore(loaded.proposals).await;
        if let Some(h) = loaded.health {
            *self.project_health.write().await = h;
        }
    }

    #[tracing::instrument(skip(self), fields(mode = %self.config.mode))]
    pub async fn run(self: Arc<Self>) -> Result<()> {
        info!(event = "daemon.start", "starting fract daemon");

        // Restore persisted state before the first refresh. `restore_from_store`
        // is infallible by construction (corrupt journal lines are skipped), so
        // a bad journal can never abort startup.
        self.restore_from_store().await;

        self.refresh_index().await?;

        let watch_root = self.config.project_root.clone();
        let _event_bus = self.event_bus.clone();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<notify::Result<NotifyEvent>>(256);

        let mut watcher: RecommendedWatcher = Watcher::new(
            move |res| {
                let _ = tx.blocking_send(res);
            },
            NotifyConfig::default(),
        )?;
        watcher.watch(&watch_root, RecursiveMode::Recursive)?;

        let daemon = self.clone();
        tokio::spawn(async move {
            loop {
                if let Some(Ok(event)) = rx.recv().await {
                    daemon.handle_notify_event(event).await;
                }
            }
        });

        // Periodic reindexing and queue processing.
        let daemon = self.clone();
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                if let Err(e) = daemon.refresh_index().await {
                    error!(event = "reindex.failed", error = %e, "reindex failed");
                }
                if let Err(e) = daemon.process_queue().await {
                    error!(event = "queue.failed", error = %e, "queue processing failed");
                }
            }
        });

        // Merge safety watcher.
        let daemon = self.clone();
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(10));
            loop {
                tick.tick().await;
                if let Err(e) = daemon.attempt_merges().await {
                    error!(event = "merge.failed", error = %e, "merge attempts failed");
                }
            }
        });

        Ok(())
    }

    async fn handle_notify_event(self: &Arc<Self>, event: NotifyEvent) {
        for path in event.paths {
            if self.is_ignored(&path) {
                continue;
            }
            if path.is_dir() {
                continue;
            }
            // Deletes have no content to re-index: drop the cached module and
            // skip emitting a save event.
            if matches!(event.kind, notify::EventKind::Remove(_)) {
                self.index_one_path(&path).await;
                continue;
            }
            let kind = match event.kind {
                notify::EventKind::Modify(_) => EventKind::FileSaved,
                notify::EventKind::Create(_) => EventKind::FileSaved,
                _ => EventKind::EditorHeartbeat,
            };
            let ev = Event {
                at: now(),
                kind,
                path: Some(path.clone()),
            };
            if let Err(e) = self.store.append_event(&ev) {
                warn!(
                    event = "store.append_event_failed",
                    error = %e,
                    "failed to persist event"
                );
            }
            self.event_bus.emit_event(ev).await;
            // Incrementally refresh the cached index + health for this one path
            // instead of waiting for the next 30s full refresh.
            self.index_one_path(&path).await;
        }
    }

    /// Re-index a single path and apply the result to the cached module list and
    /// project health. Used by the notify watcher for incremental updates; the
    /// 30s `refresh_index` tick remains the full-refresh safety net.
    async fn index_one_path(self: &Arc<Self>, path: &Path) {
        let indexer = crate::indexer::Indexer::new(
            self.config.project_root.clone(),
            self.config.ignore_patterns.clone(),
        );
        match indexer.index_file(path) {
            Ok(Some(m)) => {
                let mut stored = self.modules.write().await;
                *stored = upsert_module(std::mem::take(&mut *stored), m);
                let snapshot = stored.clone();
                drop(stored);
                let mut health = self.project_health.write().await;
                *health = recompute_health(&snapshot, &health);
                let snap = health.clone();
                drop(health);
                let _ = self.store.append_health(&snap);
            }
            Ok(None) => {
                // Empty/unsupported/deleted: if it was tracked, drop it.
                let mut stored = self.modules.write().await;
                let before = stored.len();
                let rel = path.strip_prefix(&self.config.project_root).unwrap_or(path);
                *stored = remove_module(std::mem::take(&mut *stored), rel);
                if stored.len() != before {
                    let snapshot = stored.clone();
                    drop(stored);
                    let mut health = self.project_health.write().await;
                    *health = recompute_health(&snapshot, &health);
                    let snap = health.clone();
                    drop(health);
                    let _ = self.store.append_health(&snap);
                }
            }
            Err(e) => {
                warn!(
                    event = "index.incremental_failed",
                    path = %path.display(),
                    error = %e,
                    "incremental reindex failed"
                );
            }
        }
    }

    fn is_ignored(&self, path: &Path) -> bool {
        let rel = path.strip_prefix(&self.config.project_root).unwrap_or(path);
        let rel_str = rel.to_string_lossy();
        for pat in &self.config.ignore_patterns {
            if crate::indexer::Indexer::glob_match(&rel_str, pat) {
                return true;
            }
        }
        false
    }

    #[tracing::instrument(skip(self))]
    async fn refresh_index(self: &Arc<Self>) -> Result<()> {
        let indexer = crate::indexer::Indexer::new(
            self.config.project_root.clone(),
            self.config.ignore_patterns.clone(),
        );
        let modules = indexer.index()?;
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

        let _ = self.store.append_health(&health_snapshot);

        let mut stored = self.modules.write().await;
        *stored = modules;

        info!(
            event = "index.refresh",
            total, healthy, warning, critical, score, "index refreshed"
        );
        Ok(())
    }

    #[tracing::instrument(skip(self), fields(candidate = tracing::field::Empty))]
    async fn process_queue(self: &Arc<Self>) -> Result<()> {
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
            let _ = self.store.append_proposal(&persisted);

            // Clean up scratch.
            let _ = tokio::fs::remove_dir_all(&scratch).await;
        }
        Ok(())
    }

    async fn prepare_scratch(&self, output: &refactor::RefactorOutput) -> Result<PathBuf> {
        let root = scratch::temp_dir("fract")?;
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
    async fn attempt_merges(self: &Arc<Self>) -> Result<()> {
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

            let _ = self.store.append_proposal(&proposal);
            let health_snapshot = self.project_health.read().await.clone();
            let _ = self.store.append_health(&health_snapshot);
        }
        Ok(())
    }

    pub async fn modules(&self) -> Vec<Module> {
        self.modules.read().await.clone()
    }

    pub async fn project_health(&self) -> ProjectHealth {
        self.project_health.read().await.clone()
    }

    pub async fn proposals(&self) -> Vec<Proposal> {
        self.queue.proposals().await
    }

    pub async fn events(&self, n: usize) -> Vec<Event> {
        self.event_bus.recent(n).await
    }

    /// Run a single deterministic pass: reindex the project, then emit a
    /// proposal for every module at or above the entropy threshold. Unlike the
    /// background `process_queue` loop this performs no shell-out and touches no
    /// git repository, so it is safe to drive from tests and offline hosts.
    #[tracing::instrument(skip(self))]
    pub async fn scan(self: &Arc<Self>) -> Result<()> {
        self.refresh_index().await?;
        self.detect_proposals().await?;
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
            let _ = self.store.append_proposal(&proposal);
            produced.push(proposal);
        }
        Ok(produced)
    }
}

fn build_engine(llm: &crate::config::LlmConfig) -> Arc<dyn refactor::RefactorEngine> {
    if llm.provider.eq_ignore_ascii_case("mock") || llm.provider.is_empty() {
        return Arc::new(refactor::MockRefactorEngine);
    }
    let endpoint = match llm.endpoint.clone() {
        Some(e) => e,
        None => {
            warn!(event = "engine.config", provider = %llm.provider, "no llm.endpoint set; defaulting to http://127.0.0.1:11434/v1");
            "http://127.0.0.1:11434/v1".to_string()
        }
    };
    if endpoint.starts_with("https://") {
        warn!(event = "engine.config", %endpoint, "https endpoint configured; fract's engine will refuse to connect (terminate TLS locally and use http://)");
    }
    info!(event = "engine.select", provider = %llm.provider, %endpoint, model = %llm.model, "selecting HTTP refactor engine");
    Arc::new(crate::engine_http::HttpRefactorEngine::new(
        endpoint,
        llm.model.clone(),
        llm.api_key.clone(),
        llm.max_tokens,
    ))
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

fn recompute_health(modules: &[Module], prev: &ProjectHealth) -> ProjectHealth {
    let (healthy, warning, critical) =
        modules
            .iter()
            .fold((0, 0, 0), |(h, w, c), m| match m.health {
                Health::Excellent | Health::Healthy => (h + 1, w, c),
                Health::Warning => (h, w + 1, c),
                Health::Critical => (h, w, c + 1),
            });
    let total = modules.len();
    let score = if total == 0 {
        100.0
    } else {
        (healthy as f64 * 1.0 + warning as f64 * 0.6 + critical as f64 * 0.2) / total as f64 * 100.0
    };
    let mut trend = prev.entropy_trend.clone();
    trend.push((now(), avg_entropy(modules)));
    if trend.len() > 100 {
        trend.remove(0);
    }
    ProjectHealth {
        score,
        total_modules: total,
        healthy,
        warning,
        critical,
        entropy_trend: trend,
        refactors_today: prev.refactors_today.clone(),
    }
}

/// Replace the module with the same path, or append. Returns the updated Vec.
fn upsert_module(mut modules: Vec<Module>, m: Module) -> Vec<Module> {
    if let Some(slot) = modules.iter_mut().find(|x| x.path == m.path) {
        *slot = m;
    } else {
        modules.push(m);
    }
    modules
}

fn remove_module(modules: Vec<Module>, path: &Path) -> Vec<Module> {
    modules.into_iter().filter(|x| x.path != path).collect()
}

fn avg_entropy(modules: &[Module]) -> f64 {
    if modules.is_empty() {
        return 0.0;
    }
    modules.iter().map(|m| m.entropy).sum::<f64>() / modules.len() as f64
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
    use crate::Language;

    fn module(path: &str, entropy: f64) -> Module {
        Module {
            path: PathBuf::from(path),
            language: Language::Rust,
            lines: 100,
            functions: 20,
            cyclomatic_complexity: 1,
            public_api_size: 0,
            fan_out: 0,
            fan_in: 0,
            duplicates: 0,
            edit_frequency: 0.0,
            confidence: None,
            churn: 0,
            test_coverage: 0.0,
            entropy,
            health: Health::from_entropy(entropy),
            last_modified: now(),
        }
    }

    fn empty_health() -> ProjectHealth {
        ProjectHealth {
            score: 0.0,
            total_modules: 0,
            healthy: 0,
            warning: 0,
            critical: 0,
            entropy_trend: Vec::new(),
            refactors_today: RefactorStats::default(),
        }
    }

    #[test]
    fn upsert_module_replaces_by_path() {
        let a = module("src/a.rs", 0.50);
        let updated = module("src/a.rs", 0.90);
        let out = upsert_module(vec![a], updated);
        assert_eq!(out.len(), 1);
        assert!((out[0].entropy - 0.90).abs() < 1e-9);
    }

    #[test]
    fn upsert_module_appends_new() {
        let a = module("src/a.rs", 0.50);
        let b = module("src/b.rs", 0.60);
        let out = upsert_module(vec![a], b);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn remove_module_drops_path() {
        let a = module("src/a.rs", 0.50);
        let b = module("src/b.rs", 0.60);
        let out = remove_module(vec![a, b], Path::new("src/a.rs"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, PathBuf::from("src/b.rs"));
    }

    #[test]
    fn recompute_health_score_formula() {
        // 1 healthy (0.50) + 1 warning (0.70) + 1 critical (0.90) -> 60.0
        let modules = vec![
            module("src/h.rs", 0.50),
            module("src/w.rs", 0.70),
            module("src/c.rs", 0.90),
        ];
        let h = recompute_health(&modules, &empty_health());
        assert_eq!(h.total_modules, 3);
        assert_eq!(h.healthy, 1);
        assert_eq!(h.warning, 1);
        assert_eq!(h.critical, 1);
        assert!((h.score - 60.0).abs() < 1e-9, "score was {}", h.score);
        // Trend gains exactly one entry; refactors_today preserved (default).
        assert_eq!(h.entropy_trend.len(), 1);
        assert_eq!(h.refactors_today.completed, 0);
    }
}
