//! Refactor-engine abstraction: the `RefactorEngine` trait, the context
//! package sent to an engine, and `MockRefactorEngine`, the offline
//! engine performing simple structural splits.

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

            if ctx.module.language == crate::Language::Rust {
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
                let internal_file = PathBuf::from(format!("{stem}_internal.rs"));
                files.push((internal_file, internal.join("\n")));
            } else {
                // Default: extract comments/header as migration notes and return cleaned file.
                let cleaned: Vec<_> = lines.into_iter().filter(|l| !l.trim().is_empty()).collect();
                files.push((ctx.module.path, cleaned.join("\n")));
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
///
/// # Errors
/// Returns an error if the module source file cannot be read.
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
        .map(ToString::to_string)
        .collect();

    let exports: Vec<String> = source
        .lines()
        .filter(|l| {
            let t = l.trim();
            t.starts_with("pub ") || t.starts_with("export ")
        })
        .map(ToString::to_string)
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
///
/// # Errors
/// Returns an error if the context-building task fails to join, the module
/// source cannot be read, or the refactor engine returns an error.
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

    // build_context does synchronous std::fs reads; keep them off the worker.
    let root = root.to_path_buf();
    let module = module.clone();
    let ctx = tokio::task::spawn_blocking(move || build_context(&root, &module)).await??;

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
    proposal.migration_notes.clone_from(&output.migration_notes);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Health, Language, RefactorKind};
    use std::time::SystemTime;

    fn temp_dir() -> PathBuf {
        // Rust runs the test binary's tests in parallel threads within one
        // process, so a pid-only name would collide. Mix in a per-call counter.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("fract-refactor-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn module_fixture(path: &str, language: Language) -> Module {
        Module {
            path: PathBuf::from(path),
            language,
            lines: 100,
            functions: 10,
            cyclomatic_complexity: 5,
            public_api_size: 4,
            fan_out: 2,
            fan_in: 1,
            duplicates: 0,
            edit_frequency: 0.0,
            confidence: None,
            churn: 0,
            test_coverage: 0.0,
            entropy: 0.9,
            health: Health::from_entropy(0.9),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    fn context_for(module: &Module, source: &str) -> RefactorContext {
        RefactorContext {
            module: module.clone(),
            source: source.to_string(),
            imports: Vec::new(),
            exports: Vec::new(),
            dependents: Vec::new(),
            project_conventions: "Default Rust conventions".to_string(),
        }
    }

    #[tokio::test]
    async fn mock_engine_splits_rust_module_into_public_and_internal() {
        let source = "// header comment\npub fn api() {}\nfn helper() {}\n";
        let module = module_fixture("src/lib.rs", Language::Rust);
        let output = MockRefactorEngine
            .refactor(context_for(&module, source))
            .await
            .unwrap();
        assert_eq!(output.files.len(), 2);
        let (public_path, public_body) = &output.files[0];
        assert_eq!(public_path, &PathBuf::from("src/lib.rs"));
        assert!(public_body.contains("pub fn api()"));
        assert!(public_body.contains("// header comment"));
        assert!(!public_body.contains("fn helper()"));
        let (internal_path, internal_body) = &output.files[1];
        assert_eq!(internal_path, &PathBuf::from("lib_internal.rs"));
        assert!(internal_body.contains("fn helper()"));
        assert_eq!(output.diff_summary.files_added, 1);
        assert_eq!(output.diff_summary.files_modified, 1);
        assert_eq!(output.migration_notes.len(), 3);
    }

    #[tokio::test]
    async fn mock_engine_cleans_non_rust_module_in_place() {
        let source = "def a():\n    pass\n\n\ndef b():\n    pass\n";
        let module = module_fixture("pkg/mod.py", Language::Python);
        let output = MockRefactorEngine
            .refactor(context_for(&module, source))
            .await
            .unwrap();
        assert_eq!(output.files.len(), 1);
        let (path, body) = &output.files[0];
        assert_eq!(path, &PathBuf::from("pkg/mod.py"));
        // Blank lines are dropped; real lines survive in order.
        assert!(!body.contains("\n\n"));
        assert_eq!(body, "def a():\n    pass\ndef b():\n    pass");
        assert_eq!(output.diff_summary.files_added, 0);
    }

    #[test]
    fn build_context_extracts_imports_exports_and_default_conventions() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "use std::io;\nimport foo from 'x';\nfrom y import z\npub fn api() {}\nexport const C: i32 = 1;\nfn private_fn() {}\n",
        )
        .unwrap();
        let module = module_fixture("src/lib.rs", Language::Rust);
        let ctx = build_context(&dir, &module).unwrap();
        assert_eq!(ctx.imports.len(), 3);
        assert!(ctx.imports[0].starts_with("use std::io"));
        assert_eq!(ctx.exports.len(), 2);
        assert!(ctx.exports[0].starts_with("pub fn api"));
        assert_eq!(ctx.project_conventions, "Default Rust conventions");
        assert!(ctx.dependents.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_context_reads_rustfmt_toml_conventions() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(dir.join("rustfmt.toml"), "max_width = 80\n").unwrap();
        let module = module_fixture("src/lib.rs", Language::Rust);
        let ctx = build_context(&dir, &module).unwrap();
        assert_eq!(ctx.project_conventions, "max_width = 80\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_context_reads_dot_rustfmt_toml_conventions() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(dir.join(".rustfmt.toml"), "hard_tabs = true\n").unwrap();
        let module = module_fixture("src/lib.rs", Language::Rust);
        let ctx = build_context(&dir, &module).unwrap();
        assert_eq!(ctx.project_conventions, "hard_tabs = true\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_context_missing_source_file_errors() {
        let dir = temp_dir();
        let module = module_fixture("src/missing.rs", Language::Rust);
        assert!(build_context(&dir, &module).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn execute_proposal_updates_status_timeline_and_changed_files() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn api() {}\nfn helper() {}\n").unwrap();
        let module = module_fixture("src/lib.rs", Language::Rust);
        let mut proposal = crate::queue::proposal_for(&module, RefactorKind::SplitModule);
        let output = execute_proposal(&MockRefactorEngine, &dir, &mut proposal, &module)
            .await
            .unwrap();
        assert_eq!(proposal.status, ProposalStatus::Refactoring);
        // One detection event from `proposal_for` plus three execution events.
        assert_eq!(proposal.timeline.len(), 4);
        assert!(proposal.timeline[1].message.contains("context"));
        assert!(proposal.timeline[3].message.contains("2 file(s)"));
        assert_eq!(proposal.changed_files.len(), output.files.len());
        assert_eq!(proposal.changed_files[0].path, output.files[0].0);
        assert_eq!(proposal.changed_files[0].content, output.files[0].1);
        assert_eq!(proposal.migration_notes, output.migration_notes);
        assert_eq!(
            proposal.diff_summary.lines_removed,
            output.diff_summary.lines_removed
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
