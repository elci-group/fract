//! Zero-dependency append-only JSONL journal. Persists proposals
//! (identity + lifecycle, NOT heavy `changed_files` payloads — those are
//! re-derived on demand), recent events, and the health trend so the
//! dashboard/queue survive a restart. Encoded with the in-tree `json::Value`;
//! `serde_json` is amber-forbidden.

use crate::error::Result;
use crate::json::Value;
use crate::{Event, ProjectHealth, Proposal};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::warn;

mod event;
mod health;
mod proposal;

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
        if let Err(e) = std::fs::create_dir_all(&dir) {
            warn!(
                event = "store.init_failed",
                path = %dir.display(),
                error = %e,
                "failed to create .fract state directory"
            );
        }
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
            // Missing journal is the normal first-run case: stay silent.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return loaded,
            Err(e) => {
                warn!(
                    event = "store.read_failed",
                    path = %self.path.display(),
                    error = %e,
                    "failed to read journal; starting from empty state"
                );
                return loaded;
            }
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
            let Some(kind) = value.get("type").and_then(Value::as_str) else {
                warn!(
                    event = "store.skip",
                    line = line_no,
                    "journal line missing type"
                );
                continue;
            };
            match kind {
                "health" => {
                    if let Some(h) = health::decode_health(&value) {
                        loaded.health = Some(h);
                    } else {
                        warn!(
                            event = "store.skip",
                            line = line_no,
                            "malformed health record"
                        );
                    }
                }
                "event" => {
                    if let Some(e) = event::decode_event(&value) {
                        loaded.events.push(e);
                    } else {
                        warn!(
                            event = "store.skip",
                            line = line_no,
                            "malformed event record"
                        );
                    }
                }
                "proposal" => {
                    if let Some(p) = proposal::decode_proposal(&value) {
                        loaded.proposals.push(p);
                    } else {
                        warn!(
                            event = "store.skip",
                            line = line_no,
                            "malformed proposal record"
                        );
                    }
                }
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

    /// Append one proposal record to the journal.
    ///
    /// # Errors
    /// Returns an error if the journal file cannot be opened, written, or
    /// flushed.
    pub fn append_proposal(&self, p: &Proposal) -> Result<()> {
        self.append(&proposal::encode_proposal(p))
    }

    /// Append one event record to the journal.
    ///
    /// # Errors
    /// Returns an error if the journal file cannot be opened, written, or
    /// flushed.
    pub fn append_event(&self, e: &Event) -> Result<()> {
        self.append(&event::encode_event(e))
    }

    /// Append one health record to the journal.
    ///
    /// # Errors
    /// Returns an error if the journal file cannot be opened, written, or
    /// flushed.
    pub fn append_health(&self, h: &ProjectHealth) -> Result<()> {
        self.append(&health::encode_health(h))
    }

    /// `load` on the blocking pool, for async callers: the journal read is
    /// synchronous `std::fs` and must not run on a tokio worker thread.
    ///
    /// # Panics
    /// Panics if the blocking task panics.
    pub async fn load_async(&self) -> Loaded {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.load())
            .await
            .expect("store load panicked")
    }

    /// `append_proposal` on the blocking pool, for async callers.
    ///
    /// # Errors
    /// Returns an error if the journal file cannot be opened, written, or
    /// flushed.
    ///
    /// # Panics
    /// Panics if the blocking task panics.
    pub async fn append_proposal_async(&self, p: &Proposal) -> Result<()> {
        let store = self.clone();
        let p = p.clone();
        tokio::task::spawn_blocking(move || store.append_proposal(&p))
            .await
            .expect("store append panicked")
    }

    /// `append_event` on the blocking pool, for async callers.
    ///
    /// # Errors
    /// Returns an error if the journal file cannot be opened, written, or
    /// flushed.
    ///
    /// # Panics
    /// Panics if the blocking task panics.
    pub async fn append_event_async(&self, e: &Event) -> Result<()> {
        let store = self.clone();
        let e = e.clone();
        tokio::task::spawn_blocking(move || store.append_event(&e))
            .await
            .expect("store append panicked")
    }

    /// `append_health` on the blocking pool, for async callers.
    ///
    /// # Errors
    /// Returns an error if the journal file cannot be opened, written, or
    /// flushed.
    ///
    /// # Panics
    /// Panics if the blocking task panics.
    pub async fn append_health_async(&self, h: &ProjectHealth) -> Result<()> {
        let store = self.clone();
        let h = h.clone();
        tokio::task::spawn_blocking(move || store.append_health(&h))
            .await
            .expect("store append panicked")
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
// Decoding — every required field must be present and well-typed, else the
// whole line is treated as malformed (returns None → caller skips + warns).
// ---------------------------------------------------------------------------

pub(crate) fn req_usize(obj: &Value, key: &str) -> Option<usize> {
    let n = obj.get(key)?.as_f64()?;
    // Journal counts are written as integers by the encoder; the `as` cast's
    // saturating/truncating semantics on corrupt input are acceptable here
    // because malformed records are validated field-by-field downstream.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(n as usize)
}

#[cfg(test)]
mod tests {
    use super::proposal::encode_proposal;
    use super::*;
    use crate::time::Timestamp;
    use crate::{DiffSummary, ProposalStatus, RefactorKind};
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

    fn health_fixture() -> ProjectHealth {
        ProjectHealth {
            score: 90.0,
            total_modules: 1,
            healthy: 1,
            warning: 0,
            critical: 0,
            entropy_trend: Vec::new(),
            refactors_today: crate::RefactorStats::default(),
        }
    }

    #[test]
    fn load_skips_malformed_typed_records() {
        let dir = temp_dir();
        let store = Store::open(&dir);
        // Each line is valid JSON but fails record decoding: missing `type`,
        // then one malformed record per known kind.
        let contents = concat!(
            "{\"v\":1}\n",
            "{\"v\":1,\"type\":\"health\"}\n",
            "{\"v\":1,\"type\":\"event\"}\n",
            "{\"v\":1,\"type\":\"proposal\"}\n"
        );
        std::fs::write(dir.join(".fract").join("state.jsonl"), contents).unwrap();

        let loaded = store.load();
        assert!(loaded.proposals.is_empty());
        assert!(loaded.events.is_empty());
        assert!(loaded.health.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn append_event_and_async_helpers_roundtrip() {
        let dir = temp_dir();
        let store = Store::open(&dir);
        let event = Event {
            at: fixed_ts(1_700_000_000),
            kind: crate::EventKind::FileSaved,
            path: Some(PathBuf::from("src/lib.rs")),
        };
        store.append_event(&event).unwrap();
        store.append_event_async(&event).await.unwrap();
        store.append_health_async(&health_fixture()).await.unwrap();

        let loaded = store.load_async().await;
        assert_eq!(loaded.events.len(), 2);
        assert!(loaded.health.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
