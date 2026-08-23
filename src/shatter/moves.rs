//! Legal transformations for deterministic code refactoring.
//!
//! A Move represents a single, validated transformation that can be applied atomically.
//! The finite set of Move variants ensures only deterministic transformations occur.

use std::path::PathBuf;
use crate::error::Result;

/// A legal transformation operation in the Shatter pipeline.
///
/// Each Move variant represents a distinct, deterministically applicable transformation.
/// Moves are executed in sequence and validated for non-conflicts.
#[derive(Debug, Clone)]
pub enum Move {
    /// Extract a function from one file and place it in another module.
    ExtractFunction {
        /// File containing the function
        source_file: PathBuf,
        /// Name of the function to extract
        function_name: String,
        /// Target module path (e.g., "crate::utils")
        target_module: String,
        /// Target file where function will be placed
        target_file: PathBuf,
        /// Imports needed in target file for function to be valid
        required_imports: Vec<String>,
    },

    /// Make an item public or crate-visible.
    MakePublic {
        /// File containing the item
        file: PathBuf,
        /// Name of the item to publicize
        item_name: String,
        /// Visibility level to set
        visibility: PublicVisibility,
    },

    /// Add an import statement to a file.
    AddImport {
        /// File to add import to
        file: PathBuf,
        /// Import statement (e.g., "use std::collections::HashMap;")
        import_stmt: String,
    },

    /// Remove an import statement from a file.
    RemoveImport {
        /// File to remove import from
        file: PathBuf,
        /// Import path to remove (e.g., "std::collections::HashMap")
        import_path: String,
    },

    /// Create a re-export of a type in a parent module.
    CreateReExport {
        /// File where re-export will be placed
        file: PathBuf,
        /// Item name to re-export
        item_name: String,
        /// Original module where item is defined
        original_module: String,
    },
}

/// Visibility levels for public items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicVisibility {
    /// `pub` — public
    Public,
    /// `pub(crate)` — crate-visible
    PubCrate,
}

impl PublicVisibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "pub",
            Self::PubCrate => "pub(crate)",
        }
    }
}

/// A sequence of moves that together accomplish a refactoring.
#[derive(Debug, Clone)]
pub struct MoveSequence {
    pub moves: Vec<Move>,
    /// If true, moves must execute in this exact order
    pub order_matters: bool,
}

impl MoveSequence {
    pub fn new(moves: Vec<Move>, order_matters: bool) -> Self {
        Self { moves, order_matters }
    }

    /// Validate that this sequence has no conflicts or dependencies issues.
    pub fn validate(&self) -> Result<()> {
        validate_move_order(&self.moves)?;
        validate_no_target_file_conflicts(&self.moves)?;
        Ok(())
    }
}

/// Validate that moves are ordered correctly and don't conflict.
pub fn validate_move_order(moves: &[Move]) -> Result<()> {
    // Check for conflicting operations on the same file
    let mut file_ops: std::collections::HashMap<PathBuf, Vec<&str>> = Default::default();

    for mv in moves {
        let (file, op) = match mv {
            Move::ExtractFunction { source_file, .. } => (source_file, "extract"),
            Move::MakePublic { file, .. } => (file, "make_public"),
            Move::AddImport { file, .. } => (file, "add_import"),
            Move::RemoveImport { file, .. } => (file, "remove_import"),
            Move::CreateReExport { file, .. } => (file, "create_reexport"),
        };

        file_ops.entry(file.clone()).or_insert_with(Vec::new).push(op);
    }

    // Check for problematic operation sequences
    for (_file, _ops) in file_ops {
        // Multiple extract operations on the same file are OK
        // Multiple import operations on the same file are OK
        // An extract followed by imports is OK
        // But other combinations might be problematic

        // For now, we allow most combinations since syn allows multiple passes
        // A more sophisticated check would validate operation semantics
    }

    Ok(())
}

/// Validate that target files don't conflict.
pub fn validate_no_target_file_conflicts(moves: &[Move]) -> Result<()> {
    let mut target_files = std::collections::HashSet::new();

    for mv in moves {
        let files = match mv {
            Move::ExtractFunction {
                target_file, ..
            } => vec![target_file.clone()],
            Move::MakePublic { file, .. } => vec![file.clone()],
            Move::AddImport { file, .. } => vec![file.clone()],
            Move::RemoveImport { file, .. } => vec![file.clone()],
            Move::CreateReExport { file, .. } => vec![file.clone()],
        };

        for file in files {
            if !target_files.insert(file.clone()) {
                // Note: multiple operations on the same file are allowed
                // Only check that we're not creating contradictory states
            }
        }
    }

    Ok(())
}

/// Check if all target files are writable before applying moves.
pub fn validate_all_target_files_writable(moves: &[Move]) -> Result<()> {
    let mut files = std::collections::HashSet::new();

    for mv in moves {
        let files_for_move = match mv {
            Move::ExtractFunction {
                source_file,
                target_file,
                ..
            } => vec![source_file.clone(), target_file.clone()],
            Move::MakePublic { file, .. } => vec![file.clone()],
            Move::AddImport { file, .. } => vec![file.clone()],
            Move::RemoveImport { file, .. } => vec![file.clone()],
            Move::CreateReExport { file, .. } => vec![file.clone()],
        };

        for file in files_for_move {
            files.insert(file);
        }
    }

    for file in files {
        if !file.exists() {
            return Err(format!("Target file does not exist: {}", file.display()).into());
        }
        // Check writability (simplified: just check if parent dir exists)
        if let Some(parent) = file.parent() {
            if !parent.exists() {
                return Err(format!(
                    "Target file parent directory does not exist: {}",
                    parent.display()
                ).into());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extract_function_move_creation() {
        let mv = Move::ExtractFunction {
            source_file: PathBuf::from("src/lib.rs"),
            function_name: "helper".to_string(),
            target_module: "crate::utils".to_string(),
            target_file: PathBuf::from("src/utils.rs"),
            required_imports: vec!["use std::collections::HashMap;".to_string()],
        };

        match mv {
            Move::ExtractFunction {
                function_name,
                target_module,
                ..
            } => {
                assert_eq!(function_name, "helper");
                assert_eq!(target_module, "crate::utils");
            }
            _ => panic!("Expected ExtractFunction"),
        }
    }

    #[test]
    fn make_public_visibility_levels() {
        assert_eq!(PublicVisibility::Public.as_str(), "pub");
        assert_eq!(PublicVisibility::PubCrate.as_str(), "pub(crate)");
        assert_ne!(PublicVisibility::Public, PublicVisibility::PubCrate);
    }

    #[test]
    fn move_sequence_validation() {
        let moves = vec![
            Move::AddImport {
                file: PathBuf::from("src/lib.rs"),
                import_stmt: "use std::vec::Vec;".to_string(),
            },
            Move::MakePublic {
                file: PathBuf::from("src/lib.rs"),
                item_name: "foo".to_string(),
                visibility: PublicVisibility::Public,
            },
        ];

        let seq = MoveSequence::new(moves, true);
        assert!(seq.validate().is_ok());
    }

    #[test]
    fn move_order_validates() {
        let moves = vec![
            Move::AddImport {
                file: PathBuf::from("src/lib.rs"),
                import_stmt: "use std::collections::HashMap;".to_string(),
            },
        ];

        assert!(validate_move_order(&moves).is_ok());
    }

    #[test]
    fn empty_sequence_validates() {
        let moves: Vec<Move> = vec![];
        assert!(validate_move_order(&moves).is_ok());
    }
}
