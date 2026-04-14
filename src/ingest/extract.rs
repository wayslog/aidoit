use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

use crate::domain::{
    NodeKind, NodeRecord, NodeState, PhaseState, RawEventKind, RawEventPayload, RelationKind,
    ReviewState, StoredEvent, StoredEventMeta, WorkState, kind_name, stable_node_id,
};

struct ExtractionContext<'a> {
    repo_root: &'a str,
    transcript_path: &'a str,
    session_id: &'a str,
    line_no: u64,
    occurred_at: &'a str,
}

pub fn extract_message_events(
    repo_root: &str,
    transcript_path: &str,
    session_id: &str,
    line_no: u64,
    occurred_at: &str,
    message: &str,
) -> Result<Vec<StoredEvent>> {
    let context = ExtractionContext {
        repo_root,
        transcript_path,
        session_id,
        line_no,
        occurred_at,
    };
    let mut events = Vec::new();

    for raw_line in message.lines() {
        let line = normalize_line(raw_line);
        if line.is_empty() {
            continue;
        }

        if let Some(parsed) = parse_line(&context, &line, events.len())? {
            events.extend(parsed);
        }
    }

    Ok(events)
}

fn parse_line(
    context: &ExtractionContext<'_>,
    line: &str,
    base_index: usize,
) -> Result<Option<Vec<StoredEvent>>> {
    if let Some(title) = strip_prefixes(line, &["阶段：", "阶段:"]) {
        return Ok(Some(vec![node_capture_event(
            context,
            base_index,
            NodeKind::Phase,
            title,
            NodeState::Phase(PhaseState::Active),
        )?]));
    }

    if let Some(title) = strip_prefixes(
        line,
        &[
            "主线任务：",
            "主线任务:",
            "任务：",
            "任务:",
            "TODO：",
            "TODO:",
        ],
    ) {
        return Ok(Some(vec![node_capture_event(
            context,
            base_index,
            NodeKind::Task,
            title,
            NodeState::Work(WorkState::Ready),
        )?]));
    }

    if let Some(title) = strip_prefixes(line, &["分支：", "分支:"]) {
        return Ok(Some(vec![node_capture_event(
            context,
            base_index,
            NodeKind::Branch,
            title,
            NodeState::Work(WorkState::Parked),
        )?]));
    }

    if let Some(title) = strip_prefixes(line, &["原则：", "原则:"]) {
        return Ok(Some(vec![node_capture_event(
            context,
            base_index,
            NodeKind::Principle,
            title,
            NodeState::Review(ReviewState::Proposed),
        )?]));
    }

    if let Some(title) = strip_prefixes(line, &["协定：", "协定:"]) {
        return Ok(Some(vec![node_capture_event(
            context,
            base_index,
            NodeKind::Agreement,
            title,
            NodeState::Review(ReviewState::Proposed),
        )?]));
    }

    if let Some(spec) = strip_prefixes(line, &["状态：", "状态:"]) {
        let (kind, title, state) = parse_state_change(spec)?;
        return Ok(Some(vec![state_change_event(
            context, base_index, kind, title, state,
        )?]));
    }

    if let Some(spec) = strip_prefixes(line, &["依赖：", "依赖:"]) {
        let relation_spec = parse_relation_spec(spec)?;
        return Ok(Some(vec![relation_event(
            context,
            base_index,
            relation_spec,
            RelationKind::DependsOn,
        )?]));
    }

    if let Some(spec) = strip_prefixes(line, &["归属：", "归属:"]) {
        let relation_spec = parse_relation_spec(spec)?;
        return Ok(Some(vec![relation_event(
            context,
            base_index,
            relation_spec,
            RelationKind::ChildOf,
        )?]));
    }

    Ok(None)
}

fn node_capture_event(
    context: &ExtractionContext<'_>,
    index: usize,
    kind: NodeKind,
    title: &str,
    state: NodeState,
) -> Result<StoredEvent> {
    let node_id = stable_node_id(context.repo_root, kind, title);
    let event_id = stable_event_id(
        context.transcript_path,
        context.line_no,
        index,
        &format!("node:{}:{}", kind_name(kind), title),
    );
    let node = NodeRecord {
        id: node_id,
        kind,
        title: title.to_string(),
        state,
        summary: None,
        source_event_id: event_id.clone(),
    };

    Ok(StoredEvent::from_meta(
        StoredEventMeta::new(
            event_id,
            context.repo_root,
            context.transcript_path,
            context.line_no,
            context.session_id,
            context.occurred_at,
        ),
        RawEventKind::NodeCaptured,
        RawEventPayload::NodeCaptured { node },
    ))
}

fn relation_event(
    context: &ExtractionContext<'_>,
    index: usize,
    relation_spec: RelationSpec<'_>,
    relation: RelationKind,
) -> Result<StoredEvent> {
    let source_id = stable_node_id(
        context.repo_root,
        relation_spec.source_kind,
        relation_spec.source_title,
    );
    let target_id = stable_node_id(
        context.repo_root,
        relation_spec.target_kind,
        relation_spec.target_title,
    );
    let event_id = stable_event_id(
        context.transcript_path,
        context.line_no,
        index,
        &format!(
            "relation:{}:{}:{}:{}",
            kind_name(relation_spec.source_kind),
            relation_spec.source_title,
            kind_name(relation_spec.target_kind),
            relation_spec.target_title
        ),
    );

    Ok(StoredEvent::from_meta(
        StoredEventMeta::new(
            event_id,
            context.repo_root,
            context.transcript_path,
            context.line_no,
            context.session_id,
            context.occurred_at,
        ),
        RawEventKind::RelationCaptured,
        RawEventPayload::RelationCaptured {
            source_id,
            target_id,
            relation,
        },
    ))
}

