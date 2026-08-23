//! Compiler error analysis and repair suggestions.
//!
//! When cargo check or cargo test fails, this module analyzes the error diagnostics
//! and provides structured feedback to help diagnose what went wrong with the transformation.

use std::path::PathBuf;
use crate::error::Result;

/// Type of compilation error detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompilationErrorKind {
    /// Undeclared type or function reference.
    UnresolvedReference(String),
    /// Missing import statement.
    MissingImport(String),
    /// Type mismatch in extracted function.
    TypeMismatch(String),
    /// Lifetime mismatch or missing lifetime bound.
    LifetimeMismatch(String),
    /// Access to private item.
    PrivacyViolation(String),
    /// Borrow checker issue.
    BorrowChecker(String),
    /// Macro expansion failure.
    MacroExpansion(String),
    /// Generic error that doesn't fit other categories.
    Other(String),
}

impl std::fmt::Display for CompilationErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnresolvedReference(msg) => write!(f, "Unresolved reference: {}", msg),
            Self::MissingImport(msg) => write!(f, "Missing import: {}", msg),
            Self::TypeMismatch(msg) => write!(f, "Type mismatch: {}", msg),
            Self::LifetimeMismatch(msg) => write!(f, "Lifetime mismatch: {}", msg),
            Self::PrivacyViolation(msg) => write!(f, "Privacy violation: {}", msg),
            Self::BorrowChecker(msg) => write!(f, "Borrow checker: {}", msg),
            Self::MacroExpansion(msg) => write!(f, "Macro expansion: {}", msg),
            Self::Other(msg) => write!(f, "Compilation error: {}", msg),
        }
    }
}

/// Detailed information about a compilation error.
#[derive(Debug, Clone)]
pub struct CompilationError {
    pub kind: CompilationErrorKind,
    pub file: PathBuf,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

impl CompilationError {
    pub fn new(kind: CompilationErrorKind, file: PathBuf, message: String) -> Self {
        Self {
            kind,
            file,
            line: None,
            column: None,
            message,
        }
    }
}

/// Analysis of cargo check/test output.
#[derive(Debug, Clone)]
pub struct CompilerDiagnostics {
    pub errors: Vec<CompilationError>,
    pub warnings: Vec<String>,
    pub raw_output: String,
}

impl CompilerDiagnostics {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            raw_output: String::new(),
        }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
}

impl Default for CompilerDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

/// Analyze cargo check output and extract structured error information.
pub fn analyze_compiler_output(output: &str) -> Result<CompilerDiagnostics> {
    let mut diagnostics = CompilerDiagnostics::new();
    diagnostics.raw_output = output.to_string();

    for line in output.lines() {
        // Parse error: unresolved name
        if line.contains("cannot find") && (line.contains("function") || line.contains("type")) {
            let kind = if line.contains("function") {
                CompilationErrorKind::UnresolvedReference(extract_name(line).unwrap_or_default())
            } else {
                CompilationErrorKind::UnresolvedReference(extract_name(line).unwrap_or_default())
            };
            diagnostics.errors.push(CompilationError::new(
                kind,
                PathBuf::from("unknown"),
                line.to_string(),
            ));
        }

        // Parse error: type mismatch
        if line.contains("expected") && line.contains("found") {
            let kind = CompilationErrorKind::TypeMismatch(line.to_string());
            diagnostics.errors.push(CompilationError::new(
                kind,
                PathBuf::from("unknown"),
                line.to_string(),
            ));
        }

        // Parse error: lifetime
        if line.contains("lifetime") {
            let kind = CompilationErrorKind::LifetimeMismatch(line.to_string());
            diagnostics.errors.push(CompilationError::new(
                kind,
                PathBuf::from("unknown"),
                line.to_string(),
            ));
        }

        // Parse error: privacy
        if line.contains("private") {
            let kind = CompilationErrorKind::PrivacyViolation(line.to_string());
            diagnostics.errors.push(CompilationError::new(
                kind,
                PathBuf::from("unknown"),
                line.to_string(),
            ));
        }

        // Parse warning
        if line.contains("warning:") {
            diagnostics.warnings.push(line.to_string());
        }
    }

    Ok(diagnostics)
}

/// Extract identifier/name from error message.
fn extract_name(msg: &str) -> Option<String> {
    let parts: Vec<&str> = msg.split('`').collect();
    if parts.len() >= 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}

/// Suggest repair actions for a compilation error.
pub fn suggest_repair(error: &CompilationError) -> Vec<String> {
    let mut suggestions = Vec::new();

    match &error.kind {
        CompilationErrorKind::UnresolvedReference(name) => {
            suggestions.push(format!(
                "Unresolved reference '{}': Did you forget to import it in the target file?",
                name
            ));
            suggestions.push("Check the dependency graph to verify this item is available in the target module.".to_string());
        }
        CompilationErrorKind::MissingImport(path) => {
            suggestions.push(format!("Add import: use {};", path));
        }
        CompilationErrorKind::TypeMismatch(_) => {
            suggestions.push(
                "Check that the function signature matches expected types in the target module.".to_string(),
            );
            suggestions.push("Verify generic type parameters are properly constrained.".to_string());
        }
        CompilationErrorKind::LifetimeMismatch(_) => {
            suggestions.push(
                "Verify lifetime bounds match between source and target contexts.".to_string(),
            );
        }
        CompilationErrorKind::PrivacyViolation(item) => {
            suggestions.push(format!(
                "Make '{}' public or crate-visible if needed in target module.",
                item
            ));
        }
        CompilationErrorKind::BorrowChecker(_) => {
            suggestions.push("Verify the function doesn't have unexpected ownership issues.".to_string());
            suggestions.push("Check that moved/borrowed references are properly handled.".to_string());
        }
        CompilationErrorKind::MacroExpansion(_) => {
            suggestions.push(
                "Macros may need special handling. Check if macro definitions moved with the function.".to_string(),
            );
        }
        CompilationErrorKind::Other(_) => {
            suggestions.push("Review the full error message and cargo check output.".to_string());
        }
    }

    suggestions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_unresolved_reference() {
        let output = "error[E0425]: cannot find function `helper` in this scope";
        let diag = analyze_compiler_output(output).unwrap();
        assert_eq!(diag.error_count(), 1);
        assert!(matches!(
            diag.errors[0].kind,
            CompilationErrorKind::UnresolvedReference(_)
        ));
    }

    #[test]
    fn analyze_type_mismatch() {
        let output = "error[E0308]: expected `i32`, found `String`";
        let diag = analyze_compiler_output(output).unwrap();
        assert!(diag.errors.iter().any(|e| matches!(
            e.kind,
            CompilationErrorKind::TypeMismatch(_)
        )));
    }

    #[test]
    fn suggest_privacy_repair() {
        let error = CompilationError::new(
            CompilationErrorKind::PrivacyViolation("foo".to_string()),
            PathBuf::from("src/lib.rs"),
            "private item".to_string(),
        );
        let suggestions = suggest_repair(&error);
        assert!(suggestions.iter().any(|s| s.contains("public")));
    }
}
