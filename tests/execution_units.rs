use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use filetime::{FileTime, set_file_mtime};
use tempfile::tempdir;

use aidoit::{
    domain::{
        ExecutionUnitKind, NodeState, ReviewState, WorkState, discover_execution_unit,
        list_project_execution_units,
    },
    ingest::{find_latest_transcript_for_project_in, import_codex_transcript_for_unit},
    store::{AgendaSubjectKind, GlobalAgendaLens, Store},
};

#[test]
fn linked_worktree_与主仓库共享同一个_project_identity() -> Result<()> {
    let layout = FakeProjectLayout::create()?;

    let main_unit = discover_execution_unit(layout.main_root())?;
    let linked_unit = discover_execution_unit(layout.linked_root().join("nested"))?;

    assert_eq!(main_unit.project_id, linked_unit.project_id);
    assert_eq!(main_unit.project_root, layout.main_root_string());
    assert_eq!(linked_unit.project_root, layout.main_root_string());
    assert_eq!(main_unit.unit_root, layout.main_root_string());
    assert_eq!(linked_unit.unit_root, layout.linked_root_string());
    assert_eq!(main_unit.unit_kind, ExecutionUnitKind::MainRepo);
    assert_eq!(linked_unit.unit_kind, ExecutionUnitKind::LinkedWorktree);

    let units = list_project_execution_units(&linked_unit)?;
    assert_eq!(units.len(), 2);
    assert!(
        units
            .iter()
            .any(|unit| unit.unit_root == layout.main_root_string())
    );
    assert!(
        units
            .iter()
            .any(|unit| unit.unit_root == layout.linked_root_string())
    );

    Ok(())
}

#[test]
fn stale_worktree_会被跳过而不是让_project_unit_枚举失败() -> Result<()> {
    let layout = FakeProjectLayout::create()?;
    fs::remove_file(layout.linked_root().join(".git"))?;

    let current = discover_execution_unit(layout.main_root())?.with_active(true);
    let units = list_project_execution_units(&current)?;

    assert_eq!(units.len(), 1);
    assert_eq!(units[0].unit_kind, ExecutionUnitKind::MainRepo);
    assert!(units[0].is_active);

    Ok(())
}

#[test]
fn transcript_发现会接受同一_project_下任意_execution_unit() -> Result<()> {
    let layout = FakeProjectLayout::create()?;
    let sessions_root = layout.root().join("sessions");
    fs::create_dir_all(&sessions_root)?;

    let main_transcript = sessions_root.join("main.jsonl");
    let linked_transcript = sessions_root.join("linked.jsonl");
    write_transcript(
        &main_transcript,
        &layout.main_root_string(),
        "主仓 transcript",
    )?;
    write_transcript(
        &linked_transcript,
        &layout.linked_root_string(),
        "linked worktree transcript",
    )?;
    set_file_mtime(&main_transcript, FileTime::from_unix_time(10, 0))?;
    set_file_mtime(&linked_transcript, FileTime::from_unix_time(20, 0))?;

    let current_unit = discover_execution_unit(layout.main_root())?;
    let latest = find_latest_transcript_for_project_in(&current_unit, &sessions_root)?;

    assert_eq!(latest, Some(linked_transcript));

    Ok(())
}

