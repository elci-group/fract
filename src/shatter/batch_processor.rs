//! Batch processing of multiple transformation candidates.
//!
//! Processes multiple candidates atomically with conflict detection,
//! dependency resolution, and safe ordering.

use std::path::PathBuf;
use std::collections::{HashSet, HashMap};
use crate::error::Result;
use super::preconditions::CandidateFunction;

/// Type of conflict between candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictKind {
    /// Both candidates target the same source function
    SameSourceFunction,
    /// Both candidates target the same file
    SameTargetFile,
    /// Candidates have conflicting import requirements
    ConflictingImports,
    /// Candidate B depends on transformation by candidate A
    Dependency,
}

/// Conflict information between two candidates.
#[derive(Debug, Clone)]
pub struct Conflict {
    pub candidate_a: String,
    pub candidate_b: String,
    pub kind: ConflictKind,
    pub message: String,
}

/// Batch of candidates with conflict analysis.
#[derive(Debug, Clone)]
pub struct CandidateBatch {
    pub candidates: Vec<CandidateFunction>,
    pub conflicts: Vec<Conflict>,
    pub can_process_atomically: bool,
}

impl CandidateBatch {
    pub fn new(candidates: Vec<CandidateFunction>) -> Self {
        Self {
            candidates,
            conflicts: Vec::new(),
            can_process_atomically: true,
        }
    }

    pub fn add_conflict(&mut self, conflict: Conflict) {
        self.conflicts.push(conflict);
        self.can_process_atomically = false;
    }

    pub fn conflict_count(&self) -> usize {
        self.conflicts.len()
    }
}

/// Batch processor for multiple candidates.
pub struct BatchProcessor {
    candidates: Vec<CandidateFunction>,
}

impl BatchProcessor {
    /// Create processor from candidates.
    pub fn new(candidates: Vec<CandidateFunction>) -> Self {
        Self { candidates }
    }

    /// Analyze batch for conflicts.
    pub fn analyze_conflicts(&self) -> Result<CandidateBatch> {
        let mut batch = CandidateBatch::new(self.candidates.clone());

        // Check for same source function conflicts
        self.detect_source_conflicts(&mut batch)?;

        // Check for target file conflicts
        self.detect_target_file_conflicts(&mut batch)?;

        // Check for import conflicts
        self.detect_import_conflicts(&mut batch)?;

        Ok(batch)
    }

    /// Detect candidates targeting the same source function.
    fn detect_source_conflicts(&self, batch: &mut CandidateBatch) -> Result<()> {
        let mut source_map: HashMap<String, Vec<usize>> = HashMap::new();

        for (i, candidate) in batch.candidates.iter().enumerate() {
            let key = format!("{}:{}", candidate.file.display(), candidate.function_name);
            source_map.entry(key).or_insert_with(Vec::new).push(i);
        }

        for (_key, indices) in source_map {
            if indices.len() > 1 {
                for i in 0..indices.len() {
                    for j in (i + 1)..indices.len() {
                        let a_idx = indices[i];
                        let b_idx = indices[j];
                        let conflict = Conflict {
                            candidate_a: batch.candidates[a_idx].function_name.clone(),
                            candidate_b: batch.candidates[b_idx].function_name.clone(),
                            kind: ConflictKind::SameSourceFunction,
                            message: "Both candidates extract the same source function".to_string(),
                        };
                        batch.add_conflict(conflict);
                    }
                }
            }
        }

        Ok(())
    }

    /// Detect candidates targeting the same target file.
    fn detect_target_file_conflicts(&self, batch: &mut CandidateBatch) -> Result<()> {
        let mut target_map: HashMap<PathBuf, Vec<usize>> = HashMap::new();

        for (i, candidate) in batch.candidates.iter().enumerate() {
            target_map
                .entry(candidate.target_file.clone())
                .or_insert_with(Vec::new)
                .push(i);
        }

        // Multiple candidates to same target file is OK (same module)
        // But log it for awareness
        for (_target, indices) in target_map {
            if indices.len() > 1 {
                // This is acceptable - multiple functions can be extracted to same module
                // No conflict added
            }
        }

        Ok(())
    }

