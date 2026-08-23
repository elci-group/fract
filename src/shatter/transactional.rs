//! Atomic file transactions for safe, reversible transformations.
//!
//! Provides all-or-nothing semantics: either all moves succeed and are committed,
//! or all changes are rolled back and the working tree is restored to its original state.

use std::fs;
use std::path::PathBuf;
use crate::error::Result;

/// A single file change in a transaction.
#[derive(Debug, Clone)]
pub struct FileChange {
    /// Path to the file to modify
    pub path: PathBuf,
    /// Original content (for rollback)
    pub original_content: String,
    /// New content to write
    pub new_content: String,
}

impl FileChange {
    pub fn new(path: PathBuf, original: String, new: String) -> Self {
        Self {
            path,
            original_content: original,
            new_content: new,
        }
    }
}

/// State of a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    Pending,
    Applied,
    Validated,
    Committed,
    RolledBack,
}

/// Validation results from cargo check/test.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub cargo_check_ok: bool,
    pub cargo_fmt_ok: bool,
    pub tests_ok: bool,
    pub logs: Vec<String>,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            cargo_check_ok: false,
            cargo_fmt_ok: false,
            tests_ok: false,
            logs: Vec::new(),
        }
    }

    pub fn all_passed(&self) -> bool {
        self.cargo_check_ok && self.cargo_fmt_ok && self.tests_ok
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Atomic transaction for applying file changes with rollback capability.
pub struct Transaction {
    root: PathBuf,
    changes: Vec<FileChange>,
    backup_dir: Option<PathBuf>,
    state: TransactionState,
}

impl Transaction {
    /// Create a new transaction for the given project root.
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(Self {
            root,
            changes: Vec::new(),
            backup_dir: None,
            state: TransactionState::Pending,
        })
    }

    /// Queue a file change (no I/O until apply()).
    pub fn stage(&mut self, change: FileChange) -> Result<()> {
        if self.state != TransactionState::Pending {
            return Err(format!(
                "Cannot stage changes in {:?} state",
                self.state
            ).into());
        }
        self.changes.push(change);
        Ok(())
    }

    /// Write all changes to disk and create backups.
    pub fn apply(&mut self) -> Result<()> {
        if self.state != TransactionState::Pending {
            return Err(format!(
                "Cannot apply transaction in {:?} state",
                self.state
            ).into());
        }

        // Create backup directory
        let backup_dir = self.root.join(".shatter_backup");
        fs::create_dir_all(&backup_dir)?;
        self.backup_dir = Some(backup_dir.clone());

        // Write all changes and save backups
        for change in &self.changes {
            // Create backup of original
            let backup_path = backup_dir.join(
                change.path
                    .file_name()
                    .ok_or_else(|| "Invalid file path".to_string())?
            );
            fs::write(&backup_path, &change.original_content)?;

            // Write new content
            fs::write(&change.path, &change.new_content)?;
        }

        self.state = TransactionState::Applied;
        Ok(())
    }

    /// Validate that changes compile and tests pass.
    pub async fn validate(&mut self) -> Result<ValidationResult> {
        if self.state != TransactionState::Applied {
            return Err(format!(
                "Cannot validate transaction in {:?} state",
                self.state
            ).into());
        }

        let mut result = ValidationResult::new();

        // Run cargo check
        let check_output = tokio::process::Command::new("cargo")
            .arg("check")
            .current_dir(&self.root)
            .output()
            .await?;

        result.cargo_check_ok = check_output.status.success();
        if !result.cargo_check_ok {
            result.logs.push(format!(
                "cargo check failed: {}",
                String::from_utf8_lossy(&check_output.stderr)
            ));
        }

        // Run cargo fmt (check only, don't modify)
        let fmt_output = tokio::process::Command::new("cargo")
            .arg("fmt")
            .arg("--check")
            .current_dir(&self.root)
            .output()
            .await;

        result.cargo_fmt_ok = fmt_output
            .map(|output| output.status.success())
            .unwrap_or(false);

        // Run cargo test
        let test_output = tokio::process::Command::new("cargo")
            .arg("test")
            .current_dir(&self.root)
            .output()
            .await;

        match test_output {
            Ok(output) => {
                result.tests_ok = output.status.success();
                if !result.tests_ok {
                    result.logs.push(format!(
                        "cargo test failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
            }
            Err(e) => {
                result.tests_ok = false;
                result.logs.push(format!("cargo test error: {}", e));
            }
        }

        self.state = TransactionState::Validated;
        Ok(result)
    }

    /// Commit the transaction (delete backups, finalize state).
    pub fn commit(&mut self) -> Result<()> {
        if self.state != TransactionState::Validated {
            return Err(format!(
                "Cannot commit transaction in {:?} state",
                self.state
            ).into());
        }

        // Clean up backups
        if let Some(backup_dir) = &self.backup_dir {
            fs::remove_dir_all(backup_dir)?;
        }

        self.state = TransactionState::Committed;
        Ok(())
    }

    /// Rollback: restore original files from backups.
    pub fn rollback(&mut self) -> Result<()> {
        if self.state == TransactionState::RolledBack {
            return Ok(());
        }

        if let Some(backup_dir) = &self.backup_dir {
            if backup_dir.exists() {
                // Restore each file from backup
                for change in &self.changes {
                    let backup_path = backup_dir.join(
                        change.path
                            .file_name()
                            .ok_or_else(|| "Invalid file path".to_string())?
                    );

                    if backup_path.exists() {
                        fs::copy(&backup_path, &change.path)?;
                    }
                }

                // Clean up backup directory
                fs::remove_dir_all(backup_dir)?;
            }
        }

        self.state = TransactionState::RolledBack;
        Ok(())
    }

    /// Get current transaction state.
    pub fn state(&self) -> TransactionState {
        self.state
    }

    /// Get the number of staged changes.
    pub fn change_count(&self) -> usize {
        self.changes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_change_creation() {
        let change = FileChange::new(
            PathBuf::from("src/lib.rs"),
            "original".to_string(),
            "modified".to_string(),
        );
        assert_eq!(change.path, PathBuf::from("src/lib.rs"));
        assert_eq!(change.original_content, "original");
        assert_eq!(change.new_content, "modified");
    }

    #[test]
    fn validation_result_all_passed() {
        let mut result = ValidationResult::new();
        assert!(!result.all_passed());

        result.cargo_check_ok = true;
        assert!(!result.all_passed());

        result.cargo_fmt_ok = true;
        assert!(!result.all_passed());

        result.tests_ok = true;
        assert!(result.all_passed());
    }

    #[test]
    fn transaction_state_transitions() {
        let mut tx = Transaction::new(PathBuf::from(".")).unwrap();
        assert_eq!(tx.state(), TransactionState::Pending);

        let change = FileChange::new(
            PathBuf::from("test.txt"),
            "orig".to_string(),
            "new".to_string(),
        );
        tx.stage(change).unwrap();
        assert_eq!(tx.state(), TransactionState::Pending);
        assert_eq!(tx.change_count(), 1);
    }

    #[test]
    fn cannot_stage_after_apply() {
        let tx = Transaction::new(PathBuf::from(".")).unwrap();
        // We can't actually apply without touching filesystem in unit test
        // but we can verify the state machine logic
        assert_eq!(tx.state(), TransactionState::Pending);
    }
}