fn state_change_event(
    context: &ExtractionContext<'_>,
    index: usize,
    kind: NodeKind,
    title: &str,
    state: NodeState,
) -> Result<StoredEvent> {
    let node_id = stable_node_id(context.repo_root, kind, title);
    let event_id = stable_event_id(
        context.transcript_path,
        context.line_no,
        index,
        &format!("state:{}:{}:{state:?}", kind_name(kind), title),
    );

    Ok(StoredEvent::from_meta(
        StoredEventMeta::new(
            event_id,
            context.repo_root,
            context.transcript_path,
            context.line_no,
            context.session_id,
            context.occurred_at,
        ),
        RawEventKind::StateChanged,
        RawEventPayload::StateChanged {
            node_id,
            state,
            reason: Some("来自 transcript 的显式状态声明".to_string()),
        },
    ))
}

fn normalize_line(line: &str) -> String {
    line.trim()
        .trim_start_matches("- ")
        .trim_start_matches("* ")
        .trim()
        .to_string()
}

fn strip_prefixes<'a>(line: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

struct RelationSpec<'a> {
    source_kind: NodeKind,
    source_title: &'a str,
    target_kind: NodeKind,
    target_title: &'a str,
}

fn parse_relation_spec(spec: &str) -> Result<RelationSpec<'_>> {
    let (left, right) = spec
        .split_once("->")
        .map(|(left, right)| (left.trim(), right.trim()))
        .ok_or_else(|| anyhow::anyhow!("关系语句缺少 ->"))?;
    let (source_kind, source_title) = parse_node_ref(left)?;
    let (target_kind, target_title) = parse_node_ref(right)?;

    Ok(RelationSpec {
        source_kind,
        source_title,
        target_kind,
        target_title,
    })
}

fn parse_state_change(spec: &str) -> Result<(NodeKind, &str, NodeState)> {
    let (node_ref, state_text) = spec
        .split_once("->")
        .map(|(left, right)| (left.trim(), right.trim()))
        .ok_or_else(|| anyhow::anyhow!("状态语句缺少 ->"))?;
    let (kind, title) = parse_node_ref(node_ref)?;
    let state = parse_state(kind, state_text)?;

    Ok((kind, title, state))
}

fn parse_node_ref(spec: &str) -> Result<(NodeKind, &str)> {
    let (kind, title) = spec
        .split_once(':')
        .map(|(kind, title)| (kind.trim(), title.trim()))
        .ok_or_else(|| anyhow::anyhow!("节点引用缺少 kind:title"))?;

    Ok((parse_kind(kind)?, title))
}

fn parse_kind(kind: &str) -> Result<NodeKind> {
    match kind {
        "task" | "任务" => Ok(NodeKind::Task),
        "branch" | "分支" => Ok(NodeKind::Branch),
        "principle" | "原则" => Ok(NodeKind::Principle),
        "agreement" | "协定" => Ok(NodeKind::Agreement),
        "phase" | "阶段" => Ok(NodeKind::Phase),
        _ => bail!("未知节点类型: {kind}"),
    }
}

fn parse_state(kind: NodeKind, text: &str) -> Result<NodeState> {
    let state = match text {
        "parked" => NodeState::Work(WorkState::Parked),
        "ready" => NodeState::Work(WorkState::Ready),
        "blocked" => NodeState::Work(WorkState::Blocked),
        "done" => NodeState::Work(WorkState::Done),
        "archived" => NodeState::Work(WorkState::Archived),
        "proposed" => NodeState::Review(ReviewState::Proposed),
        "confirmed" => NodeState::Review(ReviewState::Confirmed),
        "rejected" => NodeState::Review(ReviewState::Rejected),
        "active" => NodeState::Phase(PhaseState::Active),
        "closed" => NodeState::Phase(PhaseState::Closed),
        _ => bail!("未知状态: {text}"),
    };

    match (&kind, &state) {
        (NodeKind::Task | NodeKind::Branch, NodeState::Work(_))
        | (NodeKind::Principle | NodeKind::Agreement, NodeState::Review(_))
        | (NodeKind::Phase, NodeState::Phase(_)) => Ok(state),
        _ => bail!("节点类型与状态类型不匹配"),
    }
}

// 事件 ID 使用 80-bit 截断摘要，面对单仓库下几千到几万条事件时碰撞概率仍然极低，
// 但长度更短，便于日志、SQLite 和 CLI 输出使用。如果后续事件量级显著放大，可以再提高截断长度。
fn stable_event_id(transcript_path: &str, line_no: u64, index: usize, seed: &str) -> String {
    let digest = digest_hex(&format!("{transcript_path}:{line_no}:{index}:{seed}"));
    format!("evt-{}", &digest[..20])
}

fn digest_hex(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    format!("{digest:x}")
}
