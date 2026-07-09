use crate::{Module, Proposal, ValidationReport};

/// Compute a confidence score [0, 1] for a proposal.
pub fn score(_module: &Module, proposal: &Proposal, validation: &ValidationReport) -> f64 {
    let compilation = bool_score(validation.all_passed());
    let tests = bool_score(validation.test_ok);
    let static_analysis = bool_score(validation.fmt_ok && validation.clippy_ok);
    let api_compat = bool_score(validation.api_compatible);
    let coverage = coverage_score(validation.coverage_delta);
    let diff_size = diff_size_score(&proposal.diff_summary);
    let complexity_reduction = complexity_reduction_score(validation.complexity_delta);

    let score = 0.25 * compilation
        + 0.20 * tests
        + 0.15 * static_analysis
        + 0.15 * api_compat
        + 0.10 * coverage
        + 0.05 * diff_size
        + 0.10 * complexity_reduction;

    score.clamp(0.0, 0.999)
}

fn bool_score(ok: bool) -> f64 {
    if ok {
        1.0
    } else {
        0.0
    }
}

fn coverage_score(delta: f64) -> f64 {
    // Negative delta is acceptable; large positive delta is suspicious.
    if delta >= 0.0 {
        (1.0 - delta.clamp(0.0, 1.0)).max(0.0)
    } else {
        1.0
    }
}

fn diff_size_score(diff: &crate::DiffSummary) -> f64 {
    let total = diff.lines_added + diff.lines_removed;
    // Smaller diffs are safer.
    1.0 - (total as f64 / 500.0).clamp(0.0, 1.0)
}

fn complexity_reduction_score(delta: f64) -> f64 {
    // Negative delta means reduction; positive means increase.
    let normalized = (-delta).clamp(-1.0, 1.0);
    (normalized + 1.0) / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::now;
    use crate::{DiffSummary, Health, Language, ProposalStatus, RefactorKind};
    use std::path::PathBuf;

    fn sample_proposal() -> Proposal {
        Proposal {
            id: "p1".to_string(),
            created_at: now(),
            module: PathBuf::from("src/lib.rs"),
            kind: RefactorKind::SplitModule,
            confidence: 0.0,
            status: ProposalStatus::Accepted,
            validation: None,
            diff_summary: DiffSummary {
                files_added: 1,
                files_removed: 0,
                files_modified: 1,
                lines_added: 50,
                lines_removed: 150,
            },
            migration_notes: vec![],
            timeline: vec![],
        }
    }

    fn sample_module() -> Module {
        Module {
            path: PathBuf::from("src/lib.rs"),
            language: Language::Rust,
            lines: 1000,
            functions: 20,
            cyclomatic_complexity: 50,
            public_api_size: 10,
            fan_out: 5,
            fan_in: 3,
            duplicates: 5,
            edit_frequency: 0.0,
            confidence: 0.0,
            churn: 0,
            test_coverage: 0.0,
            entropy: 0.0,
            health: Health::Healthy,
            last_modified: now(),
        }
    }

    #[test]
    fn perfect_validation_yields_high_confidence() {
        let module = sample_module();
        let proposal = sample_proposal();
        let validation = ValidationReport {
            fmt_ok: true,
            clippy_ok: true,
            check_ok: true,
            test_ok: true,
            api_compatible: true,
            coverage_delta: 0.0,
            complexity_delta: -1.5,
            logs: vec![],
        };
        let s = score(&module, &proposal, &validation);
        assert!(s > 0.9, "confidence was {}", s);
    }

    #[test]
    fn failing_tests_lower_confidence() {
        let module = sample_module();
        let proposal = sample_proposal();
        let validation = ValidationReport {
            fmt_ok: true,
            clippy_ok: true,
            check_ok: true,
            test_ok: false,
            api_compatible: true,
            coverage_delta: 0.0,
            complexity_delta: -1.5,
            logs: vec![],
        };
        let s = score(&module, &proposal, &validation);
        assert!(s < 0.85, "confidence was {}", s);
    }
}
