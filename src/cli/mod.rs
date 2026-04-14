use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use sha2::{Digest, Sha256};

use crate::{
    domain::{
        NodeKind, NodeRecord, NodeState, PhaseState, RawEventKind, RawEventPayload, RelationKind,
        ReviewState, StoredEvent, StoredEventMeta, WorkState, kind_name, stable_node_id,
    },
    ingest::{find_latest_transcript_for_repo, import_codex_transcript},
    store::{IngestCheckpoint, NodeView, RelationView, Store},
};

#[derive(Debug, Parser)]
#[command(name = "aidoit")]
#[command(about = "Codex transcript 驱动的本地图谱 CLI")]
struct Cli {
    #[arg(long, global = true)]
    repo_root: Option<PathBuf>,
    #[arg(long, global = true)]
    db_path: Option<PathBuf>,
    #[arg(long, global = true)]
    transcript: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Status,
    Tree,
    Agenda,
    Principles,
    Inspect {
        #[arg(value_name = "ID_OR_PREFIX")]
        id: String,
    },
    SetStatus {
        #[arg(value_name = "ID_OR_PREFIX")]
        id: String,
        status: String,
    },
    Confirm {
        #[arg(value_name = "ID_OR_PREFIX")]
        id: String,
    },
    Reject {
        #[arg(value_name = "ID_OR_PREFIX")]
        id: String,
    },
    Promote {
        #[arg(value_name = "ID_OR_PREFIX")]
        id: String,
        #[arg(long)]
        title: Option<String>,
    },
}

struct AppContext {
    repo_root: PathBuf,
    db_path: PathBuf,
    transcript: Option<PathBuf>,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let context = resolve_context(&cli)?;
    let mut store = Store::open(&context.db_path)?;
    store.initialize()?;

    ensure_imported(&mut store, &context)?;

    let output = match cli.command {
        Commands::Status => render_status(&store)?,
        Commands::Tree => render_tree(&store)?,
        Commands::Agenda => render_agenda(&store)?,
        Commands::Principles => render_principles(&store)?,
        Commands::Inspect { id } => render_inspect(&store, &id)?,
        Commands::SetStatus { id, status } => {
            let node = resolve_node(&store, &id)?;
            apply_work_status(&mut store, &context, &node, &status)?;
            format!(
                "状态已更新：{} ({}) -> {status}",
                node.title,
                short_id(&node.id)
            )
        }
        Commands::Confirm { id } => {
            let node = resolve_node(&store, &id)?;
            apply_review_status(&mut store, &context, &node, ReviewState::Confirmed)?;
            format!("已确认：{} ({})", node.title, short_id(&node.id))
        }
        Commands::Reject { id } => {
            let node = resolve_node(&store, &id)?;
            apply_review_status(&mut store, &context, &node, ReviewState::Rejected)?;
            format!("已拒绝：{} ({})", node.title, short_id(&node.id))
        }
        Commands::Promote { id, title } => {
            let node = resolve_node(&store, &id)?;
            let promoted = promote_branch(&mut store, &context, &node, title.as_deref())?;
            format!(
                "已提升为任务：{} ({})",
                promoted.title,
                short_id(&promoted.id)
            )
        }
    };

    println!("{output}");
    Ok(())
}

fn resolve_context(cli: &Cli) -> Result<AppContext> {
    let start_dir = cli.repo_root.clone().unwrap_or(std::env::current_dir()?);
    let repo_root = normalize_repo_root(&start_dir);
    let db_path = match &cli.db_path {
        Some(path) => path.clone(),
        None => default_db_path(&repo_root)?,
    };
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }

    Ok(AppContext {
        repo_root,
        db_path,
        transcript: cli.transcript.clone(),
    })
}

fn normalize_repo_root(path: &Path) -> PathBuf {
    let normalized = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    discover_repo_root(&normalized).unwrap_or(normalized)
}