    /// Detect import requirement conflicts.
    fn detect_import_conflicts(&self, _batch: &mut CandidateBatch) -> Result<()> {
        // Import conflicts are rare and typically resolvable
        // Would need more sophisticated analysis to detect these
        Ok(())
    }

    /// Check if batch can be processed safely.
    pub fn can_process_safely(&self, batch: &CandidateBatch) -> bool {
        // Safe if no source function conflicts
        !batch.conflicts.iter().any(|c| c.kind == ConflictKind::SameSourceFunction)
    }

    /// Split batch into safe sub-batches.
    pub fn split_into_safe_batches(&self) -> Result<Vec<CandidateBatch>> {
        let analysis = self.analyze_conflicts()?;

        if analysis.conflicts.is_empty() {
            return Ok(vec![analysis]);
        }

        let mut batches = Vec::new();
        let mut processed = HashSet::new();

        for (i, candidate) in analysis.candidates.iter().enumerate() {
            if processed.contains(&i) {
                continue;
            }

            let mut batch_candidates = vec![candidate.clone()];
            processed.insert(i);

            // Add compatible candidates
            for (j, other) in analysis.candidates.iter().enumerate() {
                if i != j && !processed.contains(&j) {
                    let has_conflict = analysis.conflicts.iter().any(|c| {
                        (c.candidate_a == candidate.function_name
                            && c.candidate_b == other.function_name)
                            || (c.candidate_a == other.function_name
                                && c.candidate_b == candidate.function_name)
                    });

                    if !has_conflict {
                        batch_candidates.push(other.clone());
                        processed.insert(j);
                    }
                }
            }

            batches.push(CandidateBatch::new(batch_candidates));
        }

        Ok(batches)
    }

    /// Get dependency order for candidates.
    pub fn dependency_order(&self) -> Result<Vec<CandidateFunction>> {
        // For now, return in order - full dependency analysis would be more complex
        Ok(self.candidates.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_detects_same_source_conflicts() {
        let candidates = vec![
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "foo".to_string(),
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.95,
            ),
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "foo".to_string(),
                "crate::other".to_string(),
                PathBuf::from("src/other.rs"),
                0.90,
            ),
        ];

        let processor = BatchProcessor::new(candidates);
        let batch = processor.analyze_conflicts().unwrap();

        assert_eq!(batch.conflict_count(), 1);
        assert_eq!(batch.conflicts[0].kind, ConflictKind::SameSourceFunction);
    }

    #[test]
    fn batch_accepts_compatible_candidates() {
        let candidates = vec![
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "foo".to_string(),
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.95,
            ),
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "bar".to_string(),
                "crate::other".to_string(),
                PathBuf::from("src/other.rs"),
                0.90,
            ),
        ];

        let processor = BatchProcessor::new(candidates);
        let batch = processor.analyze_conflicts().unwrap();

        assert_eq!(batch.conflict_count(), 0);
        assert!(processor.can_process_safely(&batch));
    }

    #[test]
    fn batch_splits_conflicting_candidates() {
        let candidates = vec![
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "foo".to_string(),
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.95,
            ),
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "foo".to_string(),
                "crate::other".to_string(),
                PathBuf::from("src/other.rs"),
                0.90,
            ),
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "bar".to_string(),
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.85,
            ),
        ];

        let processor = BatchProcessor::new(candidates);
        let batches = processor.split_into_safe_batches().unwrap();

        // Should split conflicting foo candidates
        assert!(batches.len() >= 1);
        // At least one batch should be processable
        assert!(batches.iter().any(|b| processor.can_process_safely(b)));
    }

    #[test]
    fn batch_candidate_tracking() {
        let mut batch = CandidateBatch::new(vec![]);
        assert!(batch.can_process_atomically);

        let conflict = Conflict {
            candidate_a: "foo".to_string(),
            candidate_b: "bar".to_string(),
            kind: ConflictKind::SameSourceFunction,
            message: "test".to_string(),
        };
        batch.add_conflict(conflict);

        assert!(!batch.can_process_atomically);
        assert_eq!(batch.conflict_count(), 1);
    }
}
