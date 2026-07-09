//! Fract — autonomous architectural maintenance daemon.
//!
//! Fract continuously observes a software project, scores structural entropy,
//! proposes semantic refactorings, validates them, and applies them safely.

pub mod complexity;
pub mod confidence;
pub mod config;
pub mod daemon;
pub mod events;
pub mod git;
pub mod indexer;
pub mod merge;
pub mod queue;
pub mod refactor;
pub mod validation;
pub mod web;

// Internal zero-dependency replacements for third-party crates.
pub mod cli;
pub mod error;
pub mod id;
pub mod json;
pub mod scanner;
pub mod scratch;
pub mod time;
pub mod walk;

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
    pub confidence: f64,
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