#[test]
fn store_按_unit_隔离工作流状态并共享_project_级原则() -> Result<()> {
    let layout = FakeProjectLayout::create()?;
    let main_unit = discover_execution_unit(layout.main_root())?;
    let linked_unit = discover_execution_unit(layout.linked_root())?;

    let temp = tempdir()?;
    let db_path = temp.path().join("shared.sqlite3");
    let mut store = Store::open(&db_path)?;

    let main_transcript = temp.path().join("main.jsonl");
    fs::write(
        &main_transcript,
        format!(
            concat!(
                "{{\"timestamp\":\"2026-04-14T04:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"session-main\",\"cwd\":\"{}\"}}}}\n",
                "{{\"timestamp\":\"2026-04-14T04:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"分支：实现基础层\\n状态：branch:实现基础层 -> ready\\n原则：统一 schema\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n"
            ),
            layout.main_root_string(),
        ),
    )?;
    let linked_transcript = temp.path().join("linked.jsonl");
    fs::write(
        &linked_transcript,
        format!(
            concat!(
                "{{\"timestamp\":\"2026-04-14T04:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"session-linked\",\"cwd\":\"{}\"}}}}\n",
                "{{\"timestamp\":\"2026-04-14T04:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"分支：实现基础层\\n状态：branch:实现基础层 -> ready\\n状态：branch:实现基础层 -> blocked\\n原则：统一 schema\\n状态：principle:统一 schema -> confirmed\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n"
            ),
            layout.linked_root_string(),
        ),
    )?;

    import_codex_transcript_for_unit(&mut store, &main_unit, &main_transcript)?;
    import_codex_transcript_for_unit(&mut store, &linked_unit, &linked_transcript)?;

    let known_units = store.list_execution_units(&main_unit.project_id)?;
    let main_scope = store.load_unit_scope(&main_unit.project_id, &main_unit.unit_id)?;
    let linked_scope = store.load_unit_scope(&linked_unit.project_id, &linked_unit.unit_id)?;
    let main_events = store.list_raw_events_for_unit(&main_unit.project_id, &main_unit.unit_id)?;
    let linked_events =
        store.list_raw_events_for_unit(&linked_unit.project_id, &linked_unit.unit_id)?;

    assert_eq!(known_units.len(), 2);
    assert!(main_scope.nodes.iter().any(|node| {
        node.title == "实现基础层" && node.state == NodeState::Work(WorkState::Ready)
    }));
    assert!(linked_scope.nodes.iter().any(|node| {
        node.title == "实现基础层" && node.state == NodeState::Work(WorkState::Blocked)
    }));
    assert!(main_scope.nodes.iter().any(|node| {
        node.title == "统一 schema" && node.state == NodeState::Review(ReviewState::Confirmed)
    }));
    assert!(linked_scope.nodes.iter().any(|node| {
        node.title == "统一 schema" && node.state == NodeState::Review(ReviewState::Confirmed)
    }));
    assert_eq!(main_scope.raw_events, main_events);
    assert_eq!(linked_scope.raw_events, linked_events);
    assert!(main_scope.relations.is_empty());
    assert!(linked_scope.relations.is_empty());
    assert!(
        main_events
            .iter()
            .all(|event| event.unit_id == main_unit.unit_id)
    );
    assert!(
        linked_events
            .iter()
            .all(|event| event.unit_id == linked_unit.unit_id)
    );
    assert!(main_events.len() < store.list_raw_events()?.len());

    Ok(())
}

