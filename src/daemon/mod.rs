//! Autonomous maintenance daemon: watches the project tree, keeps a live
//! module index and health model, and drives the refactor/validate/merge
//! pipeline. Incremental notify reindexing lives in [`notify`]; the
//! refactor/validate/merge pipeline lives in [`pipeline`].

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
        let loaded = self.store.load_async().await;
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
