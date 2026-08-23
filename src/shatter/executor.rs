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
use crate::error::{Context, Result};
use super::context::{ShatterContext, ShatterConfig};
use super::{
    load_candidates, validate_candidate, PreconditionFailure, DependencyGraph,
    Move, MoveSequence,
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
    _ctx: &ShatterContext,
    sequence: MoveSequence,
) -> Result<Vec<PathBuf>> {
    // Phase 2: Implement actual move execution
    // For now, placeholder that shows intent

    let mut files_modified = Vec::new();

    for mv in sequence.moves {
        match mv {
            Move::ExtractFunction {
                source_file,
                target_file,
                ..
            } => {
                files_modified.push(source_file);
                files_modified.push(target_file);
            }
            Move::AddImport { file, .. } => {
                files_modified.push(file);
            }
            Move::RemoveImport { file, .. } => {
                files_modified.push(file);
            }
            Move::MakePublic { file, .. } => {
                files_modified.push(file);
            }
            Move::CreateReExport { file, .. } => {
                files_modified.push(file);
            }
        }
    }

    // Phase 2 will implement actual transformation here
    // For now, return success with files that would be modified

    Ok(files_modified)
}
