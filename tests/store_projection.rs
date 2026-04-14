use anyhow::Result;

use aidoit::domain::{
    NodeKind, NodeRecord, NodeState, RawEventKind, RawEventPayload, RelationKind, ReviewState,
    StoredEvent, StoredEventMeta, WorkState,
};
use aidoit::store::{IngestCheckpoint, Store};

fn node_event(event_id: &str, node_id: &str, kind: NodeKind, state: NodeState) -> StoredEvent {
    StoredEvent::from_meta(
        StoredEventMeta::new(
            event_id,
            "/repo/demo",
            "/tmp/demo.jsonl",
            1,
            "session-1",
            "2026-04-14T12:00:00Z",
        ),
        RawEventKind::NodeCaptured,
        RawEventPayload::NodeCaptured {
            node: NodeRecord {
                id: node_id.to_string(),
                kind,
                title: format!("节点 {node_id}"),
                state,
                summary: None,
                source_event_id: event_id.to_string(),
            },
        },
    )
}

fn state_event(event_id: &str, node_id: &str, state: NodeState) -> StoredEvent {
    StoredEvent::from_meta(
        StoredEventMeta::new(
            event_id,
            "/repo/demo",
            "/tmp/demo.jsonl",
            2,
            "session-1",
            "2026-04-14T12:00:01Z",
        ),
        RawEventKind::StateChanged,
        RawEventPayload::StateChanged {
            node_id: node_id.to_string(),
            state,
            reason: Some("测试状态流转".to_string()),
        },
    )
}

fn relation_event(event_id: &str, source_id: &str, target_id: &str) -> StoredEvent {
    StoredEvent::from_meta(
        StoredEventMeta::new(
            event_id,
            "/repo/demo",
            "/tmp/demo.jsonl",
            3,
            "session-1",
            "2026-04-14T12:00:02Z",
        ),
        RawEventKind::RelationCaptured,
        RawEventPayload::RelationCaptured {
            source_id: source_id.to_string(),
            target_id: target_id.to_string(),
            relation: RelationKind::DependsOn,
        },
    )
}

#[test]
fn schema_初始化后包含核心表() -> Result<()> {
    let store = Store::open_in_memory()?;
    store.initialize()?;

    assert!(store.has_table("ingest_checkpoints")?);
    assert!(store.has_table("raw_events")?);
    assert!(store.has_table("nodes")?);
    assert!(store.has_table("relations")?);

    Ok(())
}

#[test]
fn 重复摄入同一事件不会重复写入_raw_events() -> Result<()> {
    let mut store = Store::open_in_memory()?;
    store.initialize()?;
    let checkpoint =
        IngestCheckpoint::new("/repo/demo", "/tmp/demo.jsonl", 1, "2026-04-14T12:00:00Z");
    let event = node_event(
        "evt-1",
        "branch-1",
        NodeKind::Branch,
        NodeState::Work(WorkState::Parked),
    );

    let first = store.ingest_batch(std::slice::from_ref(&event), &checkpoint)?;
    let second = store.ingest_batch(std::slice::from_ref(&event), &checkpoint)?;

    assert_eq!(first.inserted_raw_events, 1);
    assert_eq!(second.inserted_raw_events, 0);
    assert_eq!(store.raw_event_count()?, 1);
    assert_eq!(store.node_count()?, 1);

    Ok(())
}

#[test]
fn 重复_node_capture_不会回退已更新状态() -> Result<()> {
    let mut store = Store::open_in_memory()?;
    store.initialize()?;
    let checkpoint =
        IngestCheckpoint::new("/repo/demo", "/tmp/demo.jsonl", 3, "2026-04-14T12:00:03Z");
    let initial = node_event(
        "evt-reset-1",
        "branch-keep-ready",
        NodeKind::Branch,
        NodeState::Work(WorkState::Parked),
    );
    let updated = state_event(
        "evt-reset-2",
        "branch-keep-ready",
        NodeState::Work(WorkState::Ready),
    );
    let repeated_capture = node_event(
        "evt-reset-3",
        "branch-keep-ready",
        NodeKind::Branch,
        NodeState::Work(WorkState::Parked),
    );

    store.ingest_batch(&[initial, updated, repeated_capture], &checkpoint)?;

    let branch = store
        .get_node("branch-keep-ready")?
        .expect("分支节点应存在");
    assert_eq!(branch.state, NodeState::Work(WorkState::Ready));

    Ok(())
}

#[test]
fn checkpoint_会被最新行号覆盖() -> Result<()> {
    let mut store = Store::open_in_memory()?;
    store.initialize()?;
    let event = node_event(
        "evt-2",
        "principle-1",
        NodeKind::Principle,
        NodeState::Review(ReviewState::Proposed),
    );
    let first = IngestCheckpoint::new("/repo/demo", "/tmp/demo.jsonl", 8, "2026-04-14T12:00:08Z");
    let second = IngestCheckpoint::new("/repo/demo", "/tmp/demo.jsonl", 12, "2026-04-14T12:00:12Z");

    store.ingest_batch(std::slice::from_ref(&event), &first)?;
    store.ingest_batch(std::slice::from_ref(&event), &second)?;

    let checkpoint = store
        .checkpoint("/repo/demo", "/tmp/demo.jsonl")?
        .expect("checkpoint 应存在");

    assert_eq!(checkpoint.last_line_no, 12);
    assert_eq!(checkpoint.updated_at, "2026-04-14T12:00:12Z");

    Ok(())
}

#[test]
fn 投影失败时事务整体回滚() -> Result<()> {
    let mut store = Store::open_in_memory()?;
    store.initialize()?;
    let checkpoint =
        IngestCheckpoint::new("/repo/demo", "/tmp/demo.jsonl", 2, "2026-04-14T12:00:02Z");
    let node = node_event(
        "evt-3",
        "branch-2",
        NodeKind::Branch,
        NodeState::Work(WorkState::Ready),
    );
    let invalid_state = state_event("evt-4", "missing-branch", NodeState::Work(WorkState::Done));

    let result = store.ingest_batch(&[node, invalid_state], &checkpoint);

    assert!(result.is_err());
    assert_eq!(store.raw_event_count()?, 0);
    assert_eq!(store.node_count()?, 0);
    assert_eq!(store.checkpoint("/repo/demo", "/tmp/demo.jsonl")?, None);

    Ok(())
}

#[test]
fn relation_要求两端节点都已存在() -> Result<()> {
    let mut store = Store::open_in_memory()?;
    store.initialize()?;
    let checkpoint =
        IngestCheckpoint::new("/repo/demo", "/tmp/demo.jsonl", 3, "2026-04-14T12:00:03Z");
    let source = node_event(
        "evt-5",
        "task-1",
        NodeKind::Task,
        NodeState::Work(WorkState::Ready),
    );
    let relation = relation_event("evt-6", "task-1", "missing-task");

    let result = store.ingest_batch(&[source, relation], &checkpoint);

    assert!(result.is_err());
    assert_eq!(store.raw_event_count()?, 0);
    assert_eq!(store.node_count()?, 0);

    Ok(())
}