fn discover_repo_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() { path } else { path.parent()? };
    for ancestor in start.ancestors() {
        if ancestor.join(".git").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

fn default_db_path(repo_root: &Path) -> Result<PathBuf> {
    let project_dirs =
        ProjectDirs::from("io", "wayslog", "aidoit").context("无法确定本地数据目录")?;
    let repo_key = {
        let digest = Sha256::digest(repo_root.to_string_lossy().as_bytes());
        let digest = format!("{digest:x}");
        digest[..16].to_string()
    };
    Ok(project_dirs
        .data_local_dir()
        .join("repos")
        .join(format!("{repo_key}.sqlite3")))
}

fn ensure_imported(store: &mut Store, context: &AppContext) -> Result<()> {
    let transcript = match &context.transcript {
        Some(path) => Some(path.clone()),
        None => find_latest_transcript_for_repo(&context.repo_root)?,
    };

    if let Some(path) = transcript {
        import_codex_transcript(store, context.repo_root.to_string_lossy().as_ref(), path)?;
    }

    Ok(())
}

fn render_status(store: &Store) -> Result<String> {
    let nodes = store.list_nodes()?;
    let relations = store.list_relations()?;
    let tasks = nodes_of_kind(&nodes, NodeKind::Task);
    let blocked = work_nodes_by_state(&nodes, WorkState::Blocked);
    let candidates = agenda_items(&nodes, &relations);

    let mut lines = Vec::new();
    lines.push("主线任务".to_string());
    if let Some(task) = tasks.first() {
        lines.push(format!("- {}", format_node(task, false)));
    } else {
        lines.push("- 暂无任务".to_string());
    }
    lines.push(String::new());
    lines.push("阻塞项".to_string());
    if blocked.is_empty() {
        lines.push("- 暂无阻塞项".to_string());
    } else {
        for node in blocked {
            lines.push(format!("- {}", format_node(node, true)));
        }
    }
    lines.push(String::new());
    lines.push("候选分支".to_string());
    let candidate_branches: Vec<_> = candidates
        .into_iter()
        .filter(|node| node.kind == NodeKind::Branch)
        .collect();
    if candidate_branches.is_empty() {
        lines.push("- 暂无候选分支".to_string());
    } else {
        for node in candidate_branches {
            lines.push(format!("- {}", format_node(node, false)));
        }
    }

    Ok(lines.join("\n"))
}

fn render_tree(store: &Store) -> Result<String> {
    let nodes = store.list_nodes()?;
    let relations = store.list_relations()?;
    let node_map = node_map(&nodes);
    let child_relations: Vec<_> = relations
        .iter()
        .filter(|relation| relation.relation == RelationKind::ChildOf)
        .collect();
    let derived_relations: Vec<_> = relations
        .iter()
        .filter(|relation| relation.relation == RelationKind::DerivedFrom)
        .collect();

    let mut task_children: HashMap<String, Vec<&NodeView>> = HashMap::new();
    for relation in &child_relations {
        if let Some(child) = node_map.get(relation.source_id.as_str()) {
            task_children
                .entry(relation.target_id.clone())
                .or_default()
                .push(*child);
        }
    }

    let child_targets: Vec<_> = child_relations
        .iter()
        .map(|relation| relation.source_id.as_str())
        .collect();
    let mut root_tasks: Vec<_> = nodes
        .iter()
        .filter(|node| node.kind == NodeKind::Task)
        .filter(|node| !child_targets.contains(&node.id.as_str()))
        .collect();
    root_tasks.sort_by(|left, right| left.title.cmp(&right.title));

    let mut lines = vec!["任务树".to_string()];
    for task in root_tasks {
        lines.push(format!("- {}", format_node(task, true)));
        let mut children = task_children.remove(&task.id).unwrap_or_default();
        children.sort_by(|left, right| left.title.cmp(&right.title));
        for child in children {
            lines.push(format!("  - {}", format_node(child, true)));
        }
    }

    if !derived_relations.is_empty() {
        lines.push(String::new());
        lines.push("派生关系".to_string());
        for relation in derived_relations {
            let source = node_map.get(relation.source_id.as_str());
            let target = node_map.get(relation.target_id.as_str());
            if let (Some(source), Some(target)) = (source, target) {
                lines.push(format!(
                    "- task {} [{}] derived_from branch {} [{}]",
                    source.title,
                    display_state(&source.state),
                    target.title,
                    display_state(&target.state)
                ));
            }
        }
    }

    Ok(lines.join("\n"))
}

fn render_agenda(store: &Store) -> Result<String> {
    let nodes = store.list_nodes()?;
    let relations = store.list_relations()?;
    let agenda = agenda_items(&nodes, &relations);

    let mut lines = vec!["Agenda".to_string()];
    if agenda.is_empty() {
        lines.push("- 暂无可执行项".to_string());
    } else {
        for node in agenda {
            lines.push(format!("- {}", format_node(node, true)));
        }
    }
    Ok(lines.join("\n"))
}

fn render_principles(store: &Store) -> Result<String> {
    let nodes = store.list_nodes()?;
    let mut confirmed: Vec<_> = nodes
        .iter()
        .filter(|node| {
            matches!(node.kind, NodeKind::Principle | NodeKind::Agreement)
                && node.state == NodeState::Review(ReviewState::Confirmed)
        })
        .collect();
    confirmed.sort_by(|left, right| left.title.cmp(&right.title));

    let mut lines = vec!["已确认原则与协定".to_string()];
    if confirmed.is_empty() {
        lines.push("- 暂无已确认原则或协定".to_string());
    } else {
        for node in confirmed {
            lines.push(format!("- {}", format_node(node, true)));
        }
    }
    Ok(lines.join("\n"))
}

fn render_inspect(store: &Store, node_ref: &str) -> Result<String> {
    let node = resolve_node(store, node_ref)?;
    let nodes = store.list_nodes()?;
    let relations = store.list_relations()?;
    let node_map = node_map(&nodes);
    let events = store.list_raw_events()?;

    let mut lines = vec![
        "节点".to_string(),
        format!("- id: {}", node.id),
        format!("- 类型: {}", kind_name(node.kind)),
        format!("- 标题: {}", node.title),
        format!("- 状态: {}", display_state(&node.state)),
        String::new(),
        "关系".to_string(),
    ];

    let related: Vec<_> = relations
        .iter()
        .filter(|relation| relation.source_id == node.id || relation.target_id == node.id)
        .collect();
    if related.is_empty() {
        lines.push("- 暂无关系".to_string());
    } else {
        for relation in related {
            if relation.source_id == node.id {
                let target = node_map
                    .get(relation.target_id.as_str())
                    .map(|node| format!("{} {}", kind_name(node.kind), node.title))
                    .unwrap_or_else(|| relation.target_id.clone());
                lines.push(format!(
                    "- {} -> {}",
                    relation_name(relation.relation),
                    target
                ));
            } else {
                let source = node_map
                    .get(relation.source_id.as_str())
                    .map(|node| format!("{} {}", kind_name(node.kind), node.title))
                    .unwrap_or_else(|| relation.source_id.clone());
                lines.push(format!(
                    "- {} <- {}",
                    relation_name(relation.relation),
                    source
                ));
            }
        }
    }

    lines.push(String::new());
    lines.push("历史".to_string());
    let history: Vec<_> = events
        .into_iter()
        .filter(|event| event_mentions_node(event, &node.id))
        .collect();
    if history.is_empty() {
        lines.push("- 暂无历史".to_string());
    } else {
        for event in history {
            lines.push(format!(
                "- {} {}",
                event.occurred_at,
                describe_event(&event, &node_map)
            ));
        }
    }

    Ok(lines.join("\n"))
}

fn apply_work_status(
    store: &mut Store,
    context: &AppContext,
    node: &NodeView,
    status: &str,
) -> Result<()> {
    if !matches!(node.kind, NodeKind::Task | NodeKind::Branch) {
        bail!("只有 Task / Branch 支持工作流状态更新");
    }
    let state = NodeState::Work(parse_work_state(status)?);
    let event = manual_state_event(
        context.repo_root.to_string_lossy().as_ref(),
        &node.id,
        state,
        "CLI 手动更新状态",
    );
    apply_manual_events(store, context, &[event])
}

fn apply_review_status(
    store: &mut Store,
    context: &AppContext,
    node: &NodeView,
    status: ReviewState,
) -> Result<()> {
    if !matches!(node.kind, NodeKind::Principle | NodeKind::Agreement) {
        bail!("只有 Principle / Agreement 支持确认流转");
    }
    let event = manual_state_event(
        context.repo_root.to_string_lossy().as_ref(),
        &node.id,
        NodeState::Review(status),
        "CLI 手动确认状态",
    );
    apply_manual_events(store, context, &[event])
}

fn promote_branch(
    store: &mut Store,
    context: &AppContext,
    branch: &NodeView,
    title: Option<&str>,
) -> Result<NodeView> {
    if branch.kind != NodeKind::Branch {
        bail!("只有 Branch 可以提升为任务");
    }
    let task_title = title.unwrap_or(branch.title.as_str());
    let task_id = stable_node_id(
        context.repo_root.to_string_lossy().as_ref(),
        NodeKind::Task,
        task_title,
    );
    let task_state = match branch.state {
        NodeState::Work(state) => NodeState::Work(state),
        _ => NodeState::Work(WorkState::Parked),
    };
    let (line_no, occurred_at, seed) = manual_event_meta("promote");
    let node_event_id = manual_event_id("promote-node", &seed);
    let relation_event_id = manual_event_id("promote-relation", &seed);
    let node_event = StoredEvent::from_meta(
        StoredEventMeta::new(
            node_event_id.clone(),
            context.repo_root.to_string_lossy().as_ref(),
            "cli://manual",
            line_no,
            "cli",
            &occurred_at,
        ),
        RawEventKind::NodeCaptured,
        RawEventPayload::NodeCaptured {
            node: NodeRecord {
                id: task_id.clone(),
                kind: NodeKind::Task,
                title: task_title.to_string(),
                state: task_state.clone(),
                summary: Some("由分支提升生成".to_string()),
                source_event_id: node_event_id,
            },
        },
    );
    let relation_event = StoredEvent::from_meta(
        StoredEventMeta::new(
            relation_event_id,
            context.repo_root.to_string_lossy().as_ref(),
            "cli://manual",
            line_no + 1,
            "cli",
            &occurred_at,
        ),
        RawEventKind::RelationCaptured,
        RawEventPayload::RelationCaptured {
            source_id: task_id.clone(),
            target_id: branch.id.clone(),
            relation: RelationKind::DerivedFrom,
        },
    );

    apply_manual_events(store, context, &[node_event, relation_event])?;

    Ok(NodeView {
        id: task_id,
        kind: NodeKind::Task,
        title: task_title.to_string(),
        state: task_state,
        summary: Some("由分支提升生成".to_string()),
    })
}

fn apply_manual_events(
    store: &mut Store,
    context: &AppContext,
    events: &[StoredEvent],
) -> Result<()> {
    let max_line_no = events
        .iter()
        .map(|event| event.source_line_no)
        .max()
        .unwrap_or(0);
    let updated_at = events
        .last()
        .map(|event| event.occurred_at.as_str())
        .unwrap_or("manual-0");
    let checkpoint = IngestCheckpoint::new(
        context.repo_root.to_string_lossy(),
        "cli://manual",
        max_line_no,
        updated_at,
    );
    store.ingest_batch(events, &checkpoint)?;
    Ok(())
}

fn manual_state_event(
    repo_root: &str,
    node_id: &str,
    state: NodeState,
    reason: &str,
) -> StoredEvent {
    let (line_no, occurred_at, seed) = manual_event_meta("state");
    StoredEvent::from_meta(
        StoredEventMeta::new(
            manual_event_id("state", &seed),
            repo_root,
            "cli://manual",
            line_no,
            "cli",
            &occurred_at,
        ),
        RawEventKind::StateChanged,
        RawEventPayload::StateChanged {
            node_id: node_id.to_string(),
            state,
            reason: Some(reason.to_string()),
        },
    )
}

fn manual_event_meta(label: &str) -> (u64, String, String) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 Unix epoch");
    let line_no = now.as_micros() as u64;
    let occurred_at = format!("manual-{:020}", line_no);
    let seed = format!("{label}:{line_no}");
    (line_no, occurred_at, seed)
}

