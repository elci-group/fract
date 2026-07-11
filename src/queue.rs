use crate::id;
use crate::time::now;
use crate::{Health, Module, Proposal, ProposalId, ProposalStatus, RefactorKind, TimelineEvent};
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

    pub async fn health_counts(&self, modules: &[Module]) -> (usize, usize, usize) {
        modules
            .iter()
            .fold((0, 0, 0), |(h, w, c), m| match m.health {
                Health::Excellent | Health::Healthy => (h + 1, w, c),
                Health::Warning => (h, w + 1, c),
                Health::Critical => (h, w, c + 1),
            })
    }
}

/// Create a stub proposal for a detected candidate.
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
        diff_summary: Default::default(),
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
