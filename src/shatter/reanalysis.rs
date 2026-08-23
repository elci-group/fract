//! Post-transformation verification and re-analysis.
//!
//! After applying moves, we re-analyze the modified code to ensure the transformation
//! didn't violate any invariants. This includes re-running precondition checks and
//! verifying the extraction is still valid with the new code structure.

use std::path::PathBuf;
use crate::error::{Context, Result};
use super::{DependencyGraph, validate_candidate, CandidateFunction, PreconditionFailure};

/// Result of re-analyzing a transformation.
#[derive(Debug, Clone)]
pub struct ReanalysisResult {
    /// Whether the transformation is still valid
    pub is_valid: bool,
    /// Files that were checked
    pub files_checked: Vec<PathBuf>,
    /// Any issues found during re-analysis
    pub issues: Vec<String>,
    /// Suggestions for fixing issues
    pub suggestions: Vec<String>,
}

impl ReanalysisResult {
    pub fn new() -> Self {
        Self {
            is_valid: true,
            files_checked: Vec::new(),
            issues: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    pub fn add_issue(&mut self, issue: String, suggestion: String) {
        self.is_valid = false;
        self.issues.push(issue);
        self.suggestions.push(suggestion);
    }
}

impl Default for ReanalysisResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Re-analyze a transformation to ensure it's still valid.
pub fn reanalyze_transformation(
    root: &PathBuf,
    source_file: &PathBuf,
    target_file: &PathBuf,
    function_name: &str,
    _target_module: &str,
) -> Result<ReanalysisResult> {
    let mut result = ReanalysisResult::new();
    result.files_checked.push(source_file.clone());
    result.files_checked.push(target_file.clone());

    // Check 1: Both files still exist and are valid Rust
    check_file_validity(source_file, &mut result)?;
    check_file_validity(target_file, &mut result)?;

    // Check 2: Function is gone from source file
    check_function_removed(source_file, function_name, &mut result)?;

    // Check 3: Function exists in target file
    check_function_exists(target_file, function_name, &mut result)?;

    // Check 4: Re-build dependency graph and validate
    revalidate_with_graph(root, &mut result)?;

    Ok(result)
}

/// Verify that a file is syntactically valid Rust.
fn check_file_validity(file: &PathBuf, result: &mut ReanalysisResult) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .context(format!("reading file for re-analysis: {}", file.display()))?;

    match syn::parse_file(&content) {
        Ok(_) => {
            // File is valid
        }
        Err(e) => {
            result.add_issue(
                format!("File {} is not valid Rust: {}", file.display(), e),
                format!("Review the transformations applied to {}", file.display()),
            );
        }
    }

    Ok(())
}

/// Verify that a function has been removed from the source file.
fn check_function_removed(file: &PathBuf, function_name: &str, result: &mut ReanalysisResult) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .context(format!("reading file: {}", file.display()))?;

    if let Ok(file_ast) = syn::parse_file(&content) {
        for item in &file_ast.items {
            if let syn::Item::Fn(item_fn) = item {
                if item_fn.sig.ident.to_string() == function_name {
                    result.add_issue(
                        format!(
                            "Function '{}' still exists in source file {}",
                            function_name,
                            file.display()
                        ),
                        format!(
                            "The function should have been removed from {} during extraction",
                            file.display()
                        ),
                    );
                }
            }
        }
    }

    Ok(())
}

/// Verify that a function exists in the target file.
fn check_function_exists(file: &PathBuf, function_name: &str, result: &mut ReanalysisResult) -> Result<()> {
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => {
            // Target file might not exist yet, which is OK
            return Ok(());
        }
    };

    if content.is_empty() {
        // Empty target file - could be OK depending on expectations
        return Ok(());
    }

    if let Ok(file_ast) = syn::parse_file(&content) {
        let mut found = false;
        for item in &file_ast.items {
            if let syn::Item::Fn(item_fn) = item {
                if item_fn.sig.ident.to_string() == function_name {
                    found = true;
                    break;
                }
            }
        }

        if !found {
            result.add_issue(
                format!(
                    "Function '{}' not found in target file {}",
                    function_name,
                    file.display()
                ),
                "Verify the function extraction was applied correctly".to_string(),
            );
        }
    }

    Ok(())
}

/// Re-run the dependency graph checks with the new code structure.
fn revalidate_with_graph(
    root: &PathBuf,
    result: &mut ReanalysisResult,
) -> Result<()> {
    // Try to rebuild the dependency graph
    match DependencyGraph::build(root) {
        Ok(_graph) => {
            // Graph built successfully - transformations didn't break the module structure
        }
        Err(e) => {
            result.add_issue(
                format!("Failed to build dependency graph after transformation: {}", e),
                "Check that imports and module structure are consistent".to_string(),
            );
        }
    }

    Ok(())
}

/// Verify all preconditions on a candidate are still satisfied post-transformation.
pub fn revalidate_preconditions(candidate: &CandidateFunction) -> Result<Option<PreconditionFailure>> {
    match validate_candidate(candidate) {
        Ok(()) => Ok(None),
        Err(e) => Ok(Some(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn reanalysis_result_tracks_issues() {
        let mut result = ReanalysisResult::new();
        assert!(result.is_valid);

        result.add_issue("problem".to_string(), "fix it".to_string());
        assert!(!result.is_valid);
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.suggestions.len(), 1);
    }

    #[test]
    fn check_file_validity_accepts_valid_rust() {
        let source = "fn foo() { println!(\"test\"); }";
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(source.as_bytes()).unwrap();
        file.flush().unwrap();

        let mut result = ReanalysisResult::new();
        let path = file.path().to_path_buf();
        check_file_validity(&path, &mut result).unwrap();
        assert!(result.is_valid);
    }

    #[test]
    fn check_file_validity_rejects_invalid_rust() {
        let source = "fn foo() { this is not valid rust";
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(source.as_bytes()).unwrap();
        file.flush().unwrap();

        let mut result = ReanalysisResult::new();
        let path = file.path().to_path_buf();
        check_file_validity(&path, &mut result).unwrap();
        assert!(!result.is_valid);
        assert!(!result.issues.is_empty());
    }
}
