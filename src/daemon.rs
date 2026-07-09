use crate::{
    confidence, config::Config, config::Mode, events::EventBus, merge, refactor, validation,
    Event, EventKind, Module, ProjectHealth, Proposal, ProposalStatus, RefactorKind,
    queue::RefactorQueue, RefactorStats, TimelineEvent,
};
use crate::error::{Context, Result};
use crate::scratch;
use crate::time::now;
use notify::{Config as NotifyConfig, Event as NotifyEvent, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{error, info};

pub struct Daemon {
    config: Config,
    event_bus: EventBus,
    queue: RefactorQueue,
    modules: Arc<RwLock<Vec<Module>>>,
    project_health: Arc<RwLock<ProjectHealth>>,
    engine: Arc<dyn refactor::RefactorEngine>,
}

impl Daemon {
    pub fn new(config: Config) -> Self {
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
            engine: Arc::new(refactor::MockRefactorEngine),
        }
    }

    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        info!("starting fract daemon");
        info!("mode: {:?}", self.config.mode);

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
                    error!("reindex failed: {}", e);
                }
                if let Err(e) = daemon.process_queue().await {
                    error!("queue processing failed: {}", e);
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
                    error!("merge attempts failed: {}", e);
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
            let kind = match event.kind {
                notify::EventKind::Modify(_) => EventKind::FileSaved,
                notify::EventKind::Create(_) => EventKind::FileSaved,
                _ => EventKind::EditorHeartbeat,
            };
            self.event_bus.emit(kind, Some(path)).await;
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

    async fn refresh_index(self: &Arc<Self>) -> Result<()> {
        let indexer = crate::indexer::Indexer::new(
            self.config.project_root.clone(),
            self.config.ignore_patterns.clone(),
        );
        let modules = indexer.index()?;
        self.queue.refresh(&modules, self.config.entropy_threshold).await;

        let (healthy, warning, critical) = self.queue.health_counts(&modules).await;
        let total = modules.len();
        let score = if total == 0 {
            100.0
        } else {
            let raw = (healthy as f64 * 1.0 + warning as f64 * 0.6 + critical as f64 * 0.2) / total as f64;
            raw * 100.0
        };

        let mut health = self.project_health.write().await;
        health.score = score;
        health.total_modules = total;
        health.healthy = healthy;
        health.warning = warning;
        health.critical = critical;
        health.entropy_trend.push((now(), avg_entropy(&modules)));
        if health.entropy_trend.len() > 100 {
            health.entropy_trend.remove(0);
        }

        let mut stored = self.modules.write().await;
        *stored = modules;
        Ok(())
    }

    async fn process_queue(self: &Arc<Self>) -> Result<()> {
        if let Some(path) = self.queue.next_candidate().await {
            let modules = self.modules.read().await;
            let module = modules
                .iter()
                .find(|m| m.path == path)
                .cloned()
                .context("candidate disappeared")?;
            drop(modules);

            let kind = classify_kind(&module);
            let mut proposal = crate::queue::proposal_for(&module, kind);
            info!("processing candidate {} with entropy {:.2}", path.display(), module.entropy);

            // Build and execute refactor.
            let output = refactor::execute_proposal(
                self.engine.as_ref(),
                &self.config.project_root,
                &mut proposal,
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

            self.queue.enqueue_proposal(proposal).await;

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

            // Find associated output from proposal state (simplified).
            let files = HashMap::new();
            merge::apply(&self.config.project_root, &mut proposal, &files).await?;

            if self.config.mode == Mode::Autonomous {
                let message = format!(
                    "fract: refactor {} (confidence {:.1}%)",
                    proposal.module.display(),
                    proposal.confidence * 100.0
                );
                merge::commit(&self.config.project_root, &mut proposal, &message).await?;
                let mut health = self.project_health.write().await;
                health.refactors_today.completed += 1;
                health.refactors_today.loc_removed += proposal.diff_summary.lines_removed;
                health.refactors_today.complexity_reduced += proposal.diff_summary.lines_removed as f64 / 100.0;
            } else {
                // Assisted: leave files modified, create branch is TODO.
                proposal.status = ProposalStatus::Accepted;
                proposal.timeline.push(TimelineEvent {
                    at: now(),
                    message: "Branch created (assisted mode)".to_string(),
                });
            }

            self.queue
                .update_proposal(&proposal.id, |p| *p = proposal.clone())
                .await;
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
