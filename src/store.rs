//! Zero-dependency append-only JSONL journal. Persists proposals
//! (identity + lifecycle, NOT heavy changed_files payloads — those are
//! re-derived on demand), recent events, and the health trend so the
//! dashboard/queue survive a restart. Encoded with the in-tree `json::Value`;
//! `serde_json` is amber-forbidden.

use crate::error::Result;
use crate::json::Value;
use crate::time::{parse_rfc3339, to_rfc3339, Timestamp};
use crate::{
    DiffSummary, Event, EventKind, ProjectHealth, Proposal, ProposalStatus, RefactorKind,
    RefactorStats, TimelineEvent,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::warn;

const VERSION: i32 = 1;

/// Append-only JSONL journal rooted at `<root>/.fract/state.jsonl`.
#[derive(Clone)]
pub struct Store {
    path: Arc<PathBuf>,
}

/// State reconstructed from the journal on startup.
pub struct Loaded {
    pub proposals: Vec<Proposal>,
    pub events: Vec<Event>,
    /// Last health record in the journal wins.
    pub health: Option<ProjectHealth>,
}

impl Store {
    /// Open (creating) the `.fract` directory under `root`. No records are read
    /// or written here; IO happens per `append_*`/`load` call.
    pub fn open(root: &Path) -> Self {
        let dir = root.join(".fract");
        let _ = std::fs::create_dir_all(&dir);
        Self {
            path: Arc::new(dir.join("state.jsonl")),
        }
    }

    /// Read and decode the journal. Any malformed line is skipped with a
    /// warning; corruption never aborts startup.
    pub fn load(&self) -> Loaded {
        let mut loaded = Loaded {
            proposals: Vec::new(),
            events: Vec::new(),
            health: None,
        };
        let text = match std::fs::read_to_string(self.path.as_path()) {
            Ok(t) => t,
            Err(_) => return loaded,
        };
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let line_no = i + 1;
            let value = match crate::json::parse(line) {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        event = "store.skip",
                        line = line_no,
                        error = %e,
                        "skipping malformed journal line"
                    );
                    continue;
                }
            };
            let kind = match value.get("type").and_then(|t| t.as_str()) {
                Some(t) => t,
                None => {
                    warn!(
                        event = "store.skip",
                        line = line_no,
                        "journal line missing type"
                    );
                    continue;
                }
            };
            match kind {
                "health" => match decode_health(&value) {
                    Some(h) => loaded.health = Some(h),
                    None => warn!(
                        event = "store.skip",
                        line = line_no,
                        "malformed health record"
                    ),
                },
                "event" => match decode_event(&value) {
                    Some(e) => loaded.events.push(e),
                    None => warn!(
                        event = "store.skip",
                        line = line_no,
                        "malformed event record"
                    ),
                },
                "proposal" => match decode_proposal(&value) {
                    Some(p) => loaded.proposals.push(p),
                    None => {
                        warn!(
                            event = "store.skip",
                            line = line_no,
                            "malformed proposal record"
                        )
                    }
                },
                other => warn!(
                    event = "store.skip",
                    line = line_no,
                    kind = other,
                    "unknown journal record type"
                ),
            }
        }
        loaded
    }

    pub fn append_proposal(&self, p: &Proposal) -> Result<()> {
        self.append(&encode_proposal(p))
    }

    pub fn append_event(&self, e: &Event) -> Result<()> {
        self.append(&encode_event(e))
    }

    pub fn append_health(&self, h: &ProjectHealth) -> Result<()> {
        self.append(&encode_health(h))
    }

    fn append(&self, v: &Value) -> Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path.as_path())?;
        writeln!(f, "{}", v.to_string())?;
        f.flush()?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

