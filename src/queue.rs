//! Async refactor queue: entropy-thresholded candidate paths plus the
//! proposal list, all behind a single `RwLock` so `RefactorQueue` clones
//! share state across daemon tasks.

use crate::id;
use crate::time::now;
use crate::{
    DiffSummary, Module, Proposal, ProposalId, ProposalStatus, RefactorKind, TimelineEvent,
};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Thread-safe queue of refactor candidates and proposals.
#[derive(Clone, Default)]
pub struct RefactorQueue {
    inner: Arc<RwLock<QueueInner>>,
}

#[derive(Default)]
struct QueueInner {
    candidates: VecDeque<PathBuf>,
    proposals: Vec<Proposal>,
}

impl RefactorQueue {
    /// Create an empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Refresh candidate list from the latest module index.
    pub async fn refresh(&self, modules: &[Module], threshold: f64) {
        let mut inner = self.inner.write().await;
        inner.candidates.clear();
        for module in modules.iter().filter(|m| m.entropy >= threshold) {
            inner.candidates.push_back(module.path.clone());
        }
    }

    /// Pop the next candidate path.
    pub async fn next_candidate(&self) -> Option<PathBuf> {
        let mut inner = self.inner.write().await;
        inner.candidates.pop_front()
    }

    /// Register a new proposal.
    pub async fn enqueue_proposal(&self, mut proposal: Proposal) -> ProposalId {
        let id = proposal.id.clone();
        proposal.status = ProposalStatus::Queued;
        proposal.timeline.push(TimelineEvent {
            at: now(),
            message: "Entered refactor queue".to_string(),
        });
        let mut inner = self.inner.write().await;
        inner.proposals.push(proposal);
        id
    }

    pub async fn proposals(&self) -> Vec<Proposal> {
        self.inner.read().await.proposals.clone()
    }

    /// Replace the proposal list with state restored from the journal. No
    /// timeline injection — restored proposals already carry a "Restored" event.
    pub async fn restore(&self, proposals: Vec<Proposal>) {
        let mut inner = self.inner.write().await;
        inner.proposals = proposals;
    }

    pub async fn proposal(&self, id: &str) -> Option<Proposal> {
        self.inner
            .read()
            .await
            .proposals
            .iter()
            .find(|p| p.id == id)
            .cloned()
    }

    pub async fn update_proposal<F>(&self, id: &str, f: F)
    where
        F: FnOnce(&mut Proposal),
    {
        let mut inner = self.inner.write().await;
        if let Some(p) = inner.proposals.iter_mut().find(|p| p.id == id) {
            f(p);
        }
    }
}

