//! Persistence and recovery for batch operations.
//!
//! Supports checkpointing, transaction logs, and recovery from failures
//! to enable reliable large-scale transformations.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::error::Result;
use tracing::{info, warn, error, debug};

/// A checkpoint for a batch operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub batch_id: String,
    pub created_at: DateTime<Utc>,
    pub candidates_processed: usize,
    pub candidates_total: usize,
    pub candidates: Vec<CheckpointCandidate>,
    pub status: CheckpointStatus,
}

/// Status of a checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckpointStatus {
    InProgress,
    Paused,
    Completed,
    Failed,
    RolledBack,
}

/// Candidate state in a checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointCandidate {
    pub name: String,
    pub status: CandidateStatus,
    pub error: Option<String>,
}

/// Status of a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateStatus {
    Pending,
    Processing,
    Succeeded,
    Failed,
}

impl Checkpoint {
    pub fn new(batch_id: String, total: usize) -> Self {
        info!(batch_id = %batch_id, total = total, "Creating checkpoint");
        Self {
            batch_id,
            created_at: Utc::now(),
            candidates_processed: 0,
            candidates_total: total,
            candidates: Vec::new(),
            status: CheckpointStatus::InProgress,
        }
    }

    pub fn add_candidate(&mut self, name: String) {
        self.candidates.push(CheckpointCandidate {
            name,
            status: CandidateStatus::Pending,
            error: None,
        });
    }

    pub fn update_candidate(&mut self, name: &str, status: CandidateStatus, error: Option<String>) {
        if let Some(candidate) = self.candidates.iter_mut().find(|c| c.name == name) {
            candidate.status = status;
            candidate.error = error;
            if status == CandidateStatus::Succeeded || status == CandidateStatus::Failed {
                self.candidates_processed += 1;
            }
            debug!(candidate = %name, status = ?status, "Candidate status updated");
        }
    }

    pub fn mark_complete(&mut self) {
        self.status = CheckpointStatus::Completed;
        info!(batch_id = %self.batch_id, "Checkpoint marked complete");
    }

    pub fn mark_failed(&mut self, reason: &str) {
        self.status = CheckpointStatus::Failed;
        error!(batch_id = %self.batch_id, reason = %reason, "Checkpoint marked failed");
    }
}

/// Transaction log for audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionLog {
    pub batch_id: String,
    pub entries: Vec<LogEntry>,
}

/// Log entry for a single operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub operation: String,
    pub status: String,
    pub details: String,
    pub files_affected: Vec<PathBuf>,
}

impl TransactionLog {
    pub fn new(batch_id: String) -> Self {
        Self {
            batch_id,
            entries: Vec::new(),
        }
    }

    pub fn add_entry(
        &mut self,
        operation: String,
        status: String,
        details: String,
        files_affected: Vec<PathBuf>,
    ) {
        debug!(operation = %operation, "Transaction log entry added");
        self.entries.push(LogEntry {
            timestamp: Utc::now(),
            operation,
            status,
            details,
            files_affected,
        });
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize transaction log: {}", e))?;
        std::fs::write(path, content)?;
        info!(path = %path.display(), "Transaction log saved");
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                error!(path = %path.display(), error = %e, "Failed to read transaction log");
                return Err(format!("Failed to read transaction log: {}", e).into());
            }
        };
        let log: TransactionLog = match serde_json::from_str(&content) {
            Ok(l) => l,
            Err(e) => {
                error!(path = %path.display(), error = %e, "Failed to deserialize transaction log");
                return Err(format!("Failed to deserialize transaction log: {}", e).into());
            }
        };
        info!(path = %path.display(), entries = log.entries.len(), "Transaction log loaded");
        Ok(log)
    }
}

/// Persistence manager for batch operations.
pub struct PersistenceManager {
    checkpoint_dir: PathBuf,
}