#[test]
fn store_提供可复用的_unit_summaries_查询() -> Result<()> {
    let layout = FakeProjectLayout::create()?;
    let main_unit = discover_execution_unit(layout.main_root())?;
    let linked_unit = discover_execution_unit(layout.linked_root())?;
    let linked_current = linked_unit.clone().with_active(true);

    let temp = tempdir()?;
    let db_path = temp.path().join("shared.sqlite3");
    let mut store = Store::open(&db_path)?;

    let main_transcript = temp.path().join("main.jsonl");
    fs::write(
        &main_transcript,
        format!(
            concat!(
                "{{\"timestamp\":\"2026-04-14T06:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"session-main\",\"cwd\":\"{}\"}}}}\n",
                "{{\"timestamp\":\"2026-04-14T06:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"任务：推进主线\\n任务：处理线上阻塞\\n归属：task:处理线上阻塞 -> task:推进主线\\n状态：task:推进主线 -> ready\\n状态：task:处理线上阻塞 -> blocked\\n分支：补小修\\n状态：branch:补小修 -> ready\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n"
            ),
            layout.main_root_string(),
        ),
    )?;
    let linked_transcript = temp.path().join("linked.jsonl");
    fs::write(
        &linked_transcript,
        format!(
            concat!(
                "{{\"timestamp\":\"2026-04-14T06:10:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"session-linked\",\"cwd\":\"{}\"}}}}\n",
                "{{\"timestamp\":\"2026-04-14T06:10:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"任务：继续当前 worktree\\n状态：task:继续当前 worktree -> ready\\n分支：跟进联调\\n状态：branch:跟进联调 -> ready\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n"
            ),
            layout.linked_root_string(),
        ),
    )?;

    import_codex_transcript_for_unit(&mut store, &main_unit, &main_transcript)?;
    import_codex_transcript_for_unit(&mut store, &linked_current, &linked_transcript)?;
    store.sync_project_execution_units(&linked_current)?;

    let persisted_units = store.list_execution_units(&main_unit.project_id)?;
    assert_eq!(
        persisted_units.iter().filter(|unit| unit.is_active).count(),
        1
    );
    assert_eq!(
        persisted_units
            .iter()
            .find(|unit| unit.is_active)
            .map(|unit| unit.unit_id.as_str()),
        Some(linked_unit.unit_id.as_str())
    );

    let summaries = store.load_project_unit_summaries(&linked_current)?;
    assert_eq!(summaries.len(), 2);

    let main_summary = summaries
        .iter()
        .find(|summary| summary.execution_unit.unit_id == main_unit.unit_id)
        .expect("主仓库摘要应存在");
    assert!(!main_summary.is_current);
    assert!(!main_summary.execution_unit.is_active);
    assert_eq!(main_summary.work_counts.ready, 2);
    assert_eq!(main_summary.work_counts.blocked, 1);
    assert_eq!(main_summary.work_counts.done, 0);
    assert_eq!(main_summary.work_counts.parked, 0);
    assert_eq!(
        main_summary
            .main_task
            .as_ref()
            .map(|node| node.title.as_str()),
        Some("推进主线")
    );
    assert_eq!(
        main_summary
            .candidate_branches
            .iter()
            .map(|node| node.title.as_str())
            .collect::<Vec<_>>(),
        vec!["补小修"]
    );
    assert_eq!(
        main_summary
            .blocked_items
            .iter()
            .map(|node| node.title.as_str())
            .collect::<Vec<_>>(),
        vec!["处理线上阻塞"]
    );

    let linked_summary = summaries
        .iter()
        .find(|summary| summary.execution_unit.unit_id == linked_unit.unit_id)
        .expect("linked worktree 摘要应存在");
    assert!(linked_summary.is_current);
    assert!(linked_summary.execution_unit.is_active);
    assert_eq!(linked_summary.work_counts.ready, 2);
    assert_eq!(
        linked_summary
            .main_task
            .as_ref()
            .map(|node| node.title.as_str()),
        Some("继续当前 worktree")
    );

    Ok(())
}

#[test]
fn easy_lens_不会把未_ready_的主线_task_说成_ready() -> Result<()> {
    let layout = FakeProjectLayout::create()?;
    let current = discover_execution_unit(layout.main_root())?.with_active(true);

    let temp = tempdir()?;
    let db_path = temp.path().join("shared.sqlite3");
    let mut store = Store::open(&db_path)?;

    let transcript = temp.path().join("easy-lens.jsonl");
    fs::write(
        &transcript,
        format!(
            concat!(
                "{{\"timestamp\":\"2026-04-14T08:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"easy-lens\",\"cwd\":\"{}\"}}}}\n",
                "{{\"timestamp\":\"2026-04-14T08:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"任务：阻塞中的主线\\n状态：task:阻塞中的主线 -> blocked\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n"
            ),
            layout.main_root_string(),
        ),
    )?;

    import_codex_transcript_for_unit(&mut store, &current, &transcript)?;

    let agenda = store.load_global_agenda(&current, GlobalAgendaLens::Easy, 5)?;
    let top = agenda.items.first().expect("应至少有一条推荐");

    assert_eq!(top.subject_kind, AgendaSubjectKind::Task);
    assert_eq!(top.subject_title, "阻塞中的主线");
    assert!(!top.reason.contains("已 ready"));
    assert!(top.reason.contains("未 ready"));

    Ok(())
}

