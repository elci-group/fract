//! Confidence-guided optimization for transformation prioritization.
//!
//! Ranks candidates by Fract confidence scores to maximize success rates
//! by processing high-confidence transformations first.

use crate::error::Result;
use super::preconditions::CandidateFunction;

/// Strategy for ordering candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderingStrategy {
    /// Highest confidence first (greedy approach)
    HighestConfidenceFirst,
    /// Lowest confidence first (risk-averse approach)
    LowestConfidenceFirst,
    /// Mixed: alternate high and low (exploratory approach)
    Mixed,
    /// Clustering: group by confidence bands
    Clustered,
}

/// Confidence-ranked candidate with risk assessment.
#[derive(Debug, Clone)]
pub struct RankedCandidate {
    pub candidate: CandidateFunction,
    pub confidence: f64,
    pub risk_score: f64,  // 0.0-1.0: higher = riskier
    pub rank: usize,
}

impl RankedCandidate {
    pub fn new(candidate: CandidateFunction, confidence: f64) -> Self {
        let risk_score = 1.0 - confidence;  // Inverse of confidence
        Self {
            candidate,
            confidence,
            risk_score,
            rank: 0,
        }
    }

    pub fn with_rank(mut self, rank: usize) -> Self {
        self.rank = rank;
        self
    }

    pub fn is_high_confidence(&self) -> bool {
        self.confidence >= 0.75
    }

    pub fn is_medium_confidence(&self) -> bool {
        self.confidence >= 0.50 && self.confidence < 0.75
    }

    pub fn is_low_confidence(&self) -> bool {
        self.confidence < 0.50
    }
}

/// Optimizer for ranking and ordering candidates by confidence.
pub struct ConfidenceOptimizer {
    candidates: Vec<RankedCandidate>,
    strategy: OrderingStrategy,
}

impl ConfidenceOptimizer {
    /// Create a new optimizer with candidates.
    pub fn new(candidates: Vec<CandidateFunction>, strategy: OrderingStrategy) -> Self {
        let ranked = candidates
            .into_iter()
            .map(|c| {
                let confidence = c.confidence;
                RankedCandidate::new(c, confidence)
            })
            .collect();

        Self {
            candidates: ranked,
            strategy,
        }
    }

    /// Optimize and return ordered candidates.
    pub fn optimize(&mut self) -> Result<Vec<RankedCandidate>> {
        match self.strategy {
            OrderingStrategy::HighestConfidenceFirst => self.sort_by_confidence_desc(),
            OrderingStrategy::LowestConfidenceFirst => self.sort_by_confidence_asc(),
            OrderingStrategy::Mixed => self.sort_by_mixed_strategy(),
            OrderingStrategy::Clustered => self.sort_by_confidence_clusters(),
        }

        // Assign ranks after sorting
        for (i, candidate) in self.candidates.iter_mut().enumerate() {
            candidate.rank = i + 1;
        }

        Ok(self.candidates.clone())
    }

