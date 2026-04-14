use aidoit::domain::{
    DomainError, NodeKind, ReviewState, WorkState, validate_review_state_transition,
    validate_work_state_transition,
};

#[test]
fn branch_允许从_parked_进入_ready() {
    let result =
        validate_work_state_transition(NodeKind::Branch, WorkState::Parked, WorkState::Ready);

    assert_eq!(result, Ok(()));
}

#[test]
fn branch_允许从_ready_进入_blocked() {
    let result =
        validate_work_state_transition(NodeKind::Branch, WorkState::Ready, WorkState::Blocked);

    assert_eq!(result, Ok(()));
}

#[test]
fn branch_拒绝从_blocked_直接进入_done() {
    let result =
        validate_work_state_transition(NodeKind::Branch, WorkState::Blocked, WorkState::Done);

    assert_eq!(result, Err(DomainError::InvalidTransition));
}

#[test]
fn task_允许重新停放到_parked() {
    let result =
        validate_work_state_transition(NodeKind::Task, WorkState::Ready, WorkState::Parked);

    assert_eq!(result, Ok(()));
}

#[test]
fn principle_只能从_proposed_进入_confirmed_或_rejected() {
    let confirmed = validate_review_state_transition(
        NodeKind::Principle,
        ReviewState::Proposed,
        ReviewState::Confirmed,
    );
    let rejected = validate_review_state_transition(
        NodeKind::Principle,
        ReviewState::Proposed,
        ReviewState::Rejected,
    );
    let invalid = validate_review_state_transition(
        NodeKind::Principle,
        ReviewState::Confirmed,
        ReviewState::Rejected,
    );

    assert_eq!(confirmed, Ok(()));
    assert_eq!(rejected, Ok(()));
    assert_eq!(invalid, Err(DomainError::InvalidTransition));
}

#[test]
fn agreement_不能使用工作流状态机() {
    let result =
        validate_work_state_transition(NodeKind::Agreement, WorkState::Parked, WorkState::Ready);

    assert_eq!(result, Err(DomainError::InvalidTransition));
}
