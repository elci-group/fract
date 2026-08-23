//! Main orchestration of the shatter transformation pipeline.
//!
//! Coordinates the full workflow:
//! 1. Load candidates from file
//! 2. Build project dependency graph
//! 3. Validate candidates meet preconditions
//! 4. Plan transformations (moves)
//! 5. Execute moves transactionally
//! 6. Validate with cargo check/test

use std::path::PathBuf;
use std::fs;
use std::collections::HashMap;
use crate::error::{Context, Result};
use super::context::{ShatterContext, ShatterConfig};
use super::{
    load_candidates, validate_candidate, PreconditionFailure, DependencyGraph,
    Move, MoveSequence, AstRewriter, Transaction, FileChange,
};

/// Report on the outcome of a shatter operation.
#[derive(Debug, Clone)]
pub struct ShatterReport {
    pub candidates_attempted: usize,
    pub candidates_succeeded: usize,
    pub candidates_failed: usize,
    pub files_modified: Vec<PathBuf>,
    pub errors: Vec<String>,
}

impl ShatterReport {
    pub fn new() -> Self {
        Self {
            candidates_attempted: 0,
            candidates_succeeded: 0,
            candidates_failed: 0,
            files_modified: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn add_error(&mut self, error: String) {
        self.errors.push(error);
    }

    pub fn add_success(&mut self, files: Vec<PathBuf>) {
        self.candidates_succeeded += 1;
        self.files_modified.extend(files);
    }

    pub fn add_failure(&mut self, error: String) {
        self.candidates_failed += 1;
        self.errors.push(error);
    }
}

impl Default for ShatterReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Execute shatter transformations on the given project.
pub async fn execute_shatter(
    root: PathBuf,
    candidates_file: PathBuf,
    skip_validation: bool,
    dry_run: bool,
) -> Result<ShatterReport> {
    // Validate candidates file exists
    if !candidates_file.exists() {
        return Err(format!(
            "Candidates file not found: {}",
            candidates_file.display()
        ).into());
    }

    let config = ShatterConfig {
        skip_validation,
        dry_run,
    };

    let ctx = ShatterContext::new(root.clone(), config)?;

    // Load candidates from file
    let candidates = load_candidates(&candidates_file)
        .context("loading candidates")?;

    if candidates.is_empty() {
        return Err("No candidates found in file".into());
    }

    let mut report = ShatterReport::new();
    report.candidates_attempted = candidates.len();

    // Build dependency graph
    let graph = DependencyGraph::build(&root)
        .context("building dependency graph")?;

    // Process each candidate
    for candidate in candidates {
        // Step 1: Validate preconditions
        match validate_candidate(&candidate) {
            Err(PreconditionFailure::SyntaxError(e)) => {
                report.add_failure(format!(
                    "Candidate {}: syntax error: {}",
                    candidate.function_name, e
                ));
                continue;
            }
            Err(failure) => {
                report.add_failure(format!(
                    "Candidate {}: validation failed: {}",
                    candidate.function_name, failure
                ));
                continue;
            }
            Ok(()) => {}
        }

        // Step 2: Verify extraction safety
        let fn_id = super::FunctionId::new(
            candidate.file.clone(),
            candidate.function_name.clone(),
        );

        if let Err(e) = graph.validate_extraction_safe(&fn_id) {
            report.add_failure(format!(
                "Candidate {}: extraction unsafe: {}",
                candidate.function_name, e
            ));
            continue;
        }

        // Step 3: Generate moves
        let required_imports = match graph.imports_needed(&fn_id, &candidate.target_module) {
            Ok(imports) => imports,
            Err(e) => {
                report.add_failure(format!(
                    "Candidate {}: import inference failed: {}",
                    candidate.function_name, e
                ));
                continue;
            }
        };

        let moves = vec![
            Move::ExtractFunction {
                source_file: candidate.file.clone(),
                function_name: candidate.function_name.clone(),
                target_module: candidate.target_module.clone(),
                target_file: candidate.target_file.clone(),
                required_imports,
            },
        ];

        let sequence = MoveSequence::new(moves, false);

        // Step 4: Validate move sequence
        if let Err(e) = sequence.validate() {
            report.add_failure(format!(
                "Candidate {}: move validation failed: {}",
                candidate.function_name, e
            ));
            continue;
        }

        // Step 5: If dry-run, just report the plan
        if dry_run {
            report.add_success(vec![
                candidate.file.clone(),
                candidate.target_file.clone(),
            ]);
            continue;
        }

        // Step 6: Execute transformations transactionally
        match execute_moves(&ctx, sequence).await {
            Ok(files) => {
                report.add_success(files);
            }
            Err(e) => {
                report.add_failure(format!(
                    "Candidate {}: execution failed: {}",
                    candidate.function_name, e
                ));
            }
        }
    }

    Ok(report)
}

/// Execute a sequence of moves transactionally.
async fn execute_moves(
    ctx: &ShatterContext,
    sequence: MoveSequence,
) -> Result<Vec<PathBuf>> {
    let mut tx = Transaction::new(ctx.root.clone())?;
    let rewriter = AstRewriter::new(ctx.root.clone());
    let mut files_modified = Vec::new();

    // Collect files to read before starting transaction
    let mut file_contents: HashMap<PathBuf, String> = HashMap::new();
    for mv in &sequence.moves {
        let files = move_files(mv);
        for file in files {
            if !file_contents.contains_key(&file) {
                let content = fs::read_to_string(&file)
                    .context(format!("reading file for transaction: {}", file.display()))?;
                file_contents.insert(file, content);
            }
        }
    }

    // Apply each move in sequence
    for mv in sequence.moves {
        match mv {
            Move::ExtractFunction {
                source_file,
                target_file,
                function_name,
                required_imports,
                ..
            } => {
                let source_content = file_contents.get(&source_file)
                    .cloned()
                    .ok_or_else(|| format!("source file not in cache: {}", source_file.display()))?;

                let target_content = file_contents.get(&target_file)
                    .cloned()
                    .unwrap_or_default();

                let (new_source, new_target) = rewriter.extract_function(
                    &source_file,
                    &function_name,
                    &target_file,
                    None,
                )?;

                // Add required imports to target file
                let mut final_target = new_target;
                for import in required_imports {
                    final_target = rewriter.add_import(&final_target, &import)?;
                }

                tx.stage(FileChange::new(
                    source_file.clone(),
                    source_content,
                    new_source,
                ))?;

                tx.stage(FileChange::new(
                    target_file.clone(),
                    target_content,
                    final_target,
                ))?;

                files_modified.push(source_file);
                files_modified.push(target_file);
            }

            Move::AddImport { file, import_stmt } => {
                let content = file_contents.get(&file)
                    .cloned()
                    .ok_or_else(|| format!("file not in cache: {}", file.display()))?;

                let new_content = rewriter.add_import(&content, &import_stmt)?;

                tx.stage(FileChange::new(file.clone(), content, new_content))?;
                files_modified.push(file);
            }

            Move::RemoveImport { file, import_path } => {
                let content = file_contents.get(&file)
                    .cloned()
                    .ok_or_else(|| format!("file not in cache: {}", file.display()))?;

                let new_content = rewriter.remove_import(&content, &import_path)?;

                tx.stage(FileChange::new(file.clone(), content, new_content))?;
                files_modified.push(file);
            }

            Move::MakePublic { file, item_name, visibility } => {
                let content = file_contents.get(&file)
                    .cloned()
                    .ok_or_else(|| format!("file not in cache: {}", file.display()))?;

                let new_content = rewriter.make_public(&content, &item_name, visibility)?;

                tx.stage(FileChange::new(file.clone(), content, new_content))?;
                files_modified.push(file);
            }

            Move::CreateReExport { file, item_name, original_module } => {
                let content = file_contents.get(&file)
                    .cloned()
                    .ok_or_else(|| format!("file not in cache: {}", file.display()))?;

                let new_content = rewriter.create_reexport(&content, &item_name, &original_module)?;

                tx.stage(FileChange::new(file.clone(), content, new_content))?;
                files_modified.push(file);
            }
        }
    }

    // Apply all changes atomically
    tx.apply()?;

    // Validate with cargo check
    if !ctx.config.skip_validation {
        let validation = tx.validate().await?;
        if !validation.all_passed() {
            tx.rollback()?;
            let error_msg = validation.logs.join("; ");
            return Err(format!("validation failed: {}", error_msg).into());
        }
    }

    // Commit the transaction
    tx.commit()?;

    Ok(files_modified)
}

/// Get all files touched by a move.
fn move_files(mv: &Move) -> Vec<PathBuf> {
    match mv {
        Move::ExtractFunction { source_file, target_file, .. } => {
            vec![source_file.clone(), target_file.clone()]
        }
        Move::AddImport { file, .. } => vec![file.clone()],
        Move::RemoveImport { file, .. } => vec![file.clone()],
        Move::MakePublic { file, .. } => vec![file.clone()],
        Move::CreateReExport { file, .. } => vec![file.clone()],
    }
}
