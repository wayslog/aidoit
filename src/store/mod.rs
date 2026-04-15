mod raw_events;
mod read_models;
mod schema;

use std::{cmp::Reverse, collections::HashSet, path::Path};

use anyhow::Result;
use rusqlite::Connection;

use crate::domain::{
    ExecutionUnit, ExecutionUnitKind, NodeKind, NodeState, RawEventPayload, RelationKind,
    StoredEvent, WorkState, kind_name, list_project_execution_units,
};

pub use raw_events::{IngestCheckpoint, load_checkpoint};
pub use read_models::{NodeView, RelationView};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestOutcome {
    pub inserted_raw_events: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitScope {
    pub nodes: Vec<NodeView>,
    pub relations: Vec<RelationView>,
    pub raw_events: Vec<StoredEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectScope {
    pub node: NodeView,
    pub nodes: Vec<NodeView>,
    pub relations: Vec<RelationView>,
    pub raw_events: Vec<StoredEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkCounts {
    pub ready: usize,
    pub blocked: usize,
    pub done: usize,
    pub parked: usize,
    pub archived: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitSummary {
    pub execution_unit: ExecutionUnit,
    pub is_current: bool,
    pub work_counts: WorkCounts,
    pub main_task: Option<NodeView>,
    pub candidate_branches: Vec<NodeView>,
    pub blocked_items: Vec<NodeView>,
    pub ready_items: Vec<NodeView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalAgendaLens {
    Urgent,
    Easy,
    Mainline,
    LowSwitch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgendaSubjectKind {
    Task,
    Branch,
    Summary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalAgendaEntry {
    pub execution_unit: ExecutionUnit,
    pub is_current_unit: bool,
    pub subject_kind: AgendaSubjectKind,
    pub subject_title: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalAgenda {
    pub lens: GlobalAgendaLens,
    pub rationale: String,
    pub items: Vec<GlobalAgendaEntry>,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        initialize_connection(&conn)?;
        let store = Self { conn };
        store.initialize()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        initialize_connection(&conn)?;
        let store = Self { conn };
        store.initialize()?;
        Ok(store)
    }

    pub fn initialize(&self) -> Result<()> {
        schema::initialize(&self.conn)
    }

    pub fn has_table(&self, table_name: &str) -> Result<bool> {
        let exists = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table_name],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(exists == 1)
    }

    pub fn foreign_keys_enabled(&self) -> Result<bool> {
        let enabled = self
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))?;
        Ok(enabled == 1)
    }

    pub fn ingest_batch(
        &mut self,
        events: &[StoredEvent],
        checkpoint: &IngestCheckpoint,
    ) -> Result<IngestOutcome> {
        let tx = self.conn.transaction()?;
        let mut inserted_raw_events = 0;
        let mut pending_projection = Vec::new();

        for event in events {
            raw_events::upsert_execution_unit_from_event(&tx, event)?;
            if raw_events::insert_raw_event(&tx, event)? {
                inserted_raw_events += 1;
                pending_projection.push(event);
            }
        }

        pending_projection.sort_by(|left, right| {
            left.source_line_no
                .cmp(&right.source_line_no)
                .then(event_priority(left).cmp(&event_priority(right)))
        });
        for event in pending_projection {
            read_models::apply_event(&tx, event)?;
        }

        raw_events::upsert_execution_unit_from_checkpoint(&tx, checkpoint)?;
        raw_events::upsert_checkpoint(&tx, checkpoint)?;
        tx.commit()?;

        Ok(IngestOutcome {
            inserted_raw_events,
        })
    }

    pub fn checkpoint(
        &self,
        repo_root: &str,
        transcript_path: &str,
    ) -> Result<Option<IngestCheckpoint>> {
        raw_events::load_checkpoint(&self.conn, repo_root, transcript_path)
    }

    pub fn checkpoint_for_unit(
        &self,
        project_id: &str,
        unit_id: &str,
        transcript_path: &str,
    ) -> Result<Option<IngestCheckpoint>> {
        raw_events::load_checkpoint_for_unit(&self.conn, project_id, unit_id, transcript_path)
    }

    pub fn raw_event_count(&self) -> Result<usize> {
        raw_events::count_raw_events(&self.conn)
    }

    pub fn node_count(&self) -> Result<usize> {
        read_models::count_nodes(&self.conn)
    }

    pub fn list_raw_events(&self) -> Result<Vec<StoredEvent>> {
        raw_events::list_raw_events(&self.conn)
    }

    pub fn list_raw_events_for_project(&self, project_id: &str) -> Result<Vec<StoredEvent>> {
        raw_events::list_raw_events_for_project(&self.conn, project_id)
    }

    pub fn list_raw_events_for_unit(
        &self,
        project_id: &str,
        unit_id: &str,
    ) -> Result<Vec<StoredEvent>> {
        raw_events::list_raw_events_for_unit(&self.conn, project_id, unit_id)
    }

    pub fn list_nodes(&self) -> Result<Vec<NodeView>> {
        read_models::list_nodes(&self.conn)
    }

    pub fn list_nodes_by_kind(&self, kind: NodeKind) -> Result<Vec<NodeView>> {
        read_models::list_nodes_by_kind(&self.conn, kind)
    }

    pub fn list_nodes_for_unit(&self, project_id: &str, unit_id: &str) -> Result<Vec<NodeView>> {
        read_models::list_nodes_for_unit(&self.conn, project_id, unit_id)
    }

    pub fn list_project_principles(&self, project_id: &str) -> Result<Vec<NodeView>> {
        read_models::list_project_principles(&self.conn, project_id)
    }

    pub fn get_node(&self, node_id: &str) -> Result<Option<NodeView>> {
        read_models::get_node(&self.conn, node_id)
    }

    pub fn list_relations(&self) -> Result<Vec<RelationView>> {
        read_models::list_relations(&self.conn)
    }

    pub fn list_relations_for_unit(
        &self,
        project_id: &str,
        unit_id: &str,
    ) -> Result<Vec<RelationView>> {
        read_models::list_relations_for_unit(&self.conn, project_id, unit_id)
    }

    pub fn list_execution_units(&self, project_id: &str) -> Result<Vec<ExecutionUnit>> {
        raw_events::list_execution_units(&self.conn, project_id)
    }

    pub fn sync_project_execution_units(
        &mut self,
        current: &ExecutionUnit,
    ) -> Result<Vec<ExecutionUnit>> {
        let units = if current.project_id == current.project_root {
            vec![current.clone().with_active(true)]
        } else {
            list_project_execution_units(current)?
        };
        let tx = self.conn.transaction()?;
        raw_events::upsert_project_execution_units(&tx, &current.project_id, &units, "sync")?;
        tx.commit()?;
        Ok(units)
    }

    pub fn load_unit_scope(&self, project_id: &str, unit_id: &str) -> Result<UnitScope> {
        let mut nodes = read_models::list_nodes_for_unit(&self.conn, project_id, unit_id)?;
        nodes.extend(read_models::list_project_principles(
            &self.conn, project_id,
        )?);
        nodes.sort_by(|left, right| {
            crate::domain::kind_name(left.kind)
                .cmp(crate::domain::kind_name(right.kind))
                .then(left.title.cmp(&right.title))
        });

        Ok(UnitScope {
            nodes,
            relations: read_models::list_relations_for_unit(&self.conn, project_id, unit_id)?,
            raw_events: raw_events::list_raw_events_for_unit(&self.conn, project_id, unit_id)?,
        })
    }

    pub fn load_inspect_scope(
        &self,
        project_id: &str,
        unit_id: &str,
        node_ref: &str,
    ) -> Result<InspectScope> {
        let scope = self.load_unit_scope(project_id, unit_id)?;
        let node = resolve_node_ref(&scope.nodes, node_ref)?;
        Ok(InspectScope {
            node,
            nodes: scope.nodes,
            relations: scope.relations,
            raw_events: scope.raw_events,
        })
    }

    pub fn load_project_unit_summaries(&self, current: &ExecutionUnit) -> Result<Vec<UnitSummary>> {
        let units = if current.project_id == current.project_root {
            vec![current.clone().with_active(true)]
        } else {
            list_project_execution_units(current)?
        };
        let mut summaries = units
            .into_iter()
            .map(|execution_unit| self.build_unit_summary(current, execution_unit))
            .collect::<Result<Vec<_>>>()?;
        summaries.sort_by(|left, right| {
            right.is_current.cmp(&left.is_current).then(
                left.execution_unit
                    .unit_root
                    .cmp(&right.execution_unit.unit_root),
            )
        });
        Ok(summaries)
    }

    pub fn load_global_agenda(
        &self,
        current: &ExecutionUnit,
        lens: GlobalAgendaLens,
        limit: usize,
    ) -> Result<GlobalAgenda> {
        let summaries = self.load_project_unit_summaries(current)?;
        let mut candidates = summaries
            .iter()
            .flat_map(build_agenda_candidates)
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            candidate_score(right, lens)
                .cmp(&candidate_score(left, lens))
                .then(
                    left.execution_unit
                        .unit_root
                        .cmp(&right.execution_unit.unit_root),
                )
                .then(left.subject_title.cmp(&right.subject_title))
        });

        let items = candidates
            .into_iter()
            .take(limit)
            .map(|candidate| {
                let reason = candidate_reason(&candidate, lens);
                GlobalAgendaEntry {
                    execution_unit: candidate.execution_unit,
                    is_current_unit: candidate.is_current,
                    subject_kind: candidate.subject_kind,
                    subject_title: candidate.subject_title,
                    reason,
                }
            })
            .collect();

        Ok(GlobalAgenda {
            lens,
            rationale: lens_rationale(lens).to_string(),
            items,
        })
    }

    fn build_unit_summary(
        &self,
        current: &ExecutionUnit,
        execution_unit: ExecutionUnit,
    ) -> Result<UnitSummary> {
        let scope = self.load_unit_scope(&execution_unit.project_id, &execution_unit.unit_id)?;
        let tasks = scope
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Task)
            .cloned()
            .collect::<Vec<_>>();
        let ready_items = work_nodes_with_state(&scope.nodes, WorkState::Ready);
        let blocked_items = work_nodes_with_state(&scope.nodes, WorkState::Blocked);
        let candidate_branches = ready_items
            .iter()
            .filter(|node| node.kind == NodeKind::Branch)
            .cloned()
            .collect::<Vec<_>>();

        Ok(UnitSummary {
            execution_unit: execution_unit.clone(),
            is_current: execution_unit.unit_id == current.unit_id,
            work_counts: count_work_items(&scope.nodes),
            main_task: select_main_task(&tasks, &scope.relations),
            candidate_branches,
            blocked_items,
            ready_items,
        })
    }
}

fn event_priority(event: &StoredEvent) -> u8 {
    match event.payload {
        RawEventPayload::NodeCaptured { .. } => 0,
        RawEventPayload::StateChanged { .. } => 1,
        RawEventPayload::RelationCaptured { .. } => 2,
    }
}

fn initialize_connection(conn: &Connection) -> Result<()> {
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    Ok(())
}

fn resolve_node_ref(nodes: &[NodeView], node_ref: &str) -> Result<NodeView> {
    if let Some(node) = nodes.iter().find(|node| node.id == node_ref) {
        return Ok(node.clone());
    }

    let matches: Vec<_> = nodes
        .iter()
        .filter(|node| node.id.starts_with(node_ref))
        .cloned()
        .collect();
    match matches.as_slice() {
        [node] => Ok(node.clone()),
        [] => anyhow::bail!("节点不存在: {node_ref}"),
        _ => {
            let options = matches
                .iter()
                .map(|node| {
                    format!(
                        "- {} {} [{}]",
                        crate::domain::kind_name(node.kind),
                        node.title,
                        crate::domain::kind_name(node.kind)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            anyhow::bail!("节点前缀不唯一: {node_ref}\n{options}");
        }
    }
}

#[derive(Debug, Clone)]
struct AgendaCandidate {
    execution_unit: ExecutionUnit,
    is_current: bool,
    subject_kind: AgendaSubjectKind,
    subject_title: String,
    is_main_task: bool,
    is_ready: bool,
    blocked_count: usize,
    ready_count: usize,
    top_blocked_title: Option<String>,
}

fn count_work_items(nodes: &[NodeView]) -> WorkCounts {
    let mut counts = WorkCounts::default();
    for node in nodes {
        match node.state {
            NodeState::Work(WorkState::Ready) => counts.ready += 1,
            NodeState::Work(WorkState::Blocked) => counts.blocked += 1,
            NodeState::Work(WorkState::Done) => counts.done += 1,
            NodeState::Work(WorkState::Parked) => counts.parked += 1,
            NodeState::Work(WorkState::Archived) => counts.archived += 1,
            _ => {}
        }
    }
    counts
}

fn work_nodes_with_state(nodes: &[NodeView], state: WorkState) -> Vec<NodeView> {
    let mut items = nodes
        .iter()
        .filter(|node| matches!(node.kind, NodeKind::Task | NodeKind::Branch))
        .filter(|node| node.state == NodeState::Work(state))
        .cloned()
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        kind_name(left.kind)
            .cmp(kind_name(right.kind))
            .then(left.title.cmp(&right.title))
    });
    items
}

fn select_main_task(tasks: &[NodeView], relations: &[RelationView]) -> Option<NodeView> {
    if tasks.is_empty() {
        return None;
    }

    let child_tasks = relations
        .iter()
        .filter(|relation| relation.relation == RelationKind::ChildOf)
        .map(|relation| relation.source_id.as_str())
        .collect::<HashSet<_>>();
    let mut roots = tasks
        .iter()
        .filter(|task| !child_tasks.contains(task.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if roots.is_empty() {
        roots = tasks.to_vec();
    }
    roots.sort_by(|left, right| {
        work_state_rank(&left.state)
            .cmp(&work_state_rank(&right.state))
            .then(left.title.cmp(&right.title))
    });
    roots.into_iter().next()
}

fn work_state_rank(state: &NodeState) -> u8 {
    match state {
        NodeState::Work(WorkState::Ready) => 0,
        NodeState::Work(WorkState::Blocked) => 1,
        NodeState::Work(WorkState::Parked) => 2,
        NodeState::Work(WorkState::Done) => 3,
        NodeState::Work(WorkState::Archived) => 4,
        _ => 5,
    }
}

fn build_agenda_candidates(summary: &UnitSummary) -> Vec<AgendaCandidate> {
    let mut candidates = Vec::new();
    let top_blocked_title = summary.blocked_items.first().map(|node| node.title.clone());

    if summary.work_counts.blocked > 0 {
        candidates.push(AgendaCandidate {
            execution_unit: summary.execution_unit.clone(),
            is_current: summary.is_current,
            subject_kind: AgendaSubjectKind::Summary,
            subject_title: "处理阻塞".to_string(),
            is_main_task: false,
            is_ready: false,
            blocked_count: summary.work_counts.blocked,
            ready_count: summary.work_counts.ready,
            top_blocked_title: top_blocked_title.clone(),
        });
    }

    if let Some(main_task) = &summary.main_task
        && main_task.state != NodeState::Work(WorkState::Done)
        && main_task.state != NodeState::Work(WorkState::Archived)
    {
        candidates.push(AgendaCandidate {
            execution_unit: summary.execution_unit.clone(),
            is_current: summary.is_current,
            subject_kind: AgendaSubjectKind::Task,
            subject_title: main_task.title.clone(),
            is_main_task: true,
            is_ready: main_task.state == NodeState::Work(WorkState::Ready),
            blocked_count: summary.work_counts.blocked,
            ready_count: summary.work_counts.ready,
            top_blocked_title: top_blocked_title.clone(),
        });
    }

    for branch in &summary.candidate_branches {
        candidates.push(AgendaCandidate {
            execution_unit: summary.execution_unit.clone(),
            is_current: summary.is_current,
            subject_kind: AgendaSubjectKind::Branch,
            subject_title: branch.title.clone(),
            is_main_task: false,
            is_ready: true,
            blocked_count: summary.work_counts.blocked,
            ready_count: summary.work_counts.ready,
            top_blocked_title: top_blocked_title.clone(),
        });
    }

    candidates
}

fn candidate_score(
    candidate: &AgendaCandidate,
    lens: GlobalAgendaLens,
) -> (i64, i64, i64, i64, i64, Reverse<usize>) {
    match lens {
        GlobalAgendaLens::Urgent => (
            match candidate.subject_kind {
                AgendaSubjectKind::Summary => 400,
                AgendaSubjectKind::Task => 250,
                AgendaSubjectKind::Branch => 150,
            },
            candidate.blocked_count as i64,
            candidate.is_main_task as i64,
            candidate.is_current as i64,
            candidate.is_ready as i64,
            Reverse(candidate.subject_title.chars().count()),
        ),
        GlobalAgendaLens::Easy => (
            match candidate.subject_kind {
                AgendaSubjectKind::Branch => 400,
                AgendaSubjectKind::Task => 250,
                AgendaSubjectKind::Summary => 100,
            },
            candidate.is_ready as i64,
            -(candidate.subject_title.chars().count() as i64),
            -(candidate.blocked_count as i64),
            candidate.ready_count as i64,
            Reverse(candidate.subject_title.chars().count()),
        ),
        GlobalAgendaLens::Mainline => (
            candidate.is_main_task as i64 * 500
                + match candidate.execution_unit.unit_kind {
                    ExecutionUnitKind::MainRepo => 100,
                    ExecutionUnitKind::LinkedWorktree => 0,
                },
            candidate.is_ready as i64,
            -(candidate.blocked_count as i64),
            match candidate.subject_kind {
                AgendaSubjectKind::Task => 2,
                AgendaSubjectKind::Summary => 1,
                AgendaSubjectKind::Branch => 0,
            },
            candidate.is_current as i64,
            Reverse(candidate.subject_title.chars().count()),
        ),
        GlobalAgendaLens::LowSwitch => (
            candidate.is_current as i64 * 500,
            match candidate.subject_kind {
                AgendaSubjectKind::Task => 250,
                AgendaSubjectKind::Branch => 150,
                AgendaSubjectKind::Summary => 50,
            },
            candidate.is_ready as i64,
            -(candidate.blocked_count as i64),
            candidate.ready_count as i64,
            Reverse(candidate.subject_title.chars().count()),
        ),
    }
}

fn lens_rationale(lens: GlobalAgendaLens) -> &'static str {
    match lens {
        GlobalAgendaLens::Urgent => "优先把有阻塞压力的 unit 放前面，先解堵再推进。",
        GlobalAgendaLens::Easy => "优先推荐切口更小的 ready 分支，其次才是主线 task。",
        GlobalAgendaLens::Mainline => "优先推荐主线 task，并偏向主仓库的主线推进。",
        GlobalAgendaLens::LowSwitch => "优先留在当前 execution unit，尽量减少上下文切换。",
    }
}

fn candidate_reason(candidate: &AgendaCandidate, lens: GlobalAgendaLens) -> String {
    match lens {
        GlobalAgendaLens::Urgent => match candidate.subject_kind {
            AgendaSubjectKind::Summary => format!(
                "该 unit 有 {} 个 blocked 项，先处理最急的阻塞：{}",
                candidate.blocked_count,
                candidate.top_blocked_title.as_deref().unwrap_or("暂无明细")
            ),
            AgendaSubjectKind::Task => "主线 task 已经可见，解堵后可直接承接推进。".to_string(),
            AgendaSubjectKind::Branch => {
                "该 branch 已 ready，可作为解堵后的次优推进项。".to_string()
            }
        },
        GlobalAgendaLens::Easy => match candidate.subject_kind {
            AgendaSubjectKind::Branch => "ready branch 切口更小，适合快速推进。".to_string(),
            AgendaSubjectKind::Task => {
                if candidate.is_ready {
                    "主线 task 已 ready，但通常比 branch 更大。".to_string()
                } else {
                    "主线 task 当前未 ready，只作为后备项保留。".to_string()
                }
            }
            AgendaSubjectKind::Summary => "当前没有更小的现成切口，先处理阻塞。".to_string(),
        },
        GlobalAgendaLens::Mainline => {
            if candidate.is_main_task {
                "这是该 unit 的主线 task，最贴近主线推进。".to_string()
            } else if candidate.subject_kind == AgendaSubjectKind::Summary {
                "主线推进前仍有阻塞，需要先清障。".to_string()
            } else {
                "这是主线附近的备选分支，可在主线清晰后接着做。".to_string()
            }
        }
        GlobalAgendaLens::LowSwitch => {
            if candidate.is_current {
                "留在当前 unit，切换成本最低。".to_string()
            } else {
                "需要切换到别的 unit，优先级因此后移。".to_string()
            }
        }
    }
}
