//! Activity events observed by the daemon: file saves, git activity, and
//! build/test outcomes, persisted to the journal and shown on the dashboard.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    #[serde(with = "crate::time::serde")]
    pub at: Timestamp,
    pub kind: EventKind,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EventKind {
    FileSaved,
    GitCommit { sha: String },
    BranchChanged { branch: String },
    BuildFailed { reason: String },
    TestFailed { reason: String },
    EditorHeartbeat,
}
