use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    Parked,
    Ready,
    Blocked,
    Done,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Proposed,
    Confirmed,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseState {
    Active,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Task,
    Branch,
    Principle,
    Agreement,
    Phase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    DerivedFrom,
    Blocks,
    DependsOn,
    Implements,
    Supersedes,
    RelatedTo,
    ChildOf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum NodeState {
    Work(WorkState),
    Review(ReviewState),
    Phase(PhaseState),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRecord {
    pub id: String,
    pub kind: NodeKind,
    pub title: String,
    pub state: NodeState,
    pub summary: Option<String>,
    pub source_event_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawEventKind {
    NodeCaptured,
    RelationCaptured,
    StateChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RawEventPayload {
    NodeCaptured {
        node: NodeRecord,
    },
    RelationCaptured {
        source_id: String,
        target_id: String,
        relation: RelationKind,
    },
    StateChanged {
        node_id: String,
        state: NodeState,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEvent {
    pub event_id: String,
    pub repo_root: String,
    pub transcript_path: String,
    pub source_line_no: u64,
    pub session_id: String,
    pub occurred_at: String,
    pub kind: RawEventKind,
    pub payload: RawEventPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEventMeta {
    pub event_id: String,
    pub repo_root: String,
    pub transcript_path: String,
    pub source_line_no: u64,
    pub session_id: String,
    pub occurred_at: String,
}

impl StoredEventMeta {
    pub fn new(
        event_id: impl Into<String>,
        repo_root: impl Into<String>,
        transcript_path: impl Into<String>,
        source_line_no: u64,
        session_id: impl Into<String>,
        occurred_at: impl Into<String>,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            repo_root: repo_root.into(),
            transcript_path: transcript_path.into(),
            source_line_no,
            session_id: session_id.into(),
            occurred_at: occurred_at.into(),
        }
    }
}

impl StoredEvent {
    pub fn from_meta(meta: StoredEventMeta, kind: RawEventKind, payload: RawEventPayload) -> Self {
        Self {
            event_id: meta.event_id,
            repo_root: meta.repo_root,
            transcript_path: meta.transcript_path,
            source_line_no: meta.source_line_no,
            session_id: meta.session_id,
            occurred_at: meta.occurred_at,
            kind,
            payload,
        }
    }
}

pub fn stable_node_id(repo_root: &str, kind: NodeKind, title: &str) -> String {
    let digest = Sha256::digest(format!("{repo_root}:{}:{title}", kind_name(kind)).as_bytes());
    let digest = format!("{digest:x}");
    format!("{}-{}", kind_name(kind), &digest[..16])
}

pub fn kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Task => "task",
        NodeKind::Branch => "branch",
        NodeKind::Principle => "principle",
        NodeKind::Agreement => "agreement",
        NodeKind::Phase => "phase",
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("非法状态迁移")]
    InvalidTransition,
}

pub fn validate_work_state_transition(
    kind: NodeKind,
    from: WorkState,
    to: WorkState,
) -> Result<(), DomainError> {
    if !matches!(kind, NodeKind::Task | NodeKind::Branch) {
        return Err(DomainError::InvalidTransition);
    }
    if from == to {
        return Ok(());
    }

    let allowed = match from {
        WorkState::Parked => matches!(to, WorkState::Ready | WorkState::Archived),
        WorkState::Ready => matches!(
            to,
            WorkState::Parked | WorkState::Blocked | WorkState::Done | WorkState::Archived
        ),
        WorkState::Blocked => matches!(to, WorkState::Ready | WorkState::Archived),
        WorkState::Done => matches!(to, WorkState::Archived),
        WorkState::Archived => false,
    };

    if allowed {
        Ok(())
    } else {
        Err(DomainError::InvalidTransition)
    }
}

pub fn validate_review_state_transition(
    kind: NodeKind,
    from: ReviewState,
    to: ReviewState,
) -> Result<(), DomainError> {
    if !matches!(kind, NodeKind::Principle | NodeKind::Agreement) {
        return Err(DomainError::InvalidTransition);
    }
    if from == to {
        return Ok(());
    }

    let allowed = matches!(from, ReviewState::Proposed)
        && matches!(to, ReviewState::Confirmed | ReviewState::Rejected);

    if allowed {
        Ok(())
    } else {
        Err(DomainError::InvalidTransition)
    }
}
