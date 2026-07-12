//! Core report model: schema, findings, summary, and the dispatching renderer.

use crate::{Health, Module, RefactorKind};
use std::path::{Path, PathBuf};

use super::style::{OutputFormat, Style, Verbosity};
use super::{render_human, render_json, render_jsonl, render_markdown, render_sarif};

/// Schema identifier shared by every machine-readable surface.
pub const SCHEMA: &str = "fract.report/v1";

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    /// Lowercase label used in human and JSON output.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }

    pub(crate) fn sarif_level(self) -> &'static str {
        match self {
            Severity::Info => "note",
            Severity::Warning => "warning",
            Severity::Critical => "error",
        }
    }
}

impl From<Health> for Severity {
    fn from(h: Health) -> Self {
        match h {
            Health::Excellent | Health::Healthy => Severity::Info,
            Health::Warning => Severity::Warning,
            Health::Critical => Severity::Critical,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub module: PathBuf,
    pub kind: String,
    pub entropy: f64,
    pub confidence: Option<f64>,
    pub message: String,
    pub why: String,
    pub next_action: String,
    pub evidence: Vec<(String, String)>,
}

impl Finding {
    /// Derive a finding from a module's metrics and the configured threshold.
    #[must_use]
    pub fn from_module(m: &Module, threshold: f64) -> Self {
        // Severity tracks actionability against the *configured* threshold so the
        // message, reason, and severity always agree. The hard `Health` band can
        // still escalate a finding to Critical.
        let over = m.entropy >= threshold;
        let mut severity = if over {
            Severity::Warning
        } else {
            Severity::Info
        };
        if m.health == Health::Critical {
            severity = Severity::Critical;
        }
        let kind = suggest_kind(m);
        let message = match severity {
            Severity::Critical => format!("Critical structural entropy ({:.2})", m.entropy),
            Severity::Warning => format!(
                "Over entropy threshold ({:.2} >= {:.2})",
                m.entropy, threshold
            ),
            Severity::Info => format!("Within entropy budget ({:.2})", m.entropy),
        };
        let why = if over {
            format!(
                "entropy {:.2} exceeds threshold {:.2}",
                m.entropy, threshold
            )
        } else {
            format!("entropy {:.2} below threshold {:.2}", m.entropy, threshold)
        };
        let next_action = if over {
            kind.description().to_string()
        } else {
            "No action required".to_string()
        };
        let evidence = vec![
            ("lines".to_string(), m.lines.to_string()),
            ("functions".to_string(), m.functions.to_string()),
            (
                "cyclomatic".to_string(),
                m.cyclomatic_complexity.to_string(),
            ),
            ("public_api".to_string(), m.public_api_size.to_string()),
            ("fan_out".to_string(), m.fan_out.to_string()),
            ("fan_in".to_string(), m.fan_in.to_string()),
            ("duplicates".to_string(), m.duplicates.to_string()),
            ("health".to_string(), m.health.to_string()),
        ];
        Finding {
            id: m.path.to_string_lossy().into_owned(),
            severity,
            module: m.path.clone(),
            kind: kind.description().to_string(),
            entropy: m.entropy,
            confidence: m.confidence,
            message,
            why,
            next_action,
            evidence,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Summary {
    pub total: usize,
    pub excellent: usize,
    pub healthy: usize,
    pub warning: usize,
    pub critical: usize,
    pub score: f64,
}

impl Summary {
    /// Aggregate health counts and score over a module set.
    #[must_use]
    pub fn from_modules(modules: &[Module]) -> Self {
        let total = modules.len();
        let mut excellent = 0;
        let mut healthy = 0;
        let mut warning = 0;
        let mut critical = 0;
        for m in modules {
            match m.health {
                Health::Excellent => excellent += 1,
                Health::Healthy => healthy += 1,
                Health::Warning => warning += 1,
                Health::Critical => critical += 1,
            }
        }
        let score = if total == 0 {
            100.0
        } else {
            let good = (excellent + healthy) as f64;
            (good * 1.0 + warning as f64 * 0.6 + critical as f64 * 0.2) / total as f64 * 100.0
        };
        Summary {
            total,
            excellent,
            healthy,
            warning,
            critical,
            score,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Report {
    pub schema: &'static str,
    pub root: PathBuf,
    pub summary: Summary,
    pub findings: Vec<Finding>,
}

impl Report {
    /// Build a report from an index, ordered by descending entropy (then path).
    /// When `over_only` is set, findings below the threshold are dropped.
    #[must_use]
    pub fn from_modules(root: &Path, modules: &[Module], threshold: f64, over_only: bool) -> Self {
        let mut sorted = modules.to_vec();
        sorted.sort_by(|a, b| {
            b.entropy
                .partial_cmp(&a.entropy)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.path.cmp(&b.path))
        });
        let summary = Summary::from_modules(&sorted);
        let mut findings: Vec<Finding> = sorted
            .iter()
            .map(|m| Finding::from_module(m, threshold))
            .collect();
        if over_only {
            findings.retain(|f| f.severity >= Severity::Warning);
        }
        Report {
            schema: SCHEMA,
            root: root.to_path_buf(),
            summary,
            findings,
        }
    }

    /// Cap the number of surfaced findings (noise budget). The summary stays
    /// intact so counts/score still reflect the whole project; only the listed
    /// findings are truncated, keeping the highest-entropy (first) rows.
    pub fn apply_budget(&mut self, max: usize) {
        if max > 0 && self.findings.len() > max {
            self.findings.truncate(max);
        }
    }

    /// Render the report in the requested output format.
    #[must_use]
    pub fn render(&self, format: OutputFormat, style: &Style, verbosity: Verbosity) -> String {
        match format {
            OutputFormat::Human => render_human(self, style, verbosity),
            OutputFormat::Json => render_json(self).to_string(),
            OutputFormat::Jsonl => render_jsonl(self),
            OutputFormat::Sarif => render_sarif(self).to_string(),
            OutputFormat::Markdown => render_markdown(self),
        }
    }
}

/// Mirror of the daemon's candidate classifier, kept here so renderers are
/// self-contained and testable without spinning up a daemon.
#[must_use]
pub fn suggest_kind(m: &Module) -> RefactorKind {
    if m.lines > 1500 || m.functions > 40 {
        RefactorKind::SplitModule
    } else if m.duplicates > 10 {
        RefactorKind::RemoveDuplication
    } else if m.public_api_size > 30 {
        RefactorKind::ReduceSurface
    } else if m.fan_out > 15 {
        RefactorKind::ReorderDependencies
    } else {
        RefactorKind::ExtractFunction
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;
    use std::time::SystemTime;

    fn module(path: &str, entropy: f64, health: Health) -> Module {
        Module {
            path: PathBuf::from(path),
            language: Language::Rust,
            lines: 100,
            functions: 10,
            cyclomatic_complexity: 5,
            public_api_size: 4,
            fan_out: 2,
            fan_in: 1,
            duplicates: 0,
            edit_frequency: 0.0,
            confidence: Some(0.75),
            churn: 0,
            test_coverage: 0.0,
            entropy,
            health,
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn severity_labels_and_sarif_levels() {
        for (severity, label, level) in [
            (Severity::Info, "info", "note"),
            (Severity::Warning, "warning", "warning"),
            (Severity::Critical, "critical", "error"),
        ] {
            assert_eq!(severity.label(), label);
            assert_eq!(severity.sarif_level(), level);
        }
    }

    #[test]
    fn severity_from_health_bands() {
        assert_eq!(Severity::from(Health::Excellent), Severity::Info);
        assert_eq!(Severity::from(Health::Healthy), Severity::Info);
        assert_eq!(Severity::from(Health::Warning), Severity::Warning);
        assert_eq!(Severity::from(Health::Critical), Severity::Critical);
    }

    #[test]
    fn finding_under_threshold_is_informational() {
        let m = module("src/ok.rs", 0.3, Health::Excellent);
        let f = Finding::from_module(&m, 0.82);
        assert_eq!(f.severity, Severity::Info);
        assert!(f.message.contains("Within entropy budget"), "{}", f.message);
        assert!(f.why.contains("below threshold"), "{}", f.why);
        assert_eq!(f.next_action, "No action required");
        assert_eq!(f.confidence, Some(0.75));
        assert_eq!(f.evidence.len(), 8);
    }

    #[test]
    fn finding_over_threshold_is_a_warning() {
        let m = module("src/warn.rs", 0.9, Health::Warning);
        let f = Finding::from_module(&m, 0.82);
        assert_eq!(f.severity, Severity::Warning);
        assert!(
            f.message.contains("Over entropy threshold"),
            "{}",
            f.message
        );
        assert!(f.why.contains("exceeds threshold"), "{}", f.why);
        assert_eq!(f.next_action, f.kind);
    }

    #[test]
    fn critical_health_escalates_even_below_threshold() {
        let m = module("src/crit.rs", 0.5, Health::Critical);
        let f = Finding::from_module(&m, 0.82);
        assert_eq!(f.severity, Severity::Critical);
        assert!(
            f.message.contains("Critical structural entropy"),
            "{}",
            f.message
        );
    }

    #[test]
    fn summary_counts_bands_and_scores() {
        let modules = [
            module("a.rs", 0.1, Health::Excellent),
            module("b.rs", 0.5, Health::Healthy),
            module("c.rs", 0.7, Health::Warning),
            module("d.rs", 0.9, Health::Critical),
        ];
        let s = Summary::from_modules(&modules);
        assert_eq!(s.total, 4);
        assert_eq!(s.excellent, 1);
        assert_eq!(s.healthy, 1);
        assert_eq!(s.warning, 1);
        assert_eq!(s.critical, 1);
        // (2*1.0 + 1*0.6 + 1*0.2) / 4 * 100 = 70
        assert!((s.score - 70.0).abs() < 1e-9, "score {}", s.score);

        let empty = Summary::from_modules(&[]);
        assert_eq!(empty.total, 0);
        assert!((empty.score - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn report_orders_findings_and_applies_budget() {
        let modules = [
            module("low.rs", 0.2, Health::Healthy),
            module("high.rs", 0.95, Health::Critical),
            module("mid.rs", 0.6, Health::Healthy),
        ];
        let mut report = Report::from_modules(Path::new("/tmp/x"), &modules, 0.82, false);
        assert_eq!(report.findings[0].module, PathBuf::from("high.rs"));
        assert_eq!(report.findings.len(), 3);
        report.apply_budget(1);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(
            report.summary.total, 3,
            "budget trims findings, not the summary"
        );
        // A zero budget disables truncation entirely.
        let mut report = Report::from_modules(Path::new("/tmp/x"), &modules, 0.82, false);
        report.apply_budget(0);
        assert_eq!(report.findings.len(), 3);
    }

    #[test]
    fn report_over_only_drops_informational_findings() {
        let modules = [
            module("low.rs", 0.2, Health::Healthy),
            module("high.rs", 0.95, Health::Critical),
        ];
        let report = Report::from_modules(Path::new("/tmp/x"), &modules, 0.82, true);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].module, PathBuf::from("high.rs"));
    }

    #[test]
    fn suggest_kind_matches_pipeline_classifier() {
        let mut m = module("m.rs", 0.9, Health::Warning);
        assert_eq!(suggest_kind(&m), RefactorKind::ExtractFunction);
        m.lines = 1_501;
        assert_eq!(suggest_kind(&m), RefactorKind::SplitModule);
        m.lines = 0;
        m.functions = 41;
        assert_eq!(suggest_kind(&m), RefactorKind::SplitModule);
        m.functions = 0;
        m.duplicates = 11;
        assert_eq!(suggest_kind(&m), RefactorKind::RemoveDuplication);
        m.duplicates = 0;
        m.public_api_size = 31;
        assert_eq!(suggest_kind(&m), RefactorKind::ReduceSurface);
        m.public_api_size = 0;
        m.fan_out = 16;
        assert_eq!(suggest_kind(&m), RefactorKind::ReorderDependencies);
    }
}
