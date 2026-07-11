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
