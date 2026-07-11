use crate::error::{Context, Result};
use crate::time::now;
use crate::{DiffSummary, Module, Proposal, ProposalStatus, TimelineEvent};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

/// Context package sent to the LLM refactor engine.
#[derive(Debug, Clone)]
pub struct RefactorContext {
    pub module: Module,
    pub source: String,
    pub imports: Vec<String>,
    pub exports: Vec<String>,
    pub dependents: Vec<PathBuf>,
    pub project_conventions: String,
}

/// Refactor engine abstraction.
pub trait RefactorEngine: Send + Sync {
    fn refactor(
        &self,
        ctx: RefactorContext,
    ) -> Pin<Box<dyn Future<Output = Result<RefactorOutput>> + Send + '_>>;
}

pub struct RefactorOutput {
    pub files: Vec<(PathBuf, String)>,
    pub migration_notes: Vec<String>,
    pub diff_summary: DiffSummary,
}

/// Mock engine for offline / demo use. Performs simple structural splits.
pub struct MockRefactorEngine;

impl RefactorEngine for MockRefactorEngine {
    fn refactor(
        &self,
        ctx: RefactorContext,
    ) -> Pin<Box<dyn Future<Output = Result<RefactorOutput>> + Send + '_>> {
        Box::pin(async move {
            let mut files = Vec::new();
            let original = ctx.source;
            let lines: Vec<&str> = original.lines().collect();

            match ctx.module.language {
                crate::Language::Rust => {
                    // Simple split: put public items in lib.rs-like file and internals in internal.rs.
                    let (public, internal): (Vec<&str>, Vec<&str>) = lines
                        .iter()
                        .copied()
                        .partition(|l| l.trim().starts_with("pub ") || l.trim().starts_with("//"));
                    files.push((ctx.module.path.clone(), public.join("\n")));
                    let mut internal_path = ctx.module.path.clone();
                    internal_path.set_extension("");
                    let stem = internal_path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy();
                    let internal_file = PathBuf::from(format!("{}_internal.rs", stem));
                    files.push((internal_file, internal.join("\n")));
                }
                _ => {
                    // Default: extract comments/header as migration notes and return cleaned file.
                    let cleaned: Vec<_> =
                        lines.into_iter().filter(|l| !l.trim().is_empty()).collect();
                    files.push((ctx.module.path, cleaned.join("\n")));
                }
            }

            let diff_summary = DiffSummary {
                files_added: files.len().saturating_sub(1),
                files_removed: 0,
                files_modified: 1,
                lines_added: 0,
                lines_removed: original.len() / 20, // pretend we removed 5%
            };

            Ok(RefactorOutput {
                files,
                migration_notes: vec![
                    "Preserve observable behaviour.".to_string(),
                    "Minimise public API changes.".to_string(),
                    "Split responsibilities.".to_string(),
                ],
                diff_summary,
            })
        })
    }
}

/// Build a context package for a module.
pub fn build_context(root: &Path, module: &Module) -> Result<RefactorContext> {
    let full_path = root.join(&module.path);
    let source = std::fs::read_to_string(&full_path)
        .with_context(|| format!("reading {}", full_path.display()))?;

    let imports: Vec<String> = source
        .lines()
        .filter(|l| {
            let t = l.trim();
            t.starts_with("use ") || t.starts_with("import ") || t.starts_with("from ")
        })
        .map(|l| l.to_string())
        .collect();

    let exports: Vec<String> = source
        .lines()
        .filter(|l| {
            let t = l.trim();
            t.starts_with("pub ") || t.starts_with("export ")
        })
        .map(|l| l.to_string())
        .collect();

    let project_conventions = std::fs::read_to_string(root.join("rustfmt.toml"))
        .or_else(|_| std::fs::read_to_string(root.join(".rustfmt.toml")))
        .unwrap_or_else(|_| "Default Rust conventions".to_string());

    Ok(RefactorContext {
        module: module.clone(),
        source,
        imports,
        exports,
        dependents: Vec::new(),
        project_conventions,
    })
}

/// Execute a refactor proposal in a temporary workspace.
pub async fn execute_proposal(
    engine: &dyn RefactorEngine,
    root: &Path,
    proposal: &mut Proposal,
    module: &Module,
) -> Result<RefactorOutput> {
    proposal.status = ProposalStatus::Refactoring;
    proposal.timeline.push(TimelineEvent {
        at: now(),
        message: "Building semantic context package".to_string(),
    });

    let ctx = build_context(root, module)?;

    proposal.timeline.push(TimelineEvent {
        at: now(),
        message: "Invoking LLM refactor engine".to_string(),
    });

    let output = engine.refactor(ctx).await?;

    proposal.timeline.push(TimelineEvent {
        at: now(),
        message: format!(
            "Generated {} file(s) with {} added, {} removed",
            output.files.len(),
            output.diff_summary.lines_added,
            output.diff_summary.lines_removed
        ),
    });

    proposal.diff_summary = output.diff_summary.clone();
    proposal.migration_notes = output.migration_notes.clone();
    proposal.changed_files = output
        .files
        .iter()
        .map(|(path, content)| crate::ChangedFile {
            path: path.clone(),
            content: content.clone(),
        })
        .collect();

    Ok(output)
}
