use anyhow::{Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::domain::{
    NodeKind, NodeScope, NodeState, RawEventPayload, RelationKind, ReviewState, StoredEvent,
    WorkState, node_scope, validate_review_state_transition, validate_work_state_transition,
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
            let (scope, scope_id, unit_id) = node_scope_columns(event, node.kind);
            tx.execute(
                "
                INSERT INTO nodes (
                    id,
                    project_id,
                    scope,
                    scope_id,
                    unit_id,
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
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)
                ON CONFLICT(id) DO UPDATE SET
                    project_id = excluded.project_id,
                    scope = excluded.scope,
                    scope_id = excluded.scope_id,
                    unit_id = excluded.unit_id,
                    repo_root = excluded.repo_root,
                    kind = excluded.kind,
                    title = excluded.title,
                    summary = excluded.summary,
                    last_event_id = excluded.last_event_id,
                    updated_at = excluded.updated_at
                ",
                (
                    &node.id,
                    &event.project_id,
                    scope,
                    scope_id,
                    unit_id,
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
            let source_scope =
                load_node_scope(tx, source_id)?.ok_or_else(|| anyhow!("关系源节点必须先存在"))?;
            let target_scope =
                load_node_scope(tx, target_id)?.ok_or_else(|| anyhow!("关系目标节点必须先存在"))?;
            if source_scope.project_id != target_scope.project_id
                || source_scope.scope != target_scope.scope
                || source_scope.scope_id != target_scope.scope_id
            {
                bail!("关系两端节点必须处于同一作用域");
            }
            tx.execute(
                "
                INSERT OR IGNORE INTO relations (
                    project_id,
                    scope,
                    scope_id,
                    unit_id,
                    repo_root,
                    source_id,
                    target_id,
                    kind,
                    source_event_id,
                    created_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ",
                (
                    &source_scope.project_id,
                    &source_scope.scope,
                    &source_scope.scope_id,
                    &source_scope.unit_id,
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
    query_nodes(
        conn,
        "
        SELECT id, kind, title, state_group, state_value, summary
        FROM nodes
        ORDER BY kind, title
        ",
        [],
    )
}

pub fn list_nodes_by_kind(conn: &Connection, kind: NodeKind) -> Result<Vec<NodeView>> {
    query_nodes(
        conn,
        "
        SELECT id, kind, title, state_group, state_value, summary
        FROM nodes
        WHERE kind = ?1
        ORDER BY title
        ",
        [kind_to_str(kind)],
    )
}

pub fn list_nodes_for_unit(
    conn: &Connection,
    project_id: &str,
    unit_id: &str,
) -> Result<Vec<NodeView>> {
    query_nodes(
        conn,
        "
        SELECT id, kind, title, state_group, state_value, summary
        FROM nodes
        WHERE project_id = ?1
          AND scope = 'execution_unit'
          AND scope_id = ?2
        ORDER BY kind, title
        ",
        params![project_id, unit_id],
    )
}

pub fn list_project_principles(conn: &Connection, project_id: &str) -> Result<Vec<NodeView>> {
    query_nodes(
        conn,
        "
        SELECT id, kind, title, state_group, state_value, summary
        FROM nodes
        WHERE project_id = ?1
          AND scope = 'project'
          AND kind IN ('principle', 'agreement')
        ORDER BY kind, title
        ",
        [project_id],
    )
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
    query_relations(
        conn,
        "
        SELECT source_id, target_id, kind
        FROM relations
        ORDER BY source_id, target_id, kind
        ",
        [],
    )
}

pub fn list_relations_for_unit(
    conn: &Connection,
    project_id: &str,
    unit_id: &str,
) -> Result<Vec<RelationView>> {
    query_relations(
        conn,
        "
        SELECT source_id, target_id, kind
        FROM relations
        WHERE project_id = ?1
          AND scope = 'execution_unit'
          AND scope_id = ?2
        ORDER BY source_id, target_id, kind
        ",
        params![project_id, unit_id],
    )
}

fn query_nodes<P>(conn: &Connection, sql: &str, params: P) -> Result<Vec<NodeView>>
where
    P: rusqlite::Params,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |row| {
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

fn query_relations<P>(conn: &Connection, sql: &str, params: P) -> Result<Vec<RelationView>>
where
    P: rusqlite::Params,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |row| {
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

fn load_node_scope(tx: &Transaction<'_>, node_id: &str) -> Result<Option<StoredNodeScope>> {
    let raw = tx
        .query_row(
            "
            SELECT project_id, scope, scope_id, unit_id
            FROM nodes
            WHERE id = ?1
            ",
            [node_id],
            |row| {
                Ok(StoredNodeScope {
                    project_id: row.get(0)?,
                    scope: row.get(1)?,
                    scope_id: row.get(2)?,
                    unit_id: row.get(3)?,
                })
            },
        )
        .optional()?;
    Ok(raw)
}

struct StoredNodeState {
    kind: NodeKind,
    state: NodeState,
}

struct StoredNodeScope {
    project_id: String,
    scope: String,
    scope_id: String,
    unit_id: Option<String>,
}

fn node_scope_columns(event: &StoredEvent, kind: NodeKind) -> (&'static str, &str, Option<&str>) {
    match node_scope(kind) {
        NodeScope::Project => ("project", event.project_id.as_str(), None),
        NodeScope::ExecutionUnit => (
            "execution_unit",
            event.unit_id.as_str(),
            Some(event.unit_id.as_str()),
        ),
    }
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
