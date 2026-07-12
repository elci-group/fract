//! Autonomous maintenance daemon: watches the project tree, keeps a live
//! module index and health model, and drives the refactor/validate/merge
//! pipeline. Incremental notify reindexing lives in `notify`; the
//! refactor/validate/merge pipeline lives in `pipeline`.

mod notify;
mod pipeline;

use crate::config::Config;
use crate::error::Result;
use crate::events::EventBus;
use crate::queue::RefactorQueue;
use crate::store::Store;
use crate::{refactor, Event, Module, ProjectHealth, Proposal, RefactorStats};
use ::notify::{
    Config as NotifyConfig, Event as NotifyEvent, RecommendedWatcher, RecursiveMode, Watcher,
};
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
    /// Construct a daemon from a configuration.
    #[must_use]
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

    /// Access the event bus.
    #[must_use]
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Access the daemon configuration.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Load persisted proposals, events, and health from the journal and seed
    /// the in-memory buses. Infallible: malformed journal lines are skipped.
    async fn restore_from_store(self: &Arc<Self>) {
        let loaded = self.store.load_async().await;
        self.event_bus.restore(loaded.events).await;
        self.queue.restore(loaded.proposals).await;
        if let Some(h) = loaded.health {
            *self.project_health.write().await = h;
        }
    }

    /// Start the daemon's background tasks (file watcher, periodic reindex,
    /// queue processor, merge watcher).
    ///
    /// # Errors
    /// Returns an error if the initial index refresh fails, or if the
    /// filesystem watcher cannot be created or attached to the watch root.
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
        let (tx, mut rx) = tokio::sync::mpsc::channel::<::notify::Result<NotifyEvent>>(256);

        let mut watcher: RecommendedWatcher = Watcher::new(
            move |res| {
                if tx.blocking_send(res).is_err() {
                    warn!(
                        event = "notify.channel_full",
                        "notify channel full or closed; dropping event"
                    );
                }
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
    ///
    /// # Errors
    /// Returns an error if the index refresh or proposal detection fails.
    #[tracing::instrument(skip(self))]
    pub async fn scan(self: &Arc<Self>) -> Result<()> {
        self.refresh_index().await?;
        self.detect_proposals().await?;
        Ok(())
    }
}

fn build_engine(llm: &crate::config::LlmConfig) -> Arc<dyn refactor::RefactorEngine> {
    if llm.provider.eq_ignore_ascii_case("mock") || llm.provider.is_empty() {
        return Arc::new(refactor::MockRefactorEngine);
    }
    let endpoint = if let Some(e) = llm.endpoint.clone() {
        e
    } else {
        warn!(event = "engine.config", provider = %llm.provider, "no llm.endpoint set; defaulting to http://127.0.0.1:11434/v1");
        "http://127.0.0.1:11434/v1".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn temp_dir() -> PathBuf {
        // Rust runs the test binary's tests in parallel threads within one
        // process, so a pid-only name would collide. Mix in a per-call counter.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("fract-daemon-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn proposal_fixture(id: &str) -> Proposal {
        Proposal {
            id: id.to_string(),
            created_at: SystemTime::UNIX_EPOCH,
            module: PathBuf::from("src/lib.rs"),
            kind: crate::RefactorKind::ExtractFunction,
            confidence: 0.9,
            status: crate::ProposalStatus::Accepted,
            validation: None,
            diff_summary: crate::DiffSummary::default(),
            migration_notes: Vec::new(),
            changed_files: Vec::new(),
            pr_body: None,
            timeline: Vec::new(),
        }
    }

    fn event_fixture() -> Event {
        Event {
            at: SystemTime::UNIX_EPOCH,
            kind: crate::EventKind::FileSaved,
            path: Some(PathBuf::from("src/lib.rs")),
        }
    }

    fn health_fixture(score: f64) -> ProjectHealth {
        ProjectHealth {
            score,
            total_modules: 3,
            healthy: 2,
            warning: 1,
            critical: 0,
            entropy_trend: Vec::new(),
            refactors_today: RefactorStats::default(),
        }
    }

    #[tokio::test]
    async fn restore_from_store_seeds_queue_events_and_health() {
        let dir = temp_dir();
        let store = Store::open(&dir);
        store.append_proposal(&proposal_fixture("p1")).unwrap();
        store.append_event(&event_fixture()).unwrap();
        store.append_health(&health_fixture(88.0)).unwrap();

        let daemon = Arc::new(Daemon::new(Config::default_for(dir.clone())));
        daemon.restore_from_store().await;

        let proposals = daemon.proposals().await;
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].id, "p1");
        assert_eq!(daemon.events(10).await.len(), 1);
        assert!((daemon.project_health().await.score - 88.0).abs() < f64::EPSILON);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_engine_uses_mock_for_mock_or_empty_provider() {
        for provider in ["mock", "MOCK", ""] {
            let llm = crate::config::LlmConfig {
                provider: provider.to_string(),
                ..Default::default()
            };
            let _engine = build_engine(&llm);
        }
    }

    #[test]
    fn build_engine_http_branch_handles_endpoint_variants() {
        for endpoint in [
            None,
            Some("https://example.test/v1".to_string()),
            Some("http://127.0.0.1:11434/v1".to_string()),
        ] {
            let llm = crate::config::LlmConfig {
                provider: "openai".to_string(),
                model: "m".to_string(),
                endpoint,
                api_key: None,
                max_tokens: 1_000,
            };
            let _engine = build_engine(&llm);
        }
    }
}
