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
                notify::EventKind::Modify(_) => EventKind::FileSaved,
                notify::EventKind::Create(_) => EventKind::FileSaved,
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
                let mut health = self.project_health.write().await;
                *health = recompute_health(&snapshot, &health);
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
                    if let Err(e) = self.store.append_health_async(&snap).await {
                        warn!(
                            event = "store.append_failed",
                            error = %e,
                            "failed to persist health snapshot"
                        );
                    }
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
}