fn encode_health(h: &ProjectHealth) -> Value {
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

fn encode_event(e: &Event) -> Value {
    let mut v = Value::object();
    v.insert("v", VERSION);
    v.insert("type", "event");
    v.insert("at", Value::String(to_rfc3339(e.at)));
    let path = match &e.path {
        Some(p) => Value::String(p.to_string_lossy().into_owned()),
        None => Value::Null,
    };
    v.insert("path", path);
    match &e.kind {
        EventKind::FileSaved => {
            v.insert("kind", "FileSaved");
        }
        EventKind::GitCommit { sha } => {
            v.insert("kind", "GitCommit");
            v.insert("sha", Value::String(sha.clone()));
        }
        EventKind::BranchChanged { branch } => {
            v.insert("kind", "BranchChanged");
            v.insert("branch", Value::String(branch.clone()));
        }
        EventKind::BuildFailed { reason } => {
            v.insert("kind", "BuildFailed");
            v.insert("reason", Value::String(reason.clone()));
        }
        EventKind::TestFailed { reason } => {
            v.insert("kind", "TestFailed");
            v.insert("reason", Value::String(reason.clone()));
        }
        EventKind::EditorHeartbeat => {
            v.insert("kind", "EditorHeartbeat");
        }
    }
    v
}

fn encode_proposal(p: &Proposal) -> Value {
    let mut v = Value::object();
    v.insert("v", VERSION);
    v.insert("type", "proposal");
    v.insert("id", Value::String(p.id.clone()));
    v.insert("created_at", Value::String(to_rfc3339(p.created_at)));
    v.insert(
        "module",
        Value::String(p.module.to_string_lossy().into_owned()),
    );
    v.insert("kind", Value::String(p.kind.description().to_string()));
    v.insert("status", Value::String(p.status.to_string()));
    v.insert("confidence", Value::Number(p.confidence));
    let notes: Vec<Value> = p
        .migration_notes
        .iter()
        .map(|n| Value::String(n.clone()))
        .collect();
    v.insert("migration_notes", Value::Array(notes));
    let pr = match &p.pr_body {
        Some(s) => Value::String(s.clone()),
        None => Value::Null,
    };
    v.insert("pr_body", pr);
    let mut ds = Value::object();
    ds.insert("files_added", p.diff_summary.files_added);
    ds.insert("files_removed", p.diff_summary.files_removed);
    ds.insert("files_modified", p.diff_summary.files_modified);
    ds.insert("lines_added", p.diff_summary.lines_added);
    ds.insert("lines_removed", p.diff_summary.lines_removed);
    v.insert("diff_summary", ds);
    v
}

// ---------------------------------------------------------------------------
// Decoding — every required field must be present and well-typed, else the
// whole line is treated as malformed (returns None → caller skips + warns).
// ---------------------------------------------------------------------------

fn req_usize(obj: &Value, key: &str) -> Option<usize> {
    Some(obj.get(key)?.as_f64()? as usize)
}

fn decode_health(v: &Value) -> Option<ProjectHealth> {
    let score = v.get("score")?.as_f64()?;
    let total_modules = req_usize(v, "total")?;
    let healthy = req_usize(v, "healthy")?;
    let warning = req_usize(v, "warning")?;
    let critical = req_usize(v, "critical")?;
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
        completed: req_usize(stats, "completed")?,
        pending: req_usize(stats, "pending")?,
        failed: req_usize(stats, "failed")?,
        loc_removed: req_usize(stats, "loc_removed")?,
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

fn decode_event(v: &Value) -> Option<Event> {
    let at = parse_rfc3339(v.get("at")?.as_str()?).ok()?;
    let path = match v.get("path") {
        None | Some(Value::Null) => None,
        Some(other) => Some(PathBuf::from(other.as_str()?)),
    };
    let kind = match v.get("kind")?.as_str()? {
        "FileSaved" => EventKind::FileSaved,
        "GitCommit" => EventKind::GitCommit {
            sha: v.get("sha")?.as_str()?.to_string(),
        },
        "BranchChanged" => EventKind::BranchChanged {
            branch: v.get("branch")?.as_str()?.to_string(),
        },
        "BuildFailed" => EventKind::BuildFailed {
            reason: v.get("reason")?.as_str()?.to_string(),
        },
        "TestFailed" => EventKind::TestFailed {
            reason: v.get("reason")?.as_str()?.to_string(),
        },
        "EditorHeartbeat" => EventKind::EditorHeartbeat,
        _ => return None,
    };
    Some(Event { at, kind, path })
}

fn decode_proposal(v: &Value) -> Option<Proposal> {
    let id = v.get("id")?.as_str()?.to_string();
    let created_at = parse_rfc3339(v.get("created_at")?.as_str()?).ok()?;
    let module = PathBuf::from(v.get("module")?.as_str()?);
    let kind = RefactorKind::parse(v.get("kind")?.as_str()?)?;
    let status = ProposalStatus::parse(v.get("status")?.as_str()?)?;
    let confidence = v.get("confidence")?.as_f64()?;
    let notes_arr = v.get("migration_notes")?.as_array()?;
    let mut migration_notes = Vec::with_capacity(notes_arr.len());
    for n in notes_arr {
        migration_notes.push(n.as_str()?.to_string());
    }
    let pr_body = match v.get("pr_body") {
        None | Some(Value::Null) => None,
        Some(other) => Some(other.as_str()?.to_string()),
    };
    let ds = v.get("diff_summary")?;
    let diff_summary = DiffSummary {
        files_added: req_usize(ds, "files_added")?,
        files_removed: req_usize(ds, "files_removed")?,
        files_modified: req_usize(ds, "files_modified")?,
        lines_added: req_usize(ds, "lines_added")?,
        lines_removed: req_usize(ds, "lines_removed")?,
    };
    Some(Proposal {
        id,
        created_at,
        module,
        kind,
        confidence,
        status,
        validation: None,
        diff_summary,
        migration_notes,
        changed_files: Vec::new(),
        pr_body,
        timeline: vec![TimelineEvent {
            at: created_at,
            message: "Restored from journal".to_string(),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    fn fixed_ts(secs: u64) -> Timestamp {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn temp_dir() -> PathBuf {
        // Rust runs the test binary's tests in parallel threads within one
        // process, so a pid-only name would collide. Mix in a per-call counter.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("fract-store-test-{}-{n}", std::process::id()));
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

    fn sample_proposal() -> Proposal {
        Proposal {
            id: "prop-test-1".to_string(),
            created_at: fixed_ts(1_700_000_000),
            module: PathBuf::from("src/foo.rs"),
            kind: RefactorKind::SplitModule,
            confidence: 0.93,
            status: ProposalStatus::Accepted,
            validation: None,
            diff_summary: DiffSummary {
                files_added: 1,
                files_removed: 2,
                files_modified: 3,
                lines_added: 40,
                lines_removed: 50,
            },
            migration_notes: vec!["note a".to_string(), "note b".to_string()],
            changed_files: Vec::new(),
            pr_body: Some("PR body".to_string()),
            timeline: Vec::new(),
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

    #[test]
    fn roundtrip_proposal_projection() {
        let dir = temp_dir();
        let store = Store::open(&dir);
        let proposal = sample_proposal();
        store.append_proposal(&proposal).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.proposals.len(), 1);
        let p = &loaded.proposals[0];
        assert_eq!(p.id, proposal.id);
        assert_eq!(p.module, proposal.module);
        assert_eq!(p.kind, proposal.kind);
        assert_eq!(p.status, proposal.status);
        assert_eq!(p.confidence, proposal.confidence);
        assert_eq!(p.migration_notes, proposal.migration_notes);
        assert_eq!(p.pr_body, proposal.pr_body);
        assert_eq!(
            crate::json::to_value(&p.diff_summary),
            crate::json::to_value(&proposal.diff_summary)
        );
        assert!(p.changed_files.is_empty(), "changed_files not persisted");
        assert!(
            p.timeline
                .iter()
                .any(|t| t.message == "Restored from journal"),
            "restored timeline marker present"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_events_each_kind() {
        let kinds = [
            EventKind::FileSaved,
            EventKind::GitCommit {
                sha: "abc123".to_string(),
            },
            EventKind::BranchChanged {
                branch: "feature/x".to_string(),
            },
            EventKind::BuildFailed {
                reason: "compile error".to_string(),
            },
            EventKind::TestFailed {
                reason: "assertion failed".to_string(),
            },
            EventKind::EditorHeartbeat,
        ];
        for kind in kinds {
            let event = Event {
                at: fixed_ts(1_700_000_000),
                kind,
                path: Some(PathBuf::from("src/foo.rs")),
            };
            let encoded = encode_event(&event);
            let decoded = decode_event(&encoded).expect("event decodes");
            let re_encoded = encode_event(&decoded);
            assert_eq!(re_encoded, encoded, "event kind must roundtrip losslessly");
        }
    }

    #[test]
    fn load_skips_malformed_lines() {
        let dir = temp_dir();
        let store = Store::open(&dir);
        let proposal = sample_proposal();
        let good = encode_proposal(&proposal).to_string();
        let unknown = {
            let mut v = Value::object();
            v.insert("v", 1);
            v.insert("type", "mystery");
            v.to_string()
        };
        let contents = format!("{good}\nnot json\n{unknown}\n");
        std::fs::write(dir.join(".fract").join("state.jsonl"), contents).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.proposals.len(), 1, "only the valid proposal loads");
        assert!(loaded.events.is_empty());
        assert!(loaded.health.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_is_empty() {
        let dir = temp_dir();
        // Store::open creates the `.fract` directory but no state.jsonl yet,
        // so load() must return an empty snapshot rather than erroring.
        let store = Store::open(&dir);
        let loaded = store.load();
        assert!(loaded.proposals.is_empty());
        assert!(loaded.events.is_empty());
        assert!(loaded.health.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fract_journal_is_ignored_by_default_globs() {
        // Proves `.fract/state.jsonl` is excluded from both indexing and the
        // notify feedback loop by the default ignore patterns.
        let patterns = crate::config::default_ignore_patterns();
        assert!(
            patterns
                .iter()
                .any(|pat| crate::indexer::Indexer::glob_match(".fract/state.jsonl", pat)),
            "default ignore patterns must exclude .fract/state.jsonl; got {patterns:?}"
        );
    }
}
