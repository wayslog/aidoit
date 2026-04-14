use std::{fs, io::Write};

use anyhow::Result;
use tempfile::tempdir;

use aidoit::domain::{NodeKind, NodeState, ReviewState, WorkState};
use aidoit::ingest::import_codex_transcript;
use aidoit::store::Store;

const FIXTURE: &str = "tests/fixtures/codex_session.jsonl";

#[test]
fn codex_transcript_能抽取最小对象集() -> Result<()> {
    let temp = tempdir()?;
    let transcript = temp.path().join("session.jsonl");
    fs::copy(FIXTURE, &transcript)?;

    let mut store = Store::open_in_memory()?;
    store.initialize()?;

    let report = import_codex_transcript(&mut store, "/repo/demo", &transcript)?;

    assert_eq!(report.message_count, 2);
    assert_eq!(report.emitted_events, 12);
    assert_eq!(report.inserted_raw_events, 12);
    assert_eq!(report.last_line_no, 4);
    assert_eq!(store.raw_event_count()?, 12);

    let branches = store.list_nodes_by_kind(NodeKind::Branch)?;
    let tasks = store.list_nodes_by_kind(NodeKind::Task)?;
    let principles = store.list_nodes_by_kind(NodeKind::Principle)?;
    let agreements = store.list_nodes_by_kind(NodeKind::Agreement)?;
    let phases = store.list_nodes_by_kind(NodeKind::Phase)?;
    let relations = store.list_relations()?;

    assert_eq!(branches.len(), 3);
    assert_eq!(tasks.len(), 1);
    assert_eq!(principles.len(), 1);
    assert_eq!(agreements.len(), 1);
    assert_eq!(phases.len(), 1);
    assert_eq!(relations.len(), 4);

    assert!(branches.iter().any(|node| {
        node.title == "实现 CLI 视图" && node.state == NodeState::Work(WorkState::Parked)
    }));
    assert!(branches.iter().any(|node| {
        node.title == "实现 raw_events" && node.state == NodeState::Work(WorkState::Ready)
    }));
    assert!(branches.iter().any(|node| {
        node.title == "补充 ingest fixture" && node.state == NodeState::Work(WorkState::Parked)
    }));
    assert!(principles.iter().any(|node| {
        node.title == "transcript 是权威源"
            && node.state == NodeState::Review(ReviewState::Proposed)
    }));

    Ok(())
}

#[test]
fn 增量导入只吸收新增事件() -> Result<()> {
    let temp = tempdir()?;
    let transcript = temp.path().join("session.jsonl");
    fs::copy(FIXTURE, &transcript)?;

    let mut store = Store::open_in_memory()?;
    store.initialize()?;

    let first = import_codex_transcript(&mut store, "/repo/demo", &transcript)?;
    let repeated = import_codex_transcript(&mut store, "/repo/demo", &transcript)?;

    assert_eq!(first.inserted_raw_events, 12);
    assert_eq!(repeated.inserted_raw_events, 0);

    fs::OpenOptions::new()
        .append(true)
        .open(&transcript)?
        .write_all(
            "{\"timestamp\":\"2026-04-14T04:00:03.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"状态：branch:补充 ingest fixture -> ready\\n状态：principle:transcript 是权威源 -> confirmed\",\"phase\":\"commentary\",\"memory_citation\":null}}\n"
                .as_bytes(),
        )?;

    let appended = import_codex_transcript(&mut store, "/repo/demo", &transcript)?;
    let branches = store.list_nodes_by_kind(NodeKind::Branch)?;
    let principles = store.list_nodes_by_kind(NodeKind::Principle)?;

    assert_eq!(appended.inserted_raw_events, 2);
    assert_eq!(store.raw_event_count()?, 14);
    assert!(
        branches
            .iter()
            .any(|node| node.title == "补充 ingest fixture"
                && node.state == NodeState::Work(WorkState::Ready))
    );
    assert!(principles.iter().any(|node| {
        node.title == "transcript 是权威源"
            && node.state == NodeState::Review(ReviewState::Confirmed)
    }));

    Ok(())
}

#[test]
fn 同一条消息里前置状态和依赖也能成功导入() -> Result<()> {
    let temp = tempdir()?;
    let transcript = temp.path().join("reordered.jsonl");
    fs::write(
        &transcript,
        concat!(
            "{\"timestamp\":\"2026-04-14T04:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-1\",\"cwd\":\"/repo/demo\"}}\n",
            "{\"timestamp\":\"2026-04-14T04:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"状态：branch:实现 raw_events -> ready\\n依赖：branch:实现 CLI 视图 -> branch:实现 raw_events\\n分支：实现 raw_events\\n分支：实现 CLI 视图\",\"phase\":\"commentary\",\"memory_citation\":null}}\n"
        ),
    )?;

    let mut store = Store::open_in_memory()?;
    store.initialize()?;

    let report = import_codex_transcript(&mut store, "/repo/demo", &transcript)?;
    let branches = store.list_nodes_by_kind(NodeKind::Branch)?;
    let relations = store.list_relations()?;

    assert_eq!(report.inserted_raw_events, 4);
    assert_eq!(branches.len(), 2);
    assert!(branches.iter().any(|node| {
        node.title == "实现 raw_events" && node.state == NodeState::Work(WorkState::Ready)
    }));
    assert!(
        relations
            .iter()
            .any(|relation| relation.relation == aidoit::domain::RelationKind::DependsOn)
    );

    Ok(())
}

#[test]
fn 损坏的_transcript_会返回行号错误() -> Result<()> {
    let temp = tempdir()?;
    let transcript = temp.path().join("broken.jsonl");
    fs::write(
        &transcript,
        "{\"timestamp\":\"2026-04-14T04:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-1\",\"cwd\":\"/repo/demo\"}}\nnot-json\n",
    )?;

    let mut store = Store::open_in_memory()?;
    store.initialize()?;

    let error = import_codex_transcript(&mut store, "/repo/demo", &transcript).unwrap_err();

    assert!(format!("{error:#}").contains("第 2 行"));

    Ok(())
}

#[test]
fn 节点标题里的冒号会被完整保留() -> Result<()> {
    let temp = tempdir()?;
    let transcript = temp.path().join("colon.jsonl");
    fs::write(
        &transcript,
        concat!(
            "{\"timestamp\":\"2026-04-14T04:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-1\",\"cwd\":\"/repo/demo\"}}\n",
            "{\"timestamp\":\"2026-04-14T04:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"任务：TODO: 修 bug\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}\n"
        ),
    )?;

    let mut store = Store::open_in_memory()?;
    store.initialize()?;

    import_codex_transcript(&mut store, "/repo/demo", &transcript)?;
    let tasks = store.list_nodes_by_kind(NodeKind::Task)?;

    assert!(
        tasks.iter().any(|node| node.title == "TODO: 修 bug"),
        "任务标题中的冒号应该被完整保留"
    );

    Ok(())
}