fn manual_event_id(label: &str, seed: &str) -> String {
    let digest = Sha256::digest(format!("{label}:{seed}").as_bytes());
    let digest = format!("{digest:x}");
    format!("manual-{}", &digest[..20])
}

fn parse_work_state(status: &str) -> Result<WorkState> {
    match status {
        "parked" => Ok(WorkState::Parked),
        "ready" => Ok(WorkState::Ready),
        "blocked" => Ok(WorkState::Blocked),
        "done" => Ok(WorkState::Done),
        "archived" => Ok(WorkState::Archived),
        _ => bail!("未知工作流状态: {status}"),
    }
}

fn display_state(state: &NodeState) -> &'static str {
    match state {
        NodeState::Work(WorkState::Parked) => "parked",
        NodeState::Work(WorkState::Ready) => "ready",
        NodeState::Work(WorkState::Blocked) => "blocked",
        NodeState::Work(WorkState::Done) => "done",
        NodeState::Work(WorkState::Archived) => "archived",
        NodeState::Review(ReviewState::Proposed) => "proposed",
        NodeState::Review(ReviewState::Confirmed) => "confirmed",
        NodeState::Review(ReviewState::Rejected) => "rejected",
        NodeState::Phase(PhaseState::Active) => "active",
        NodeState::Phase(PhaseState::Closed) => "closed",
    }
}

