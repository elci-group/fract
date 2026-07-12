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

/// Upper entropy bound (exclusive) for `Health::Excellent`.
const HEALTH_EXCELLENT_MAX: f64 = 0.4;
/// Upper entropy bound (exclusive) for `Health::Healthy`.
const HEALTH_HEALTHY_MAX: f64 = 0.65;
/// Upper entropy bound (exclusive) for `Health::Warning`; above it a module is
/// `Health::Critical`.
const HEALTH_WARNING_MAX: f64 = 0.82;

impl Health {
    /// Band an entropy score into a health level.
    #[must_use]
    pub fn from_entropy(entropy: f64) -> Self {
        match entropy {
            e if e < HEALTH_EXCELLENT_MAX => Health::Excellent,
            e if e < HEALTH_HEALTHY_MAX => Health::Healthy,
            e if e < HEALTH_WARNING_MAX => Health::Warning,
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

#[cfg(test)]
mod tests {
    use super::{Health, Language};
    use std::path::Path;

    #[test]
    fn health_from_entropy_band_boundaries() {
        // Boundary values land in the higher band: 0.4 is Healthy, not Excellent.
        assert_eq!(Health::from_entropy(0.0), Health::Excellent);
        assert_eq!(Health::from_entropy(0.3999), Health::Excellent);
        assert_eq!(Health::from_entropy(0.4), Health::Healthy);
        assert_eq!(Health::from_entropy(0.6499), Health::Healthy);
        assert_eq!(Health::from_entropy(0.65), Health::Warning);
        assert_eq!(Health::from_entropy(0.8199), Health::Warning);
        assert_eq!(Health::from_entropy(0.82), Health::Critical);
        assert_eq!(Health::from_entropy(1.0), Health::Critical);
    }

    #[test]
    fn health_labels_match_display() {
        for (health, label) in [
            (Health::Excellent, "Excellent"),
            (Health::Healthy, "Healthy"),
            (Health::Warning, "Warning"),
            (Health::Critical, "Critical"),
        ] {
            assert_eq!(health.label(), label);
            assert_eq!(health.to_string(), label);
        }
    }

    #[test]
    fn language_from_path_known_extensions() {
        assert_eq!(Language::from_path(Path::new("a.rs")), Language::Rust);
        assert_eq!(Language::from_path(Path::new("a.py")), Language::Python);
        assert_eq!(
            Language::from_path(Path::new("dir/b.ts")),
            Language::TypeScript
        );
        assert_eq!(Language::from_path(Path::new("a.js")), Language::JavaScript);
    }

    #[test]
    fn language_from_path_unknown_or_missing_extension() {
        assert_eq!(Language::from_path(Path::new("a.go")), Language::Other);
        assert_eq!(Language::from_path(Path::new("Makefile")), Language::Other);
        // Detection is case-sensitive: an uppercase extension is not Rust.
        assert_eq!(Language::from_path(Path::new("a.RS")), Language::Other);
    }

    #[test]
    fn language_display_labels() {
        for (language, label) in [
            (Language::Rust, "rust"),
            (Language::Python, "python"),
            (Language::TypeScript, "typescript"),
            (Language::JavaScript, "javascript"),
            (Language::Other, "other"),
        ] {
            assert_eq!(language.to_string(), label);
        }
    }
}
