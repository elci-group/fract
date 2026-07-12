//! Module observation model: per-file metrics, language detection, and the
//! entropy-derived health bands used across reports and the daemon.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    /// Detect the language from a file extension.
    #[must_use]
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
    /// Band an entropy score into a health level.
    #[must_use]
    pub fn from_entropy(entropy: f64) -> Self {
        match entropy {
            e if e < 0.4 => Health::Excellent,
            e if e < 0.65 => Health::Healthy,
            e if e < 0.82 => Health::Warning,
            _ => Health::Critical,
        }
    }

    /// Human-readable label for the health level.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Health::Excellent => "Excellent",
            Health::Healthy => "Healthy",
            Health::Warning => "Warning",
            Health::Critical => "Critical",
        }
    }
}

// ---------------------------------------------------------------------------
// Stable human-readable representations (never render these via `{:?}`).
// ---------------------------------------------------------------------------

impl std::fmt::Display for Health {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
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