    /// Sort by highest confidence first (greedy).
    fn sort_by_confidence_desc(&mut self) {
        self.candidates.sort_by(|a, b| {
            b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Sort by lowest confidence first (risk-averse).
    fn sort_by_confidence_asc(&mut self) {
        self.candidates.sort_by(|a, b| {
            a.confidence.partial_cmp(&b.confidence).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Alternate high and low confidence (exploratory).
    fn sort_by_mixed_strategy(&mut self) {
        self.sort_by_confidence_desc();
        let len = self.candidates.len();
        let mut mixed = Vec::new();

        let mut left = 0;
        let mut right = len - 1;
        let mut take_high = true;

        while left <= right {
            if take_high {
                mixed.push(self.candidates[left].clone());
                left += 1;
            } else {
                mixed.push(self.candidates[right].clone());
                if right == 0 {
                    break;
                }
                right -= 1;
            }
            take_high = !take_high;
        }

        self.candidates = mixed;
    }

    /// Group by confidence bands (high, medium, low).
    fn sort_by_confidence_clusters(&mut self) {
        let mut high: Vec<_> = self.candidates
            .iter()
            .filter(|c| c.is_high_confidence())
            .cloned()
            .collect();
        let mut medium: Vec<_> = self.candidates
            .iter()
            .filter(|c| c.is_medium_confidence())
            .cloned()
            .collect();
        let mut low: Vec<_> = self.candidates
            .iter()
            .filter(|c| c.is_low_confidence())
            .cloned()
            .collect();

        high.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        medium.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
        low.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));

        self.candidates = [high, medium, low].concat();
    }

    /// Get candidates above confidence threshold.
    pub fn filter_by_confidence(&self, threshold: f64) -> Vec<RankedCandidate> {
        self.candidates
            .iter()
            .filter(|c| c.confidence >= threshold)
            .cloned()
            .collect()
    }

    /// Batch candidates by confidence level.
    pub fn batch_by_confidence(&self) -> ConfidenceBatches {
        let mut high = Vec::new();
        let mut medium = Vec::new();
        let mut low = Vec::new();

        for c in &self.candidates {
            if c.is_high_confidence() {
                high.push(c.clone());
            } else if c.is_medium_confidence() {
                medium.push(c.clone());
            } else if c.is_low_confidence() {
                low.push(c.clone());
            }
        }

        ConfidenceBatches {
            high_confidence: high,
            medium_confidence: medium,
            low_confidence: low,
        }
    }
}

/// Candidates grouped by confidence level.
#[derive(Debug, Clone)]
pub struct ConfidenceBatches {
    pub high_confidence: Vec<RankedCandidate>,
    pub medium_confidence: Vec<RankedCandidate>,
    pub low_confidence: Vec<RankedCandidate>,
}

impl ConfidenceBatches {
    pub fn total_count(&self) -> usize {
        self.high_confidence.len() + self.medium_confidence.len() + self.low_confidence.len()
    }

    pub fn high_count(&self) -> usize {
        self.high_confidence.len()
    }

    pub fn medium_count(&self) -> usize {
        self.medium_confidence.len()
    }

    pub fn low_count(&self) -> usize {
        self.low_confidence.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn ranked_candidate_confidence_bands() {
        let high = RankedCandidate::new(
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "foo".to_string(),
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.95,
            ),
            0.95,
        );
        assert!(high.is_high_confidence());
        assert!(!high.is_medium_confidence());
        assert!(!high.is_low_confidence());

        let medium = RankedCandidate::new(
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "bar".to_string(),
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.60,
            ),
            0.60,
        );
        assert!(!medium.is_high_confidence());
        assert!(medium.is_medium_confidence());
        assert!(!medium.is_low_confidence());
    }

    #[test]
    fn optimizer_sorts_by_confidence() {
        let candidates = vec![
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "foo".to_string(),
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.5,
            ),
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "bar".to_string(),
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.95,
            ),
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "baz".to_string(),
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.75,
            ),
        ];

        let mut optimizer = ConfidenceOptimizer::new(candidates, OrderingStrategy::HighestConfidenceFirst);
        let ordered = optimizer.optimize().unwrap();

        assert_eq!(ordered[0].confidence, 0.95);
        assert_eq!(ordered[1].confidence, 0.75);
        assert_eq!(ordered[2].confidence, 0.5);
    }

    #[test]
    fn optimizer_batches_by_confidence() {
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
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.60,
            ),
            CandidateFunction::new(
                PathBuf::from("src/lib.rs"),
                "baz".to_string(),
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.30,
            ),
        ];

        let optimizer = ConfidenceOptimizer::new(candidates, OrderingStrategy::Clustered);
        let batches = optimizer.batch_by_confidence();

        assert_eq!(batches.high_count(), 1);
        assert_eq!(batches.medium_count(), 1);
        assert_eq!(batches.low_count(), 1);
    }

    #[test]
    fn optimizer_filters_by_threshold() {
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
                "crate::utils".to_string(),
                PathBuf::from("src/utils.rs"),
                0.60,
            ),
        ];

        let optimizer = ConfidenceOptimizer::new(candidates, OrderingStrategy::HighestConfidenceFirst);
        let filtered = optimizer.filter_by_confidence(0.75);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].confidence, 0.95);
    }
}