fn relation_name(relation: RelationKind) -> &'static str {
    match relation {
        RelationKind::DerivedFrom => "derived_from",
        RelationKind::Blocks => "blocks",
        RelationKind::DependsOn => "depends_on",
        RelationKind::Implements => "implements",
        RelationKind::Supersedes => "supersedes",
        RelationKind::RelatedTo => "related_to",
        RelationKind::ChildOf => "child_of",
    }
}

fn nodes_of_kind(nodes: &[NodeView], kind: NodeKind) -> Vec<&NodeView> {
    let mut result: Vec<_> = nodes.iter().filter(|node| node.kind == kind).collect();
    result.sort_by(|left, right| left.title.cmp(&right.title));
    result
}

fn work_nodes_by_state(nodes: &[NodeView], state: WorkState) -> Vec<&NodeView> {
    let mut result: Vec<_> = nodes
        .iter()
        .filter(|node| node.state == NodeState::Work(state))
        .collect();
    result.sort_by(|left, right| left.title.cmp(&right.title));
    result
}

fn agenda_items<'a>(nodes: &'a [NodeView], relations: &'a [RelationView]) -> Vec<&'a NodeView> {
    let node_map = node_map(nodes);
    let mut result: Vec<_> = nodes
        .iter()
        .filter(|node| matches!(node.kind, NodeKind::Task | NodeKind::Branch))
        .filter(|node| node.state == NodeState::Work(WorkState::Ready))
        .filter(|node| {
            relations
                .iter()
                .filter(|relation| relation.relation == RelationKind::DependsOn)
                .filter(|relation| relation.source_id == node.id)
                .all(|relation| {
                    node_map
                        .get(relation.target_id.as_str())
                        .map(|target| target.state == NodeState::Work(WorkState::Done))
                        .unwrap_or(false)
                })
        })
        .collect();
    result.sort_by(|left, right| {
        kind_name(left.kind)
            .cmp(kind_name(right.kind))
            .then(left.title.cmp(&right.title))
    });
    result
}

