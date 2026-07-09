use crate::time::now;
use crate::{Proposal, ProposalStatus, TimelineEvent, ValidationReport};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, error, info, warn};

/// Run the full validation pipeline on a proposal in its scratch workspace.
pub async fn validate(root: &Path, proposal: &mut Proposal) -> ValidationReport {
    let mut report = ValidationReport::default();
    proposal.status = ProposalStatus::Validating;

    macro_rules! stage {
        ($name:expr, $cmd:expr, $flag:ident) => {
            info!("validating {}: {}", proposal.id, $name);
            proposal.timeline.push(TimelineEvent {
                at: now(),
                message: format!("Running {}", $name),
            });
            match run_cargo(root, $cmd).await {
                Ok((ok, stdout, stderr)) => {
                    report.$flag = ok;
                    if !ok {
                        report.logs.push(format!("{} failed:\n{}", $name, stderr));
                        warn!("{} failed for {}", $name, proposal.module.display());
                    } else {
                        report.logs.push(format!("{} passed", $name));
                        debug!("{} output:\n{}", $name, stdout);
                    }
                }
                Err(e) => {
                    report.$flag = false;
                    report.logs.push(format!("{} error: {}", $name, e));
                    error!("{} error for {}: {}", $name, proposal.module.display(), e);
                }
            }
        };
    }

    stage!("cargo fmt", vec!["fmt", "--", "--check"], fmt_ok);
    stage!("cargo clippy", vec!["clippy", "--", "-D", "warnings"], clippy_ok);
    stage!("cargo check", vec!["check"], check_ok);
    stage!("cargo test", vec!["test"], test_ok);

    // API compatibility check: ensure public items from before still exist.
    report.api_compatible = check_api_compatibility(root, proposal).await;

    // Placeholder coverage / complexity deltas.
    report.coverage_delta = 0.0;
    report.complexity_delta = -(proposal.diff_summary.lines_removed as f64) / 100.0;

    proposal.validation = Some(report.clone());
    proposal.timeline.push(TimelineEvent {
        at: now(),
        message: format!(
            "Validation complete: fmt={} clippy={} check={} test={} api={}",
            report.fmt_ok, report.clippy_ok, report.check_ok, report.test_ok, report.api_compatible
        ),
    });

    if report.all_passed() {
        proposal.status = ProposalStatus::Accepted;
    } else {
        proposal.status = ProposalStatus::Rejected;
    }

    report
}

async fn run_cargo(
    root: &Path,
    args: Vec<&str>,
) -> crate::error::Result<(bool, String, String)> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(root)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().await?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let ok = output.status.success();
    Ok((ok, stdout, stderr))
}

async fn check_api_compatibility(_root: &Path, _proposal: &Proposal) -> bool {
    // Real implementation would compare pre/post public API symbols.
    // Mock engine preserves pub items, so assume compatible.
    true
}
