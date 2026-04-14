use anyhow::Result;
use rusqlite::Connection;
use tempfile::tempdir;

use aidoit::domain::{
    NodeKind, NodeRecord, NodeState, RawEventKind, RawEventPayload, RelationKind, ReviewState,
    StoredEvent, StoredEventMeta, WorkState,
};
use aidoit::store::{IngestCheckpoint, Store};

fn node_event(
    event_id: &str,
    node_id: &str,
    kind: NodeKind,
    state: NodeState,
    line_no: u64,
    occurred_at: &str,
) -> StoredEvent {
    StoredEvent::from_meta(
        StoredEventMeta::new(
            event_id,
            "/repo/demo",
            "/tmp/demo.jsonl",
            line_no,
            "session-1",
            occurred_at,
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

fn state_event(
    event_id: &str,
    node_id: &str,
    state: NodeState,
    line_no: u64,
    occurred_at: &str,
) -> StoredEvent {
    StoredEvent::from_meta(
        StoredEventMeta::new(
            event_id,
            "/repo/demo",
            "/tmp/demo.jsonl",
            line_no,
            "session-1",
            occurred_at,
        ),
        RawEventKind::StateChanged,
        RawEventPayload::StateChanged {
            node_id: node_id.to_string(),
            state,
            reason: Some("测试状态流转".to_string()),
        },
    )
}

fn relation_event(
    event_id: &str,
    source_id: &str,
    target_id: &str,
    line_no: u64,
    occurred_at: &str,
) -> StoredEvent {
    StoredEvent::from_meta(
        StoredEventMeta::new(
            event_id,
            "/repo/demo",
            "/tmp/demo.jsonl",
            line_no,
            "session-1",
            occurred_at,
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
    assert!(store.foreign_keys_enabled()?);

    Ok(())
}

#[test]
fn relations_表包含节点外键约束() -> Result<()> {
    let temp = tempdir()?;
    let db_path = temp.path().join("schema.db");
    let store = Store::open(&db_path)?;
    store.initialize()?;

    let conn = Connection::open(&db_path)?;
    let fk_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_foreign_key_list('relations')",
        [],
        |row| row.get(0),
    )?;

    assert_eq!(fk_count, 2);

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
        1,
        "2026-04-14T12:00:00Z",
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
        1,
        "2026-04-14T12:00:00Z",
    );
    let updated = state_event(
        "evt-reset-2",
        "branch-keep-ready",
        NodeState::Work(WorkState::Ready),
        2,
        "2026-04-14T12:00:01Z",
    );
    let repeated_capture = node_event(
        "evt-reset-3",
        "branch-keep-ready",
        NodeKind::Branch,
        NodeState::Work(WorkState::Parked),
        3,
        "2026-04-14T12:00:03Z",
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
        1,
        "2026-04-14T12:00:00Z",
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
        1,
        "2026-04-14T12:00:00Z",
    );
    let invalid_state = state_event(
        "evt-4",
        "missing-branch",
        NodeState::Work(WorkState::Done),
        2,
        "2026-04-14T12:00:01Z",
    );

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
        1,
        "2026-04-14T12:00:00Z",
    );
    let relation = relation_event("evt-6", "task-1", "missing-task", 2, "2026-04-14T12:00:01Z");

    let result = store.ingest_batch(&[source, relation], &checkpoint);

    assert!(result.is_err());
    assert_eq!(store.raw_event_count()?, 0);
    assert_eq!(store.node_count()?, 0);

    Ok(())
}

#[test]
fn 跨行重放不会让节点最后事件回退() -> Result<()> {
    let temp = tempdir()?;
    let db_path = temp.path().join("ordering.db");
    let mut store = Store::open(&db_path)?;
    store.initialize()?;
    let checkpoint =
        IngestCheckpoint::new("/repo/demo", "/tmp/demo.jsonl", 3, "2026-04-14T12:00:03Z");
    let initial = node_event(
        "evt-order-1",
        "branch-ordered",
        NodeKind::Branch,
        NodeState::Work(WorkState::Parked),
        1,
        "2026-04-14T12:00:00Z",
    );
    let updated = state_event(
        "evt-order-2",
        "branch-ordered",
        NodeState::Work(WorkState::Ready),
        2,
        "2026-04-14T12:00:01Z",
    );
    let repeated_capture = node_event(
        "evt-order-3",
        "branch-ordered",
        NodeKind::Branch,
        NodeState::Work(WorkState::Parked),
        3,
        "2026-04-14T12:00:03Z",
    );

    store.ingest_batch(&[initial, updated, repeated_capture], &checkpoint)?;

    let conn = Connection::open(&db_path)?;
    let (last_event_id, updated_at): (String, String) = conn.query_row(
        "SELECT last_event_id, updated_at FROM nodes WHERE id = ?1",
        ["branch-ordered"],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    assert_eq!(last_event_id, "evt-order-3");
    assert_eq!(updated_at, "2026-04-14T12:00:03Z");

    Ok(())
}
