//! Project health snapshots and per-day refactor statistics.

use crate::time::Timestamp;
use serde::{Deserialize, Serialize};

/// Overall project health snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectHealth {
    pub score: f64,
    pub total_modules: usize,
    pub healthy: usize,
    pub warning: usize,
    pub critical: usize,
    #[serde(with = "crate::time::serde_trend")]
    pub entropy_trend: Vec<(Timestamp, f64)>,
    pub refactors_today: RefactorStats,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RefactorStats {
    pub completed: usize,
    pub pending: usize,
    pub failed: usize,
    pub loc_removed: usize,
    pub complexity_reduced: f64,
}
