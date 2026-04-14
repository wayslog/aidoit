use anyhow::{Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, Transaction};

use crate::domain::{
    NodeKind, NodeState, RawEventPayload, RelationKind, ReviewState, StoredEvent, WorkState,
    validate_review_state_transition, validate_work_state_transition,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub id: String,
    pub kind: NodeKind,
    pub title: String,
    pub state: NodeState,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationView {
    pub source_id: String,
    pub target_id: String,
    pub relation: RelationKind,
}

pub fn apply_event(tx: &Transaction<'_>, event: &StoredEvent) -> Result<()> {
    match &event.payload {
        RawEventPayload::NodeCaptured { node } => {
            validate_state_matches_kind(node.kind, &node.state)?;
            let (state_group, state_value) = split_state(&node.state);
            tx.execute(
                "
                INSERT INTO nodes (
                    id,
                    repo_root,
                    kind,
                    title,
                    state_group,
                    state_value,
                    summary,
                    source_event_id,
                    last_event_id,
                    created_at,
                    updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
                ON CONFLICT(id) DO UPDATE SET
                    repo_root = excluded.repo_root,
                    kind = excluded.kind,
                    title = excluded.title,
                    state_group = excluded.state_group,
                    state_value = excluded.state_value,
                    summary = excluded.summary,
                    source_event_id = excluded.source_event_id,
                    last_event_id = excluded.last_event_id,
                    updated_at = excluded.updated_at
                ",
                (
                    &node.id,
                    &event.repo_root,
                    kind_to_str(node.kind),
                    &node.title,
                    state_group,
                    state_value,
                    &node.summary,
                    &node.source_event_id,
                    &event.event_id,
                    &event.occurred_at,
                ),
            )?;
            Ok(())
        }
        RawEventPayload::RelationCaptured {
            source_id,
            target_id,
            relation,
        } => {
            if !node_exists(tx, source_id)? || !node_exists(tx, target_id)? {
                bail!("关系两端节点必须先存在");
            }
            tx.execute(
                "
                INSERT OR IGNORE INTO relations (
                    repo_root,
                    source_id,
                    target_id,
                    kind,
                    source_event_id,
                    created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ",
                (
                    &event.repo_root,
                    source_id,
                    target_id,
                    serde_json::to_string(relation)?,
                    &event.event_id,
                    &event.occurred_at,
                ),
            )?;
            Ok(())
        }
        RawEventPayload::StateChanged {
            node_id,
            state,
            reason: _,
        } => {
            let stored =
                load_node_state(tx, node_id)?.ok_or_else(|| anyhow!("状态变更目标节点不存在"))?;
            validate_state_matches_kind(stored.kind, state)?;
            validate_state_change(stored.kind, &stored.state, state)?;
            let (state_group, state_value) = split_state(state);
            tx.execute(
                "
                UPDATE nodes
                SET state_group = ?2,
                    state_value = ?3,
                    last_event_id = ?4,
                    updated_at = ?5
                WHERE id = ?1
                ",
                (
                    node_id,
                    state_group,
                    state_value,
                    &event.event_id,
                    &event.occurred_at,
                ),
            )?;
            Ok(())
        }
    }
}

pub fn count_nodes(conn: &Connection) -> Result<usize> {
    let count = conn.query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get::<_, i64>(0))?;
    Ok(count as usize)
}

pub fn list_nodes(conn: &Connection) -> Result<Vec<NodeView>> {
    let mut stmt = conn.prepare(
        "
        SELECT id, kind, title, state_group, state_value, summary
        FROM nodes
        ORDER BY kind, title
        ",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;

    let mut nodes = Vec::new();
    for row in rows {
        let (id, kind, title, state_group, state_value, summary) = row?;
        nodes.push(NodeView {
            id,
            kind: parse_kind(kind.as_str())?,
            title,
            state: join_state(&state_group, &state_value)?,
            summary,
        });
    }

    Ok(nodes)
}

pub fn list_nodes_by_kind(conn: &Connection, kind: NodeKind) -> Result<Vec<NodeView>> {
    let mut stmt = conn.prepare(
        "
        SELECT id, kind, title, state_group, state_value, summary
        FROM nodes
        WHERE kind = ?1
        ORDER BY title
        ",
    )?;
    let rows = stmt.query_map([kind_to_str(kind)], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;

    let mut nodes = Vec::new();
    for row in rows {
        let (id, kind, title, state_group, state_value, summary) = row?;
        nodes.push(NodeView {
            id,
            kind: parse_kind(kind.as_str())?,
            title,
            state: join_state(&state_group, &state_value)?,
            summary,
        });
    }

    Ok(nodes)
}

pub fn get_node(conn: &Connection, node_id: &str) -> Result<Option<NodeView>> {
    let raw = conn
        .query_row(
            "
            SELECT id, kind, title, state_group, state_value, summary
            FROM nodes
            WHERE id = ?1
            ",
            [node_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()?;

    let Some((id, kind, title, state_group, state_value, summary)) = raw else {
        return Ok(None);
    };

    Ok(Some(NodeView {
        id,
        kind: parse_kind(kind.as_str())?,
        title,
        state: join_state(&state_group, &state_value)?,
        summary,
    }))
}

pub fn list_relations(conn: &Connection) -> Result<Vec<RelationView>> {
    let mut stmt = conn.prepare(
        "
        SELECT source_id, target_id, kind
        FROM relations
        ORDER BY source_id, target_id, kind
        ",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut relations = Vec::new();
    for row in rows {
        let (source_id, target_id, relation) = row?;
        relations.push(RelationView {
            source_id,
            target_id,
            relation: serde_json::from_str(&relation)?,
        });
    }

    Ok(relations)
}

fn node_exists(tx: &Transaction<'_>, node_id: &str) -> Result<bool> {
    let exists = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM nodes WHERE id = ?1)",
        [node_id],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(exists == 1)
}

fn validate_state_matches_kind(kind: NodeKind, state: &NodeState) -> Result<()> {
    let matches = matches!(
        (kind, state),
        (NodeKind::Task | NodeKind::Branch, NodeState::Work(_))
            | (
                NodeKind::Principle | NodeKind::Agreement,
                NodeState::Review(_)
            )
            | (NodeKind::Phase, NodeState::Phase(_))
    );

    if matches {
        Ok(())
    } else {
        bail!("节点类型与状态类型不匹配")
    }
}

fn validate_state_change(kind: NodeKind, from: &NodeState, to: &NodeState) -> Result<()> {
    match (from, to) {
        (NodeState::Work(from), NodeState::Work(to)) => {
            validate_work_state_transition(kind, *from, *to)?;
            Ok(())
        }
        (NodeState::Review(from), NodeState::Review(to)) => {
            validate_review_state_transition(kind, *from, *to)?;
            Ok(())
        }
        (NodeState::Phase(_), NodeState::Phase(_)) => Ok(()),
        _ => bail!("状态类型不一致"),
    }
}

fn load_node_state(tx: &Transaction<'_>, node_id: &str) -> Result<Option<StoredNodeState>> {
    let raw = tx
        .query_row(
            "
            SELECT kind, state_group, state_value
            FROM nodes
            WHERE id = ?1
            ",
            [node_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;

    let Some((kind, state_group, state_value)) = raw else {
        return Ok(None);
    };

    Ok(Some(StoredNodeState {
        kind: parse_kind(kind.as_str())?,
        state: join_state(&state_group, &state_value)?,
    }))
}

struct StoredNodeState {
    kind: NodeKind,
    state: NodeState,
}

fn split_state(state: &NodeState) -> (&'static str, &'static str) {
    match state {
        NodeState::Work(WorkState::Parked) => ("work", "parked"),
        NodeState::Work(WorkState::Ready) => ("work", "ready"),
        NodeState::Work(WorkState::Blocked) => ("work", "blocked"),
        NodeState::Work(WorkState::Done) => ("work", "done"),
        NodeState::Work(WorkState::Archived) => ("work", "archived"),
        NodeState::Review(ReviewState::Proposed) => ("review", "proposed"),
        NodeState::Review(ReviewState::Confirmed) => ("review", "confirmed"),
        NodeState::Review(ReviewState::Rejected) => ("review", "rejected"),
        NodeState::Phase(crate::domain::PhaseState::Active) => ("phase", "active"),
        NodeState::Phase(crate::domain::PhaseState::Closed) => ("phase", "closed"),
    }
}

fn join_state(group: &str, value: &str) -> Result<NodeState> {
    match (group, value) {
        ("work", "parked") => Ok(NodeState::Work(WorkState::Parked)),
        ("work", "ready") => Ok(NodeState::Work(WorkState::Ready)),
        ("work", "blocked") => Ok(NodeState::Work(WorkState::Blocked)),
        ("work", "done") => Ok(NodeState::Work(WorkState::Done)),
        ("work", "archived") => Ok(NodeState::Work(WorkState::Archived)),
        ("review", "proposed") => Ok(NodeState::Review(ReviewState::Proposed)),
        ("review", "confirmed") => Ok(NodeState::Review(ReviewState::Confirmed)),
        ("review", "rejected") => Ok(NodeState::Review(ReviewState::Rejected)),
        ("phase", "active") => Ok(NodeState::Phase(crate::domain::PhaseState::Active)),
        ("phase", "closed") => Ok(NodeState::Phase(crate::domain::PhaseState::Closed)),
        _ => bail!("未知状态"),
    }
}

fn parse_kind(kind: &str) -> Result<NodeKind> {
    match kind {
        "task" => Ok(NodeKind::Task),
        "branch" => Ok(NodeKind::Branch),
        "principle" => Ok(NodeKind::Principle),
        "agreement" => Ok(NodeKind::Agreement),
        "phase" => Ok(NodeKind::Phase),
        _ => bail!("未知节点类型"),
    }
}

fn kind_to_str(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Task => "task",
        NodeKind::Branch => "branch",
        NodeKind::Principle => "principle",
        NodeKind::Agreement => "agreement",
        NodeKind::Phase => "phase",
    }
}
