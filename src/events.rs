//! Broadcast event bus with a bounded in-memory history (last 1000
//! events), fanning repository/filesystem events out to subscribers and
//! the dashboard. History can be seeded from the journal on startup
//! without re-broadcasting.

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
    /// Create an empty event bus.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe to future events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    pub async fn emit(&self, kind: EventKind, path: Option<PathBuf>) {
        let event = Event {
            at: now(),
            kind,
            path,
        };
        self.emit_event(event).await;
    }

    /// Emit a pre-built event: broadcast it and record it in history.
    pub async fn emit_event(&self, event: Event) {
        let _ = self.tx.send(event.clone());
        let mut history = self.history.write().await;
        history.push(event);
        if history.len() > 1000 {
            history.remove(0);
        }
    }

    /// Seed history from the journal on startup. Does NOT broadcast — restored
    /// events must not fan out to live subscribers — and caps at 1000 entries.
    pub async fn restore(&self, events: Vec<Event>) {
        let mut history = self.history.write().await;
        history.clear();
        let start = events.len().saturating_sub(1000);
        history.extend(events.into_iter().skip(start));
    }

    pub async fn recent(&self, n: usize) -> Vec<Event> {
        let history = self.history.read().await;
        history.iter().rev().take(n).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn event_at(secs: u64) -> Event {
        Event {
            at: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
            kind: EventKind::FileSaved,
            path: Some(PathBuf::from(format!("src/f{secs}.rs"))),
        }
    }

    #[tokio::test]
    async fn subscribe_receives_emitted_event() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.emit(EventKind::EditorHeartbeat, None).await;
        let got = rx.recv().await.unwrap();
        assert!(matches!(got.kind, EventKind::EditorHeartbeat));
        assert!(got.path.is_none());
    }

    #[tokio::test]
    async fn recent_returns_newest_first() {
        let bus = EventBus::new();
        for i in 0..3 {
            bus.emit_event(event_at(i)).await;
        }
        let recent = bus.recent(2).await;
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].path, Some(PathBuf::from("src/f2.rs")));
        assert_eq!(recent[1].path, Some(PathBuf::from("src/f1.rs")));
    }

    #[tokio::test]
    async fn history_evicts_oldest_beyond_1000_entries() {
        let bus = EventBus::new();
        for i in 0..1_001 {
            bus.emit_event(event_at(i)).await;
        }
        let all = bus.recent(2_000).await;
        assert_eq!(all.len(), 1_000);
        // Newest first; the oldest retained entry is event #1 (event #0 evicted).
        assert_eq!(all[0].path, Some(PathBuf::from("src/f1000.rs")));
        assert_eq!(all[999].path, Some(PathBuf::from("src/f1.rs")));
    }

    #[tokio::test]
    async fn restore_seeds_history_without_broadcasting() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.restore(vec![event_at(0), event_at(1), event_at(2)])
            .await;
        // Restored events must not fan out to live subscribers.
        assert!(rx.try_recv().is_err());
        let recent = bus.recent(10).await;
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].path, Some(PathBuf::from("src/f2.rs")));
    }

    #[tokio::test]
    async fn restore_caps_history_at_1000_entries() {
        let bus = EventBus::new();
        let events: Vec<Event> = (0..1_005).map(event_at).collect();
        bus.restore(events).await;
        let all = bus.recent(2_000).await;
        assert_eq!(all.len(), 1_000);
        // The five oldest events (0..=4) were dropped by the cap.
        assert_eq!(all[999].path, Some(PathBuf::from("src/f5.rs")));
    }

    #[tokio::test]
    async fn restore_replaces_previous_history() {
        let bus = EventBus::new();
        bus.emit_event(event_at(0)).await;
        bus.restore(vec![event_at(9)]).await;
        let recent = bus.recent(10).await;
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].path, Some(PathBuf::from("src/f9.rs")));
    }

    #[tokio::test]
    async fn emit_event_broadcasts_and_records_prebuilt_event() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.emit_event(event_at(42)).await;
        let got = rx.recv().await.unwrap();
        assert_eq!(got.path, Some(PathBuf::from("src/f42.rs")));
        let recent = bus.recent(1).await;
        assert_eq!(recent[0].path, Some(PathBuf::from("src/f42.rs")));
    }
}
