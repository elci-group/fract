//! Journal codec for `ProjectHealth` snapshots: encode/decode health
//! records (score, health buckets, entropy trend, refactor stats) as
//! `json::Value`.

use crate::json::Value;
use crate::time::{parse_rfc3339, to_rfc3339, Timestamp};
use crate::{ProjectHealth, RefactorStats};

use super::VERSION;

pub(crate) fn encode_health(h: &ProjectHealth) -> Value {
    let mut v = Value::object();
    v.insert("v", VERSION);
    v.insert("type", "health");
    v.insert("score", Value::Number(h.score));
    v.insert("total", h.total_modules);
    v.insert("healthy", h.healthy);
    v.insert("warning", h.warning);
    v.insert("critical", h.critical);
    let trend: Vec<Value> = h
        .entropy_trend
        .iter()
        .map(|(t, val)| Value::Array(vec![Value::String(to_rfc3339(*t)), Value::Number(*val)]))
        .collect();
    v.insert("trend", Value::Array(trend));
    let mut stats = Value::object();
    stats.insert("completed", h.refactors_today.completed);
    stats.insert("pending", h.refactors_today.pending);
    stats.insert("failed", h.refactors_today.failed);
    stats.insert("loc_removed", h.refactors_today.loc_removed);
    stats.insert(
        "complexity_reduced",
        Value::Number(h.refactors_today.complexity_reduced),
    );
    v.insert("stats", stats);
    v
}

pub(crate) fn decode_health(v: &Value) -> Option<ProjectHealth> {
    let score = v.get("score")?.as_f64()?;
    let total_modules = super::req_usize(v, "total")?;
    let healthy = super::req_usize(v, "healthy")?;
    let warning = super::req_usize(v, "warning")?;
    let critical = super::req_usize(v, "critical")?;
    let trend_arr = v.get("trend")?.as_array()?;
    let mut entropy_trend: Vec<(Timestamp, f64)> = Vec::with_capacity(trend_arr.len());
    for entry in trend_arr {
        let pair = entry.as_array()?;
        if pair.len() != 2 {
            return None;
        }
        let ts = parse_rfc3339(pair[0].as_str()?).ok()?;
        let val = pair[1].as_f64()?;
        entropy_trend.push((ts, val));
    }
    let stats = v.get("stats")?;
    let refactors_today = RefactorStats {
        completed: super::req_usize(stats, "completed")?,
        pending: super::req_usize(stats, "pending")?,
        failed: super::req_usize(stats, "failed")?,
        loc_removed: super::req_usize(stats, "loc_removed")?,
        complexity_reduced: stats.get("complexity_reduced")?.as_f64()?,
    };
    Some(ProjectHealth {
        score,
        total_modules,
        healthy,
        warning,
        critical,
        entropy_trend,
        refactors_today,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn fixed_ts(secs: u64) -> Timestamp {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn temp_dir() -> PathBuf {
        // Rust runs the test binary's tests in parallel threads within one
        // process, so a pid-only name would collide. Mix in a per-module prefix
        // and a per-call counter (each test module has its own static counter).
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "fract-store-health-test-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_health() -> ProjectHealth {
        ProjectHealth {
            score: 73.5,
            total_modules: 12,
            healthy: 7,
            warning: 3,
            critical: 2,
            entropy_trend: vec![
                (fixed_ts(1_700_000_000), 0.42),
                (fixed_ts(1_700_000_100), 0.55),
            ],
            refactors_today: RefactorStats {
                completed: 4,
                pending: 1,
                failed: 2,
                loc_removed: 30,
                complexity_reduced: 0.5,
            },
        }
    }

    #[test]
    fn roundtrip_health() {
        let dir = temp_dir();
        let store = Store::open(&dir);
        let health = sample_health();
        store.append_health(&health).unwrap();

        let loaded = store.load();
        let decoded = loaded.health.expect("health record present");
        assert_eq!(
            crate::json::to_value(&decoded),
            crate::json::to_value(&health)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