impl PersistenceManager {
    pub fn new(checkpoint_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&checkpoint_dir)?;
        info!(path = %checkpoint_dir.display(), "Persistence manager initialized");
        Ok(Self { checkpoint_dir })
    }

    /// Save checkpoint to disk.
    pub fn save_checkpoint(&self, checkpoint: &Checkpoint) -> Result<()> {
        let path = self.checkpoint_dir.join(format!("{}.ckpt.json", checkpoint.batch_id));
        let content = serde_json::to_string_pretty(checkpoint)
            .map_err(|e| format!("Failed to serialize checkpoint: {}", e))?;
        std::fs::write(&path, content)?;
        info!(path = %path.display(), "Checkpoint saved");
        Ok(())
    }

    /// Load checkpoint from disk.
    pub fn load_checkpoint(&self, batch_id: &str) -> Result<Option<Checkpoint>> {
        let path = self.checkpoint_dir.join(format!("{}.ckpt.json", batch_id));
        if !path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&path)?;
        let checkpoint: Checkpoint = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to deserialize checkpoint: {}", e))?;
        info!(path = %path.display(), "Checkpoint loaded");
        Ok(Some(checkpoint))
    }

    /// List all available checkpoints.
    pub fn list_checkpoints(&self) -> Result<Vec<String>> {
        let mut checkpoints = Vec::new();
        for entry in std::fs::read_dir(&self.checkpoint_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                if let Some(file_stem) = path.file_stem() {
                    if let Some(name) = file_stem.to_str() {
                        let batch_id = name.replace(".ckpt", "");
                        checkpoints.push(batch_id);
                    }
                }
            }
        }
        Ok(checkpoints)
    }

    /// Delete checkpoint.
    pub fn delete_checkpoint(&self, batch_id: &str) -> Result<()> {
        let path = self.checkpoint_dir.join(format!("{}.ckpt.json", batch_id));
        if path.exists() {
            std::fs::remove_file(&path)?;
            info!(path = %path.display(), "Checkpoint deleted");
        }
        Ok(())
    }
}

/// Recovery manager for restoring from failures.
pub struct RecoveryManager {
    persistence: PersistenceManager,
}

impl RecoveryManager {
    pub fn new(checkpoint_dir: PathBuf) -> Result<Self> {
        let persistence = PersistenceManager::new(checkpoint_dir)?;
        Ok(Self { persistence })
    }

    /// Check if recovery is available for batch.
    pub fn has_recovery(&self, batch_id: &str) -> Result<bool> {
        Ok(self.persistence.load_checkpoint(batch_id)?.is_some())
    }

    /// Recover from checkpoint.
    pub fn recover(&self, batch_id: &str) -> Result<Checkpoint> {
        let checkpoint = self.persistence.load_checkpoint(batch_id)?
            .ok_or_else(|| format!("No checkpoint found for batch: {}", batch_id))?;

        // Verify checkpoint is not already completed
        if checkpoint.status == CheckpointStatus::Completed {
            warn!(batch_id = %batch_id, "Attempting to recover from completed checkpoint");
        }

        info!(
            batch_id = %batch_id,
            processed = checkpoint.candidates_processed,
            total = checkpoint.candidates_total,
            "Recovered from checkpoint"
        );

        Ok(checkpoint)
    }

    /// Validate recovered state.
    pub fn validate_state(&self, checkpoint: &Checkpoint) -> Result<bool> {
        if checkpoint.candidates.is_empty() {
            warn!(batch_id = %checkpoint.batch_id, "Checkpoint has no candidates");
            return Ok(false);
        }

        let pending = checkpoint.candidates
            .iter()
            .filter(|c| c.status == CandidateStatus::Pending || c.status == CandidateStatus::Processing)
            .count();

        info!(
            batch_id = %checkpoint.batch_id,
            pending_count = pending,
            "Checkpoint state validation complete"
        );

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn checkpoint_creation() {
        let cp = Checkpoint::new("batch_001".to_string(), 5);
        assert_eq!(cp.status, CheckpointStatus::InProgress);
        assert_eq!(cp.candidates_total, 5);
    }

    #[test]
    fn checkpoint_candidate_tracking() {
        let mut cp = Checkpoint::new("batch_001".to_string(), 2);
        cp.add_candidate("foo".to_string());
        cp.add_candidate("bar".to_string());

        cp.update_candidate("foo", CandidateStatus::Succeeded, None);
        assert_eq!(cp.candidates_processed, 1);
    }

    #[test]
    fn transaction_log_operations() {
        let mut log = TransactionLog::new("batch_001".to_string());
        log.add_entry(
            "extract".to_string(),
            "success".to_string(),
            "Extracted function foo".to_string(),
            vec![],
        );

        assert_eq!(log.entries.len(), 1);
    }

    #[test]
    fn persistence_manager_checkpoint_lifecycle() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PersistenceManager::new(temp_dir.path().to_path_buf()).unwrap();

        let mut cp = Checkpoint::new("batch_001".to_string(), 3);
        cp.add_candidate("foo".to_string());

        manager.save_checkpoint(&cp).unwrap();
        let loaded = manager.load_checkpoint("batch_001").unwrap();

        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().batch_id, "batch_001");
    }

    #[test]
    fn recovery_manager_restore() {
        let temp_dir = TempDir::new().unwrap();
        let manager = RecoveryManager::new(temp_dir.path().to_path_buf()).unwrap();

        let mut cp = Checkpoint::new("batch_001".to_string(), 2);
        cp.add_candidate("foo".to_string());

        manager.persistence.save_checkpoint(&cp).unwrap();
        assert!(manager.has_recovery("batch_001").unwrap());

        let recovered = manager.recover("batch_001").unwrap();
        assert_eq!(recovered.candidates_total, 2);
    }
}
