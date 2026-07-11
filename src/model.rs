//! Core domain model shared across the crate.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Unique identifier for a refactor proposal.
pub type ProposalId = String;

/// Overall project health snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectHealth {
    pub score: f64,
    pub total_modules: usize,
    pub healthy: usize,
    pub warning: usize,
    pub critical: usize,
    #[serde(with = "crate::time::serde_trend")]
    pub entropy_trend: Vec<(Timestamp, f64)>,
    pub refactors_today: RefactorStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RefactorStats {
    pub completed: usize,
    pub pending: usize,
    pub failed: usize,
    pub loc_removed: usize,
    pub complexity_reduced: f64,
}

/// A module (file) under observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub path: PathBuf,
    pub language: Language,
    pub lines: usize,
    pub functions: usize,
    pub cyclomatic_complexity: usize,
    pub public_api_size: usize,
    pub fan_out: usize,
    pub fan_in: usize,
    pub duplicates: usize,
    pub edit_frequency: f64,
    #[serde(default)]
    pub confidence: Option<f64>,
    pub churn: usize,
    pub test_coverage: f64,
    pub entropy: f64,
    pub health: Health,
    #[serde(with = "crate::time::serde")]
    pub last_modified: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Other,
}

impl Language {
    pub fn from_path(path: &std::path::Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Language::Rust,
            Some("py") => Language::Python,
            Some("ts") => Language::TypeScript,
            Some("js") => Language::JavaScript,
            _ => Language::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Health {
    Excellent,
    Healthy,
    Warning,
    Critical,
}

impl Health {
    pub fn from_entropy(entropy: f64) -> Self {
        match entropy {
            e if e < 0.4 => Health::Excellent,
            e if e < 0.65 => Health::Healthy,
            e if e < 0.82 => Health::Warning,
            _ => Health::Critical,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Health::Excellent => "Excellent",
            Health::Healthy => "Healthy",
            Health::Warning => "Warning",
            Health::Critical => "Critical",
        }
    }
}

/// A file produced by a refactor, retained on the proposal so the change can be
/// applied, diffed, and rendered into a PR body without re-running the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub content: String,
}

/// A proposed refactoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: ProposalId,
    #[serde(with = "crate::time::serde")]
    pub created_at: Timestamp,
    pub module: PathBuf,
    pub kind: RefactorKind,
    pub confidence: f64,
    pub status: ProposalStatus,
    pub validation: Option<ValidationReport>,
    pub diff_summary: DiffSummary,
    pub migration_notes: Vec<String>,
    /// Refactored file contents produced by the engine (empty until executed).
    #[serde(default)]
    pub changed_files: Vec<ChangedFile>,
    /// Rendered pull-request body, once a branch/commit has been prepared.
    #[serde(default)]
    pub pr_body: Option<String>,
    pub timeline: Vec<TimelineEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefactorKind {
    SplitModule,
    ExtractFunction,
    RemoveDuplication,
    ReduceSurface,
    ReorderDependencies,
}

impl RefactorKind {
    pub fn description(&self) -> &'static str {
        match self {
            RefactorKind::SplitModule => "Split responsibilities into submodules",
            RefactorKind::ExtractFunction => "Extract cohesive functions",
            RefactorKind::RemoveDuplication => "Remove duplicated code",
            RefactorKind::ReduceSurface => "Reduce public API surface",
            RefactorKind::ReorderDependencies => "Break dependency cycles",
        }
    }