#[test]
fn list_project_execution_units_会跳过失效项并去重重复项() -> Result<()> {
    let layout = FakeProjectLayout::create()?;
    let current = discover_execution_unit(layout.main_root())?.with_active(true);

    let duplicate_worktree = layout
        .root()
        .join(".git")
        .join("worktrees")
        .join("feature-copy");
    fs::create_dir_all(&duplicate_worktree)?;
    fs::write(
        duplicate_worktree.join("gitdir"),
        format!("gitdir: {}\n", layout.linked_root().join(".git").display()),
    )?;

    let stale_worktree = layout.root().join(".git").join("worktrees").join("stale");
    fs::create_dir_all(&stale_worktree)?;
    fs::write(
        stale_worktree.join("gitdir"),
        format!(
            "gitdir: {}\n",
            layout.root().join("missing-worktree").display()
        ),
    )?;

    let units = list_project_execution_units(&current)?;

    assert_eq!(units.len(), 2);
    assert!(
        units
            .iter()
            .filter(|unit| unit.unit_root == layout.main_root_string())
            .count()
            == 1
    );
    assert!(
        units
            .iter()
            .filter(|unit| unit.unit_root == layout.linked_root_string())
            .count()
            == 1
    );

    Ok(())
}

#[test]
fn sync_project_execution_units_不会删除库里已有但当前不可枚举的_unit() -> Result<()> {
    let layout = FakeProjectLayout::create()?;
    let main_unit = discover_execution_unit(layout.main_root())?.with_active(true);
    let linked_unit = discover_execution_unit(layout.linked_root())?;

    let temp = tempdir()?;
    let db_path = temp.path().join("shared.sqlite3");
    let mut store = Store::open(&db_path)?;

    let linked_transcript = temp.path().join("linked-sync.jsonl");
    fs::write(
        &linked_transcript,
        format!(
            concat!(
                "{{\"timestamp\":\"2026-04-14T07:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"linked-sync\",\"cwd\":\"{}\"}}}}\n",
                "{{\"timestamp\":\"2026-04-14T07:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"任务：历史 worktree 任务\\n状态：task:历史 worktree 任务 -> ready\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n"
            ),
            layout.linked_root_string(),
        ),
    )?;

    import_codex_transcript_for_unit(&mut store, &linked_unit, &linked_transcript)?;
    let before_sync = store.list_execution_units(&main_unit.project_id)?;
    assert_eq!(before_sync.len(), 1);
    assert_eq!(before_sync[0].unit_id, linked_unit.unit_id);

    fs::remove_file(layout.linked_root().join(".git"))?;
    store.sync_project_execution_units(&main_unit)?;

    let after_sync = store.list_execution_units(&main_unit.project_id)?;
    assert_eq!(after_sync.len(), 2);
    assert!(
        after_sync
            .iter()
            .any(|unit| unit.unit_id == linked_unit.unit_id)
    );
    assert!(
        after_sync
            .iter()
            .any(|unit| unit.unit_id == main_unit.unit_id)
    );
    assert_eq!(after_sync.iter().filter(|unit| unit.is_active).count(), 1);
    assert_eq!(
        after_sync
            .iter()
            .find(|unit| unit.is_active)
            .map(|unit| unit.unit_id.as_str()),
        Some(main_unit.unit_id.as_str())
    );

    Ok(())
}