fn node_map(nodes: &[NodeView]) -> HashMap<&str, &NodeView> {
    nodes.iter().map(|node| (node.id.as_str(), node)).collect()
}

const SHORT_ID_LEN: usize = 12;

fn format_node(node: &NodeView, include_kind: bool) -> String {
    let prefix = if include_kind {
        format!("{} ", kind_name(node.kind))
    } else {
        String::new()
    };
    format!(
        "{}{} [{}] (id: {})",
        prefix,
        node.title,
        display_state(&node.state),
        short_id(&node.id)
    )
}

fn short_id(id: &str) -> &str {
    &id[..id.len().min(SHORT_ID_LEN)]
}

fn resolve_node(store: &Store, node_ref: &str) -> Result<NodeView> {
    let nodes = store.list_nodes()?;
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
        [] => bail!("节点不存在: {node_ref}"),
        _ => {
            let options = matches
                .iter()
                .map(|node| format!("- {}", format_node(node, true)))
                .collect::<Vec<_>>()
                .join("\n");
            bail!("节点前缀不唯一: {node_ref}\n{options}");
        }
    }
}

fn event_mentions_node(event: &StoredEvent, node_id: &str) -> bool {
    match &event.payload {
        RawEventPayload::NodeCaptured { node } => node.id == node_id,
        RawEventPayload::RelationCaptured {
            source_id,
            target_id,
            relation: _,
        } => source_id == node_id || target_id == node_id,
        RawEventPayload::StateChanged {
            node_id: changed_node_id,
            state: _,
            reason: _,
        } => changed_node_id == node_id,
    }
}

fn describe_event(event: &StoredEvent, node_map: &HashMap<&str, &NodeView>) -> String {
    match &event.payload {
        RawEventPayload::NodeCaptured { node } => {
            format!("node_captured {} {}", kind_name(node.kind), node.title)
        }
        RawEventPayload::RelationCaptured {
            source_id,
            target_id,
            relation,
        } => {
            let source = node_map
                .get(source_id.as_str())
                .map(|node| node.title.as_str())
                .unwrap_or(source_id.as_str());
            let target = node_map
                .get(target_id.as_str())
                .map(|node| node.title.as_str())
                .unwrap_or(target_id.as_str());
            format!("{} {} -> {}", relation_name(*relation), source, target)
        }
        RawEventPayload::StateChanged {
            node_id,
            state,
            reason: _,
        } => {
            let title = node_map
                .get(node_id.as_str())
                .map(|node| node.title.as_str())
                .unwrap_or(node_id.as_str());
            format!("state_changed {} -> {}", title, display_state(state))
        }
    }
}