    /// Parse a `RefactorKind` from its human-readable `description()`/`Display`
    /// label. Returns `None` for unknown labels.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Split responsibilities into submodules" => Some(RefactorKind::SplitModule),
            "Extract cohesive functions" => Some(RefactorKind::ExtractFunction),
            "Remove duplicated code" => Some(RefactorKind::RemoveDuplication),
            "Reduce public API surface" => Some(RefactorKind::ReduceSurface),
            "Break dependency cycles" => Some(RefactorKind::ReorderDependencies),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalStatus {
    Detected,
    Queued,
    Refactoring,
    Validating,
    Accepted,
    Rejected,
    Merged,
    Conflicts,
}

impl ProposalStatus {
    /// Parse a `ProposalStatus` from its human-readable `Display` label.
    /// Returns `None` for unknown labels.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Detected" => Some(ProposalStatus::Detected),
            "Queued" => Some(ProposalStatus::Queued),
            "Refactoring" => Some(ProposalStatus::Refactoring),
            "Validating" => Some(ProposalStatus::Validating),
            "Accepted" => Some(ProposalStatus::Accepted),
            "Rejected" => Some(ProposalStatus::Rejected),
            "Merged" => Some(ProposalStatus::Merged),
            "Conflicts" => Some(ProposalStatus::Conflicts),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiffSummary {
    pub files_added: usize,
    pub files_removed: usize,
    pub files_modified: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    #[serde(with = "crate::time::serde")]
    pub at: Timestamp,
    pub message: String,
}

/// Validation report produced by the automated pipeline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationReport {
    pub fmt_ok: bool,
    pub clippy_ok: bool,
    pub check_ok: bool,
    pub test_ok: bool,
    pub api_compatible: bool,
    pub coverage_delta: f64,
    pub complexity_delta: f64,
    pub logs: Vec<String>,
}

impl ValidationReport {
    pub fn all_passed(&self) -> bool {
        self.fmt_ok && self.clippy_ok && self.check_ok && self.test_ok && self.api_compatible
    }
}

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

// ---------------------------------------------------------------------------
// Stable human-readable representations (never render these via `{:?}`).
// ---------------------------------------------------------------------------

impl std::fmt::Display for Health {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl std::fmt::Display for RefactorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.description())
    }
}

impl std::fmt::Display for ProposalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ProposalStatus::Detected => "Detected",
            ProposalStatus::Queued => "Queued",
            ProposalStatus::Refactoring => "Refactoring",
            ProposalStatus::Validating => "Validating",
            ProposalStatus::Accepted => "Accepted",
            ProposalStatus::Rejected => "Rejected",
            ProposalStatus::Merged => "Merged",
            ProposalStatus::Conflicts => "Conflicts",
        };
        f.write_str(s)
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Language::Rust => "rust",
            Language::Python => "python",
            Language::TypeScript => "typescript",
            Language::JavaScript => "javascript",
            Language::Other => "other",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::{ProposalStatus, RefactorKind};

    #[test]
    fn refactor_kind_parse_roundtrips_all_variants() {
        let kinds = [
            RefactorKind::SplitModule,
            RefactorKind::ExtractFunction,
            RefactorKind::RemoveDuplication,
            RefactorKind::ReduceSurface,
            RefactorKind::ReorderDependencies,
        ];
        for kind in kinds {
            let label = kind.description();
            assert_eq!(RefactorKind::parse(label), Some(kind), "label {label}");
            // Display must match description exactly so the journal label is stable.
            assert_eq!(kind.to_string(), label);
        }
    }

    #[test]
    fn refactor_kind_parse_rejects_unknown() {
        assert_eq!(RefactorKind::parse("nonsense"), None);
        assert_eq!(RefactorKind::parse("SplitModule"), None);
    }

    #[test]
    fn proposal_status_parse_roundtrips_all_variants() {
        let statuses = [
            ProposalStatus::Detected,
            ProposalStatus::Queued,
            ProposalStatus::Refactoring,
            ProposalStatus::Validating,
            ProposalStatus::Accepted,
            ProposalStatus::Rejected,
            ProposalStatus::Merged,
            ProposalStatus::Conflicts,
        ];
        for status in statuses {
            let label = status.to_string();
            assert_eq!(ProposalStatus::parse(&label), Some(status), "label {label}");
        }
    }

    #[test]
    fn proposal_status_parse_rejects_unknown() {
        assert_eq!(ProposalStatus::parse("nonsense"), None);
        assert_eq!(ProposalStatus::parse("queued"), None);
    }
}