/// Create a stub proposal for a detected candidate.
#[must_use]
pub fn proposal_for(module: &Module, kind: RefactorKind) -> Proposal {
    let id = id::next();
    Proposal {
        id,
        created_at: now(),
        module: module.path.clone(),
        kind,
        confidence: 0.0,
        status: ProposalStatus::Detected,
        validation: None,
        diff_summary: DiffSummary::default(),
        migration_notes: Vec::new(),
        changed_files: Vec::new(),
        pr_body: None,
        timeline: vec![TimelineEvent {
            at: now(),
            message: format!(
                "Detected {} in {} (entropy {:.2})",
                kind.description(),
                module.path.display(),
                module.entropy
            ),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Health, Language};
    use std::time::SystemTime;

    fn module(path: &str, entropy: f64) -> Module {
        Module {
            path: PathBuf::from(path),
            language: Language::Rust,
            lines: 0,
            functions: 0,
            cyclomatic_complexity: 0,
            public_api_size: 0,
            fan_out: 0,
            fan_in: 0,
            duplicates: 0,
            edit_frequency: 0.0,
            confidence: None,
            churn: 0,
            test_coverage: 0.0,
            entropy,
            health: Health::from_entropy(entropy),
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }

    fn proposal(id: &str) -> Proposal {
        Proposal {
            id: id.to_string(),
            created_at: SystemTime::UNIX_EPOCH,
            module: PathBuf::from("src/lib.rs"),
            kind: RefactorKind::ExtractFunction,
            confidence: 0.0,
            status: ProposalStatus::Detected,
            validation: None,
            diff_summary: DiffSummary::default(),
            migration_notes: Vec::new(),
            changed_files: Vec::new(),
            pr_body: None,
            timeline: Vec::new(),
        }
    }

    #[tokio::test]
    async fn refresh_keeps_only_modules_at_or_above_threshold() {
        let queue = RefactorQueue::new();
        let modules = [
            module("src/low.rs", 0.5),
            module("src/edge.rs", 0.82),
            module("src/high.rs", 0.9),
        ];
        queue.refresh(&modules, 0.82).await;
        assert_eq!(
            queue.next_candidate().await,
            Some(PathBuf::from("src/edge.rs"))
        );
        assert_eq!(
            queue.next_candidate().await,
            Some(PathBuf::from("src/high.rs"))
        );
        assert_eq!(queue.next_candidate().await, None);
    }

    #[tokio::test]
    async fn refresh_replaces_previous_candidates() {
        let queue = RefactorQueue::new();
        queue.refresh(&[module("src/old.rs", 0.9)], 0.82).await;
        queue.refresh(&[module("src/new.rs", 0.9)], 0.82).await;
        assert_eq!(
            queue.next_candidate().await,
            Some(PathBuf::from("src/new.rs"))
        );
        assert_eq!(queue.next_candidate().await, None);
    }

    #[tokio::test]
    async fn next_candidate_on_empty_queue_returns_none() {
        let queue = RefactorQueue::new();
        assert_eq!(queue.next_candidate().await, None);
    }

    #[tokio::test]
    async fn enqueue_proposal_sets_queued_status_and_timeline() {
        let queue = RefactorQueue::new();
        let id = queue.enqueue_proposal(proposal("p1")).await;
        assert_eq!(id, "p1");
        let stored = queue.proposal("p1").await.unwrap();
        assert_eq!(stored.status, ProposalStatus::Queued);
        assert_eq!(stored.timeline.len(), 1);
        assert!(stored.timeline[0]
            .message
            .contains("Entered refactor queue"));
    }

    #[tokio::test]
    async fn proposals_listing_and_lookup() {
        let queue = RefactorQueue::new();
        queue.enqueue_proposal(proposal("p1")).await;
        queue.enqueue_proposal(proposal("p2")).await;
        assert_eq!(queue.proposals().await.len(), 2);
        assert!(queue.proposal("p2").await.is_some());
        assert!(queue.proposal("missing").await.is_none());
    }

    #[tokio::test]
    async fn update_proposal_mutates_only_the_matching_entry() {
        let queue = RefactorQueue::new();
        queue.enqueue_proposal(proposal("p1")).await;
        queue.enqueue_proposal(proposal("p2")).await;
        queue
            .update_proposal("p1", |p| p.status = ProposalStatus::Accepted)
            .await;
        assert_eq!(
            queue.proposal("p1").await.unwrap().status,
            ProposalStatus::Accepted
        );
        assert_eq!(
            queue.proposal("p2").await.unwrap().status,
            ProposalStatus::Queued
        );
        // Updating a missing id is a no-op, not a panic.
        queue
            .update_proposal("missing", |p| p.status = ProposalStatus::Merged)
            .await;
        assert_eq!(queue.proposals().await.len(), 2);
    }

    #[tokio::test]
    async fn restore_replaces_proposal_list_without_timeline_injection() {
        let queue = RefactorQueue::new();
        queue.enqueue_proposal(proposal("p1")).await;
        queue.restore(vec![proposal("p9")]).await;
        assert!(queue.proposal("p1").await.is_none());
        let restored = queue.proposal("p9").await.unwrap();
        assert_eq!(restored.status, ProposalStatus::Detected);
        assert!(restored.timeline.is_empty());
    }

    #[tokio::test]
    async fn clones_share_the_same_underlying_state() {
        let queue = RefactorQueue::new();
        let clone = queue.clone();
        clone.enqueue_proposal(proposal("p1")).await;
        assert!(queue.proposal("p1").await.is_some());
    }

    #[test]
    fn proposal_for_builds_detected_stub() {
        let module = module("src/big.rs", 0.95);
        let proposal = proposal_for(&module, RefactorKind::SplitModule);
        assert!(!proposal.id.is_empty());
        assert_eq!(proposal.status, ProposalStatus::Detected);
        assert_eq!(proposal.module, PathBuf::from("src/big.rs"));
        assert_eq!(proposal.kind, RefactorKind::SplitModule);
        assert!(proposal.changed_files.is_empty());
        assert!(proposal.pr_body.is_none());
        assert_eq!(proposal.timeline.len(), 1);
        let message = &proposal.timeline[0].message;
        assert!(
            message.contains("Split responsibilities into submodules"),
            "{message}"
        );
        assert!(message.contains("src/big.rs"), "{message}");
        assert!(message.contains("0.95"), "{message}");
    }
}