#[test]
fn 缺少_schema_version_的旧库会_fail_fast_并提示删库重建() -> Result<()> {
    let temp = tempdir()?;
    let db_path = temp.path().join("legacy.sqlite3");
    let conn = rusqlite::Connection::open(&db_path)?;
    conn.execute("CREATE TABLE legacy_nodes (id TEXT PRIMARY KEY)", [])?;
    drop(conn);

    let error = match Store::open(&db_path) {
        Ok(_) => panic!("旧库缺少 schema version 时应直接失败"),
        Err(error) => error,
    };
    let message = format!("{error:#}");

    assert!(message.contains("schema version"));
    assert!(message.contains("删除") || message.contains("重建"));

    Ok(())
}

#[test]
fn 过旧_schema_version_的旧库会_fail_fast_并提示删库重建() -> Result<()> {
    let temp = tempdir()?;
    let db_path = temp.path().join("old-version.sqlite3");
    let conn = rusqlite::Connection::open(&db_path)?;
    conn.execute(
        "CREATE TABLE schema_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    )?;
    conn.execute(
        "INSERT INTO schema_metadata (key, value) VALUES ('schema_version', '0')",
        [],
    )?;
    drop(conn);

    let error = match Store::open(&db_path) {
        Ok(_) => panic!("过旧 schema version 时应直接失败"),
        Err(error) => error,
    };
    let message = format!("{error:#}");

    assert!(message.contains("schema version"));
    assert!(message.contains("删除") || message.contains("重建"));

    Ok(())
}

fn write_transcript(path: &Path, cwd: &str, message: &str) -> Result<()> {
    fs::write(
        path,
        format!(
            concat!(
                "{{\"timestamp\":\"2026-04-14T04:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"session-1\",\"cwd\":\"{}\"}}}}\n",
                "{{\"timestamp\":\"2026-04-14T04:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"{}\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n"
            ),
            cwd, message
        ),
    )?;
    Ok(())
}

struct FakeProjectLayout {
    _temp: tempfile::TempDir,
    main_root: PathBuf,
    linked_root: PathBuf,
}

impl FakeProjectLayout {
    fn create() -> Result<Self> {
        let temp = tempdir()?;
        let main_root = temp.path().join("main-repo");
        let linked_root = temp.path().join("feature-worktree");
        fs::create_dir_all(main_root.join(".git").join("refs").join("heads"))?;
        fs::create_dir_all(main_root.join(".git").join("worktrees").join("feature"))?;
        fs::create_dir_all(linked_root.join("nested"))?;

        fs::write(
            main_root.join(".git").join("HEAD"),
            "ref: refs/heads/main\n",
        )?;
        fs::write(
            main_root
                .join(".git")
                .join("refs")
                .join("heads")
                .join("main"),
            "1111111111111111111111111111111111111111\n",
        )?;
        fs::write(
            main_root
                .join(".git")
                .join("refs")
                .join("heads")
                .join("feature"),
            "2222222222222222222222222222222222222222\n",
        )?;

        let worktree_git_dir = main_root.join(".git").join("worktrees").join("feature");
        fs::write(
            linked_root.join(".git"),
            format!("gitdir: {}\n", worktree_git_dir.display()),
        )?;
        fs::write(worktree_git_dir.join("HEAD"), "ref: refs/heads/feature\n")?;
        fs::write(worktree_git_dir.join("commondir"), "../..\n")?;
        fs::write(
            worktree_git_dir.join("gitdir"),
            format!("{}\n", linked_root.join(".git").display()),
        )?;

        Ok(Self {
            _temp: temp,
            main_root,
            linked_root,
        })
    }

    fn root(&self) -> &Path {
        self._temp.path()
    }

    fn main_root(&self) -> &Path {
        &self.main_root
    }

    fn linked_root(&self) -> &Path {
        &self.linked_root
    }

    fn main_root_string(&self) -> String {
        fs::canonicalize(&self.main_root)
            .unwrap_or_else(|_| self.main_root.clone())
            .to_string_lossy()
            .to_string()
    }

    fn linked_root_string(&self) -> String {
        fs::canonicalize(&self.linked_root)
            .unwrap_or_else(|_| self.linked_root.clone())
            .to_string_lossy()
            .to_string()
    }
}
