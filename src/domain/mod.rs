use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionUnitKind {
    MainRepo,
    LinkedWorktree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeScope {
    Project,
    ExecutionUnit,
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
    pub project_id: String,
    pub unit_id: String,
    pub project_root: String,
    pub unit_root: String,
    pub unit_kind: ExecutionUnitKind,
    pub branch_ref: Option<String>,
    pub head_oid: Option<String>,
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
    pub project_id: String,
    pub unit_id: String,
    pub project_root: String,
    pub unit_root: String,
    pub unit_kind: ExecutionUnitKind,
    pub branch_ref: Option<String>,
    pub head_oid: Option<String>,
    pub repo_root: String,
    pub transcript_path: String,
    pub source_line_no: u64,
    pub session_id: String,
    pub occurred_at: String,
}

impl StoredEventMeta {
    pub fn for_execution_unit(
        event_id: impl Into<String>,
        execution_unit: &ExecutionUnit,
        transcript_path: impl Into<String>,
        source_line_no: u64,
        session_id: impl Into<String>,
        occurred_at: impl Into<String>,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            project_id: execution_unit.project_id.clone(),
            unit_id: execution_unit.unit_id.clone(),
            project_root: execution_unit.project_root.clone(),
            unit_root: execution_unit.unit_root.clone(),
            unit_kind: execution_unit.unit_kind,
            branch_ref: execution_unit.branch_ref.clone(),
            head_oid: execution_unit.head_oid.clone(),
            repo_root: execution_unit.project_root.clone(),
            transcript_path: transcript_path.into(),
            source_line_no,
            session_id: session_id.into(),
            occurred_at: occurred_at.into(),
        }
    }

    pub fn new(
        event_id: impl Into<String>,
        repo_root: impl Into<String>,
        transcript_path: impl Into<String>,
        source_line_no: u64,
        session_id: impl Into<String>,
        occurred_at: impl Into<String>,
    ) -> Self {
        let execution_unit = ExecutionUnit::legacy_main(repo_root.into());
        Self::for_execution_unit(
            event_id,
            &execution_unit,
            transcript_path,
            source_line_no,
            session_id,
            occurred_at,
        )
    }
}

