use crate::time::now;
use crate::{Event, EventKind};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Central event bus for repository and filesystem events.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
    history: Arc<RwLock<Vec<Event>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            tx,
            history: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    pub async fn emit(&self, kind: EventKind, path: Option<PathBuf>) {
        let event = Event {
            at: now(),
            kind,
            path,
        };
        let _ = self.tx.send(event.clone());
        let mut history = self.history.write().await;
        history.push(event);
        if history.len() > 1000 {
            history.remove(0);
        }
    }

    pub async fn recent(&self, n: usize) -> Vec<Event> {
        let history = self.history.read().await;
        history.iter().rev().take(n).cloned().collect()
    }
}
