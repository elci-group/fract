use crate::time::now;
use crate::{Proposal, ProposalStatus, TimelineEvent, ValidationReport};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, error, info, warn};

/// Run the full validation pipeline on a proposal in its scratch workspace.
#[tracing::instrument(skip(root, proposal), fields(proposal = %proposal.id, module = %proposal.module.display()))]
pub async fn validate(root: &Path, proposal: &mut Proposal) -> ValidationReport {
    let mut report = ValidationReport::default();
    proposal.status = ProposalStatus::Validating;

    macro_rules! stage {
        ($name:expr, $cmd:expr, $flag:ident) => {
            info!(event = "validate.stage", stage = $name, "running validation stage");
            proposal.timeline.push(TimelineEvent {
                at: now(),
                message: format!("Running {}", $name),
            });
            match run_cargo(root, $cmd).await {
                Ok((ok, stdout, stderr)) => {
                    report.$flag = ok;
                    if !ok {
                        report.logs.push(format!("{} failed:\n{}", $name, stderr));
                        warn!(
                            event = "validate.stage_failed",
                            stage = $name,
                            module = %proposal.module.display(),
                            "validation stage failed"
                        );
                    } else {
                        report.logs.push(format!("{} passed", $name));
                        debug!(
                            event = "validate.stage_output",
                            stage = $name,
                            stdout = %stdout,
                            "stage output"
                        );
                    }
                }
                Err(e) => {
                    report.$flag = false;
                    report.logs.push(format!("{} error: {}", $name, e));
                    error!(
                        event = "validate.stage_error",
                        stage = $name,
                        module = %proposal.module.display(),
                        error = %e,
                        "validation stage error"
                    );
                }
            }
        };
    }

    stage!("cargo fmt", vec!["fmt", "--", "--check"], fmt_ok);
    stage!(
        "cargo clippy",
        vec!["clippy", "--", "-D", "warnings"],
        clippy_ok
    );
    stage!("cargo check", vec!["check"], check_ok);
    stage!("cargo test", vec!["test"], test_ok);

    // API compatibility check: ensure public items from before still exist.
    let (api, api_logs) = check_api_compatibility(root, proposal).await;
    report.api_compatible = api;
    report.logs.extend(api_logs);

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

async fn run_cargo(root: &Path, args: Vec<&str>) -> crate::error::Result<(bool, String, String)> {
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

/// Public symbols present in `before` but missing from `after` (i.e. breaking removals).
pub(crate) fn removed_public_symbols(
    before: &str,
    after: &str,
    lang: crate::Language,
) -> Vec<String> {
    use std::collections::BTreeSet;
    let a: BTreeSet<String> = crate::scanner::public_symbols(before, lang)
        .into_iter()
        .collect();
    let b: BTreeSet<String> = crate::scanner::public_symbols(after, lang)
        .into_iter()
        .collect();
    a.difference(&b).cloned().collect()
}

async fn check_api_compatibility(root: &Path, proposal: &Proposal) -> (bool, Vec<String>) {
    // (relpath, after_content) pairs to inspect.
    let pairs: Vec<(std::path::PathBuf, String)> = if proposal.changed_files.is_empty() {
        match tokio::fs::read_to_string(root.join(&proposal.module)).await {
            Ok(s) => vec![(proposal.module.clone(), s)],
            Err(e) => {
                // Conservative: an unreadable module cannot be checked, so fail
                // validation rather than wave a possibly-breaking change through.
                warn!(
                    event = "validation.module_unreadable",
                    path = %proposal.module.display(),
                    error = %e,
                    "module unreadable during API check; treating as incompatible"
                );
                return (false, Vec::new());
            }
        }
    } else {
        proposal
            .changed_files
            .iter()
            .map(|cf| (cf.path.clone(), cf.content.clone()))
            .collect()
    };

    let mut compatible = true;
    let mut logs: Vec<String> = Vec::new();
    for (path, after) in pairs {
        let lang = crate::Language::from_path(&path);
        if lang == crate::Language::Other {
            logs.push(format!(
                "api check skipped for {} (unsupported language)",
                path.display()
            ));
            continue;
        }
        let rel = if path.is_absolute() {
            path.strip_prefix(root).unwrap_or(&path).to_path_buf()
        } else {
            path.clone()
        };
        // New file (or git failure) → empty baseline → no removals required.
        let before = match git_show_head(root, &rel).await {
            Some(b) => b,
            None => {
                warn!(
                    event = "validation.git_show_failed",
                    path = %rel.display(),
                    "git show HEAD failed; using empty baseline"
                );
                String::new()
            }
        };
        let removed = removed_public_symbols(&before, &after, lang);
        if !removed.is_empty() {
            compatible = false;
            warn!(
                event = "validate.api_incompatible",
                module = %rel.display(),
                removed = ?removed,
                "public symbols removed by refactor"
            );
            logs.push(format!(
                "API incompatible: {} removed public symbol(s) in {}: {:?}",
                removed.len(),
                rel.display(),
                removed
            ));
        }
    }
    (compatible, logs)
}

async fn git_show_head(root: &Path, rel: &Path) -> Option<String> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("show")
        .arg(format!("HEAD:{}", rel.display()))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Language;

    #[test]
    fn rust_removal_detected() {
        let before = "pub fn a() {}\npub fn b() {}\n";
        let after = "pub fn a() {}\n";
        let removed = removed_public_symbols(before, after, Language::Rust);
        assert_eq!(removed, vec!["b".to_string()]);
    }

    #[test]
    fn rust_superset_yields_no_removals() {
        let before = "pub fn a() {}\n";
        let after = "pub fn a() {}\npub fn b() {}\n";
        let removed = removed_public_symbols(before, after, Language::Rust);
        assert!(removed.is_empty());
    }

    #[test]
    fn python_removal_detected() {
        let before = "def a():\n    pass\ndef b():\n    pass\n";
        let after = "def a():\n    pass\n";
        let removed = removed_public_symbols(before, after, Language::Python);
        assert_eq!(removed, vec!["b".to_string()]);
    }

    #[test]
    fn jsts_removal_detected() {
        let before = "export function a() {}\nexport function b() {}\n";
        let after = "export function a() {}\n";
        let removed = removed_public_symbols(before, after, Language::TypeScript);
        assert_eq!(removed, vec!["b".to_string()]);
    }

    #[test]
    fn comment_or_string_false_positive_guarded() {
        // `ghost`/`commented` only ever appear inside a string/comment, so they
        // are not real public symbols and their "removal" must not be reported.
        let before = r#"pub fn real() {}
let s = "pub fn ghost()";
// pub fn commented() {}
"#;
        let after = r#"pub fn real() {}
"#;
        let removed = removed_public_symbols(before, after, Language::Rust);
        assert!(removed.is_empty(), "got {removed:?}");
    }

    #[test]
    fn empty_before_yields_no_removals() {
        // A brand-new file (nothing in HEAD) can never break the public API.
        let removed = removed_public_symbols("", "pub fn a() {}\npub fn b() {}\n", Language::Rust);
        assert!(removed.is_empty(), "got {removed:?}");
    }

    #[test]
    fn rust_removed_struct_is_reported() {
        let before = "pub struct Foo;\npub fn keep() {}\n";
        let after = "pub fn keep() {}\n";
        let removed = removed_public_symbols(before, after, Language::Rust);
        assert_eq!(removed, vec!["Foo".to_string()]);
    }
}
