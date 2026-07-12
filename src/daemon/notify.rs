//! Incremental reindexing: maps filesystem watch events to single-path
//! index updates (upsert/remove), event persistence and broadcast, and
//! project-health recomputation.

use super::Daemon;
use crate::time::now;
use crate::{Event, EventKind, Health, Module, ProjectHealth};
use notify::Event as NotifyEvent;
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

impl Daemon {
    pub(crate) async fn handle_notify_event(self: &Arc<Self>, event: NotifyEvent) {
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
                notify::EventKind::Modify(_) | notify::EventKind::Create(_) => EventKind::FileSaved,
                _ => EventKind::EditorHeartbeat,
            };
            let ev = Event {
                at: now(),
                kind,
                path: Some(path.clone()),
            };
            if let Err(e) = self.store.append_event_async(&ev).await {
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
                self.persist_health(&snapshot).await;
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
                    self.persist_health(&snapshot).await;
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

    /// Recompute project health from a module snapshot and persist it.
    async fn persist_health(&self, modules: &[Module]) {
        let mut health = self.project_health.write().await;
        *health = recompute_health(modules, &health);
        let snap = health.clone();
        drop(health);
        if let Err(e) = self.store.append_health_async(&snap).await {
            warn!(
                event = "store.append_failed",
                error = %e,
                "failed to persist health snapshot"
            );
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
}

pub(crate) fn recompute_health(modules: &[Module], prev: &ProjectHealth) -> ProjectHealth {
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
pub(crate) fn upsert_module(mut modules: Vec<Module>, m: Module) -> Vec<Module> {
    if let Some(slot) = modules.iter_mut().find(|x| x.path == m.path) {
        *slot = m;
    } else {
        modules.push(m);
    }
    modules
}

pub(crate) fn remove_module(modules: Vec<Module>, path: &Path) -> Vec<Module> {
    modules.into_iter().filter(|x| x.path != path).collect()
}

pub(crate) fn avg_entropy(modules: &[Module]) -> f64 {
    if modules.is_empty() {
        return 0.0;
    }
    modules.iter().map(|m| m.entropy).sum::<f64>() / modules.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, RefactorStats};
    use std::path::PathBuf;

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

    #[test]
    fn recompute_health_caps_trend_at_100_entries() {
        let mut prev = empty_health();
        prev.entropy_trend = (0..100)
            .map(|i| {
                (
                    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(i),
                    0.5,
                )
            })
            .collect();
        let h = recompute_health(&[module("src/a.rs", 0.50)], &prev);
        assert_eq!(h.entropy_trend.len(), 100);
    }

    #[test]
    fn avg_entropy_empty_module_set_is_zero() {
        assert!((avg_entropy(&[]) - 0.0).abs() < f64::EPSILON);
        let m = [module("src/a.rs", 0.4), module("src/b.rs", 0.8)];
        assert!((avg_entropy(&m) - 0.6).abs() < 1e-9);
    }

    fn temp_dir() -> PathBuf {
        // Rust runs the test binary's tests in parallel threads within one
        // process, so a pid-only name would collide. Mix in a per-call counter.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("fract-notify-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn daemon_for(root: &std::path::Path) -> Arc<Daemon> {
        Arc::new(Daemon::new(crate::config::Config::default_for(
            root.to_path_buf(),
        )))
    }

    #[test]
    fn is_ignored_matches_configured_patterns() {
        let root = temp_dir();
        let daemon = daemon_for(&root);
        assert!(daemon.is_ignored(&root.join("target/out.rs")));
        assert!(daemon.is_ignored(&root.join(".git/config")));
        assert!(!daemon.is_ignored(&root.join("src/lib.rs")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn index_one_path_upserts_then_removes_module() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/a.rs");
        std::fs::write(&file, "pub fn a() -> i32 { 1 }\n").unwrap();

        let daemon = daemon_for(&root);
        daemon.index_one_path(&file).await;
        assert_eq!(daemon.modules().await.len(), 1);
        assert_eq!(daemon.project_health().await.total_modules, 1);

        // Deleting the file drops the cached module on the next event.
        std::fs::remove_file(&file).unwrap();
        daemon.index_one_path(&file).await;
        assert!(daemon.modules().await.is_empty());
        assert_eq!(daemon.project_health().await.total_modules, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn handle_create_event_emits_file_saved_and_indexes() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/new.rs");
        std::fs::write(&file, "pub fn n() -> i32 { 1 }\n").unwrap();

        let daemon = daemon_for(&root);
        let mut rx = daemon.event_bus().subscribe();
        daemon
            .handle_notify_event(NotifyEvent {
                kind: notify::EventKind::Create(notify::event::CreateKind::File),
                paths: vec![file],
                attrs: notify::event::EventAttributes::new(),
            })
            .await;

        let got = rx.recv().await.unwrap();
        assert!(matches!(got.kind, EventKind::FileSaved));
        assert_eq!(daemon.modules().await.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn handle_remove_event_drops_module_without_save_event() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/gone.rs");
        std::fs::write(&file, "pub fn g() -> i32 { 1 }\n").unwrap();

        let daemon = daemon_for(&root);
        daemon.index_one_path(&file).await;
        assert_eq!(daemon.modules().await.len(), 1);

        std::fs::remove_file(&file).unwrap();
        let mut rx = daemon.event_bus().subscribe();
        daemon
            .handle_notify_event(NotifyEvent {
                kind: notify::EventKind::Remove(notify::event::RemoveKind::File),
                paths: vec![file],
                attrs: notify::event::EventAttributes::new(),
            })
            .await;

        // Deletes must not fan out a save event.
        assert!(rx.try_recv().is_err());
        assert!(daemon.modules().await.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn handle_event_skips_ignored_paths_and_directories() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join("src")).unwrap();

        let daemon = daemon_for(&root);
        let mut rx = daemon.event_bus().subscribe();
        for event in [
            NotifyEvent {
                kind: notify::EventKind::Create(notify::event::CreateKind::File),
                paths: vec![root.join("target/generated.rs")],
                attrs: notify::event::EventAttributes::new(),
            },
            NotifyEvent {
                kind: notify::EventKind::Modify(notify::event::ModifyKind::Any),
                paths: vec![root.join("src")],
                attrs: notify::event::EventAttributes::new(),
            },
        ] {
            daemon.handle_notify_event(event).await;
        }

        assert!(rx.try_recv().is_err(), "no events for ignored/dir paths");
        assert!(daemon.modules().await.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
