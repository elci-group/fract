use crate::json::Value;
use crate::time::{parse_rfc3339, to_rfc3339};
use crate::{Event, EventKind};
use std::path::PathBuf;

use super::VERSION;

pub(crate) fn encode_event(e: &Event) -> Value {
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

pub(crate) fn decode_event(v: &Value) -> Option<Event> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Timestamp;
    use std::time::{Duration, SystemTime};

    fn fixed_ts(secs: u64) -> Timestamp {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
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
}