impl StoredEvent {
    pub fn from_meta(meta: StoredEventMeta, kind: RawEventKind, payload: RawEventPayload) -> Self {
        Self {
            event_id: meta.event_id,
            project_id: meta.project_id,
            unit_id: meta.unit_id,
            project_root: meta.project_root,
            unit_root: meta.unit_root,
            unit_kind: meta.unit_kind,
            branch_ref: meta.branch_ref,
            head_oid: meta.head_oid,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionUnit {
    pub project_id: String,
    pub unit_id: String,
    pub project_root: String,
    pub unit_root: String,
    pub unit_kind: ExecutionUnitKind,
    pub branch_ref: Option<String>,
    pub head_oid: Option<String>,
    pub is_active: bool,
}

impl ExecutionUnit {
    pub fn legacy_main(project_root: impl Into<String>) -> Self {
        let project_root = project_root.into();
        Self {
            project_id: project_root.clone(),
            unit_id: project_root.clone(),
            project_root: project_root.clone(),
            unit_root: project_root,
            unit_kind: ExecutionUnitKind::MainRepo,
            branch_ref: None,
            head_oid: None,
            is_active: false,
        }
    }

    pub fn with_active(mut self, is_active: bool) -> Self {
        self.is_active = is_active;
        self
    }
}

pub fn node_scope(kind: NodeKind) -> NodeScope {
    match kind {
        NodeKind::Principle | NodeKind::Agreement => NodeScope::Project,
        NodeKind::Task | NodeKind::Branch | NodeKind::Phase => NodeScope::ExecutionUnit,
    }
}

pub fn node_scope_key(execution_unit: &ExecutionUnit, kind: NodeKind) -> &str {
    match node_scope(kind) {
        NodeScope::Project => execution_unit.project_id.as_str(),
        NodeScope::ExecutionUnit => execution_unit.unit_id.as_str(),
    }
}

pub fn stable_node_id(scope_key: &str, kind: NodeKind, title: &str) -> String {
    let digest = Sha256::digest(format!("{scope_key}:{}:{title}", kind_name(kind)).as_bytes());
    let digest = format!("{digest:x}");
    format!("{}-{}", kind_name(kind), &digest[..16])
}

pub fn stable_node_id_for_unit(
    execution_unit: &ExecutionUnit,
    kind: NodeKind,
    title: &str,
) -> String {
    stable_node_id(node_scope_key(execution_unit, kind), kind, title)
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

pub fn discover_execution_unit(path: impl AsRef<Path>) -> Result<ExecutionUnit> {
    let unit_root = discover_unit_root(path.as_ref())?;
    let git_marker = unit_root.join(".git");
    let git_dir = resolve_git_dir(&unit_root, &git_marker)?;
    let common_dir = resolve_common_dir(&git_dir)?;
    let project_root = common_dir
        .parent()
        .context("无法从 git common dir 推导 project_root")?
        .to_path_buf();
    let unit_kind = if git_dir == common_dir {
        ExecutionUnitKind::MainRepo
    } else {
        ExecutionUnitKind::LinkedWorktree
    };
    let head_text = read_trimmed_file(&git_dir.join("HEAD")).ok();
    let branch_ref = head_text
        .as_deref()
        .and_then(|head| head.strip_prefix("ref:"))
        .map(str::trim)
        .map(str::to_string);
    let head_oid = resolve_head_oid(
        &git_dir,
        &common_dir,
        head_text.as_deref(),
        branch_ref.as_deref(),
    );

    Ok(ExecutionUnit {
        project_id: stable_identity("project", &common_dir.to_string_lossy()),
        unit_id: stable_identity(
            "unit",
            &format!(
                "{}:{}",
                common_dir.to_string_lossy(),
                unit_root.to_string_lossy()
            ),
        ),
        project_root: project_root.to_string_lossy().to_string(),
        unit_root: unit_root.to_string_lossy().to_string(),
        unit_kind,
        branch_ref,
        head_oid,
        is_active: false,
    })
}

pub fn resolve_execution_unit(path: impl AsRef<Path>) -> Result<Option<ExecutionUnit>> {
    let path = path.as_ref();
    if !has_git_ancestor(path) {
        return Ok(None);
    }
    discover_execution_unit(path).map(Some)
}

pub fn list_project_execution_units(current: &ExecutionUnit) -> Result<Vec<ExecutionUnit>> {
    let project_root = Path::new(&current.project_root);
    let main_unit = discover_execution_unit(project_root)?;
    let main_is_active = main_unit.unit_id == current.unit_id;
    let main_unit = main_unit.with_active(main_is_active);
    let mut units = vec![main_unit];
    let worktrees_root = project_root.join(".git").join("worktrees");
    if worktrees_root.exists() {
        for entry in fs::read_dir(worktrees_root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let gitdir_path = entry.path().join("gitdir");
            let Ok(worktree_git_marker) = read_git_path(&gitdir_path, &entry.path()) else {
                continue;
            };
            let Some(worktree_root) = worktree_git_marker.parent() else {
                continue;
            };
            let Ok(unit) = discover_execution_unit(worktree_root) else {
                continue;
            };
            let is_active = unit.unit_id == current.unit_id;
            let unit = unit.with_active(is_active);
            if !units.iter().any(|known| known.unit_id == unit.unit_id) {
                units.push(unit);
            }
        }
    }
    units.sort_by(|left, right| left.unit_root.cmp(&right.unit_root));
    Ok(units)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("非法状态迁移")]
    InvalidTransition,
}

fn stable_identity(kind: &str, raw: &str) -> String {
    let digest = Sha256::digest(format!("{kind}:{raw}").as_bytes());
    let digest = format!("{digest:x}");
    format!("{kind}-{}", &digest[..16])
}

fn has_git_ancestor(path: &Path) -> bool {
    let normalized = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let start = if normalized.is_dir() {
        normalized
    } else if let Some(parent) = normalized.parent() {
        parent.to_path_buf()
    } else {
        return false;
    };

    for ancestor in start.ancestors() {
        let git_marker = ancestor.join(".git");
        if git_marker.is_dir() || git_marker.is_file() {
            return true;
        }
    }

    false
}

fn discover_unit_root(path: &Path) -> Result<PathBuf> {
    let normalized = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let start = if normalized.is_dir() {
        normalized
    } else {
        normalized
            .parent()
            .map(Path::to_path_buf)
            .context("无法定位文件所在目录")?
    };

    for ancestor in start.ancestors() {
        let git_marker = ancestor.join(".git");
        if git_marker.is_dir() || git_marker.is_file() {
            return Ok(ancestor.to_path_buf());
        }
    }

    bail!("当前目录不在 git 仓库内")
}

fn resolve_git_dir(unit_root: &Path, git_marker: &Path) -> Result<PathBuf> {
    if git_marker.is_dir() {
        return Ok(fs::canonicalize(git_marker).unwrap_or_else(|_| git_marker.to_path_buf()));
    }
    if git_marker.is_file() {
        return read_git_path(git_marker, unit_root);
    }
    bail!("缺少 .git 标记")
}

fn resolve_common_dir(git_dir: &Path) -> Result<PathBuf> {
    let common_dir_file = git_dir.join("commondir");
    if !common_dir_file.exists() {
        return Ok(git_dir.to_path_buf());
    }
    read_git_path(&common_dir_file, git_dir)
}

fn read_git_path(path_file: &Path, base_dir: &Path) -> Result<PathBuf> {
    let content = read_trimmed_file(path_file)?;
    let raw_path = content
        .strip_prefix("gitdir:")
        .or_else(|| content.strip_prefix("commondir:"))
        .map(str::trim)
        .unwrap_or(content.as_str());
    let candidate = Path::new(raw_path);
    let resolved = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        base_dir.join(candidate)
    };
    if !resolved.exists() {
        bail!("git 元数据路径不存在: {}", resolved.display());
    }
    Ok(fs::canonicalize(&resolved).unwrap_or(resolved))
}

fn read_trimmed_file(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)
        .with_context(|| format!("读取文件失败: {}", path.display()))?
        .trim()
        .to_string())
}

fn resolve_head_oid(
    git_dir: &Path,
    common_dir: &Path,
    head_text: Option<&str>,
    branch_ref: Option<&str>,
) -> Option<String> {
    if let Some(branch_ref) = branch_ref {
        for root in [common_dir, git_dir] {
            if let Ok(oid) = read_trimmed_file(&root.join(branch_ref))
                && !oid.is_empty()
            {
                return Some(oid);
            }
        }
        for root in [common_dir, git_dir] {
            if let Some(oid) = lookup_packed_ref(root, branch_ref) {
                return Some(oid);
            }
        }
    }

    head_text.and_then(|head| {
        let head = head.trim();
        if head.starts_with("ref:") || head.is_empty() {
            None
        } else {
            Some(head.to_string())
        }
    })
}

fn lookup_packed_ref(root: &Path, branch_ref: &str) -> Option<String> {
    let packed_refs = root.join("packed-refs");
    let content = fs::read_to_string(packed_refs).ok()?;
    for line in content.lines() {
        if line.starts_with('#') || line.starts_with('^') {
            continue;
        }
        let (oid, name) = line.split_once(' ')?;
        if name.trim() == branch_ref {
            return Some(oid.trim().to_string());
        }
    }
    None
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
