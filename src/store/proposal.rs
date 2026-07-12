//! Journal codec for `Proposal` records: persists identity and lifecycle
//! only — heavy `changed_files` payloads are re-derived on demand, not
//! journaled.

use crate::json::Value;
use crate::time::{parse_rfc3339, to_rfc3339};
use crate::{DiffSummary, Proposal, ProposalStatus, RefactorKind, TimelineEvent};
use std::path::PathBuf;

use super::VERSION;

pub(crate) fn encode_proposal(p: &Proposal) -> Value {
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

pub(crate) fn decode_proposal(v: &Value) -> Option<Proposal> {
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
        files_added: super::req_usize(ds, "files_added")?,
        files_removed: super::req_usize(ds, "files_removed")?,
        files_modified: super::req_usize(ds, "files_modified")?,
        lines_added: super::req_usize(ds, "lines_added")?,
        lines_removed: super::req_usize(ds, "lines_removed")?,
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
    use crate::store::Store;
    use crate::time::Timestamp;
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
            "fract-store-proposal-test-{}-{n}",
            std::process::id()
        ));
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
    // Round-trip must be bit-exact: confidence is written/read through the
    // in-tree JSON codec, so an epsilon comparison would weaken the test.
    #[allow(clippy::float_cmp)]
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
}
