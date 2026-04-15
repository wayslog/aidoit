use std::{fs, path::PathBuf};

use anyhow::Result;
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::json;
use tempfile::tempdir;

use aidoit::domain::{
    NodeKind, NodeState, ReviewState, discover_execution_unit, stable_node_id,
    stable_node_id_for_unit,
};
use aidoit::ingest::import_codex_transcript_for_unit;
use aidoit::store::Store;

const FIXTURE: &str = "tests/fixtures/codex_session.jsonl";
const REPO_ROOT: &str = "/repo/demo";

fn prepare_paths() -> Result<(tempfile::TempDir, PathBuf, PathBuf)> {
    let temp = tempdir()?;
    let transcript = temp.path().join("session.jsonl");
    let db_path = temp.path().join("aidoit.db");
    fs::copy(FIXTURE, &transcript)?;
    Ok((temp, transcript, db_path))
}

fn base_command(transcript: &PathBuf, db_path: &PathBuf) -> Result<Command> {
    let mut command = Command::cargo_bin("aidoit")?;
    command
        .arg("--repo-root")
        .arg(REPO_ROOT)
        .arg("--transcript")
        .arg(transcript)
        .arg("--db-path")
        .arg(db_path);
    Ok(command)
}

#[test]
fn status_会先导入再输出主线和候选分支() -> Result<()> {
    let (_temp, transcript, db_path) = prepare_paths()?;

    base_command(&transcript, &db_path)?
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("主线任务"))
        .stdout(predicate::str::contains("完成 transcript 闭环"))
        .stdout(predicate::str::contains("候选分支"))
        .stdout(predicate::str::contains("实现 raw_events"));

    let store = Store::open(&db_path)?;
    assert_eq!(store.raw_event_count()?, 12);

    Ok(())
}

#[test]
fn agenda_只显示_ready_项() -> Result<()> {
    let (_temp, transcript, db_path) = prepare_paths()?;

    base_command(&transcript, &db_path)?
        .arg("agenda")
        .assert()
        .success()
        .stdout(predicate::str::contains("完成 transcript 闭环"))
        .stdout(predicate::str::contains("实现 raw_events"))
        .stdout(predicate::str::contains("实现 CLI 视图").not())
        .stdout(predicate::str::contains("补充 ingest fixture").not());

    Ok(())
}

#[test]
fn principles_只展示已确认项() -> Result<()> {
    let (_temp, transcript, db_path) = prepare_paths()?;
    let principle_id = stable_node_id(REPO_ROOT, NodeKind::Principle, "transcript 是权威源");

    base_command(&transcript, &db_path)?
        .arg("principles")
        .assert()
        .success()
        .stdout(predicate::str::contains("暂无已确认原则或协定"));

    base_command(&transcript, &db_path)?
        .arg("confirm")
        .arg(&principle_id)
        .assert()
        .success()
        .stdout(predicate::str::contains("已确认"));

    base_command(&transcript, &db_path)?
        .arg("principles")
        .assert()
        .success()
        .stdout(predicate::str::contains("transcript 是权威源"))
        .stdout(predicate::str::contains("第一版不做 Web 控制台").not());

    Ok(())
}

#[test]
fn set_status_与_promote_会更新_tree_和_inspect() -> Result<()> {
    let (_temp, transcript, db_path) = prepare_paths()?;
    let branch_id = stable_node_id(REPO_ROOT, NodeKind::Branch, "补充 ingest fixture");
    let task_id = stable_node_id(REPO_ROOT, NodeKind::Task, "补充 ingest fixture");
    let task_short_id = &task_id[..12];

    base_command(&transcript, &db_path)?
        .arg("set-status")
        .arg(&branch_id)
        .arg("ready")
        .assert()
        .success()
        .stdout(predicate::str::contains("状态已更新"));

    base_command(&transcript, &db_path)?
        .arg("agenda")
        .assert()
        .success()
        .stdout(predicate::str::contains("补充 ingest fixture"));

    base_command(&transcript, &db_path)?
        .arg("promote")
        .arg(&branch_id)
        .assert()
        .success()
        .stdout(predicate::str::contains("已提升为任务"))
        .stdout(predicate::str::contains(task_short_id));

    base_command(&transcript, &db_path)?
        .arg("tree")
        .assert()
        .success()
        .stdout(predicate::str::contains("task 补充 ingest fixture"));

    base_command(&transcript, &db_path)?
        .arg("inspect")
        .arg(&task_id)
        .assert()
        .success()
        .stdout(predicate::str::contains("derived_from"))
        .stdout(predicate::str::contains("补充 ingest fixture"));

    Ok(())
}

#[test]
fn 自动发现_transcript_会跳过坏_session_并使用最新有效文件() -> Result<()> {
    let home = tempdir()?;
    let repo_root = home.path().join("demo-repo");
    let session_dir = home.path().join(".codex").join("sessions").join("demo");
    fs::create_dir_all(repo_root.join(".git"))?;
    fs::create_dir_all(&session_dir)?;
    fs::create_dir_all(home.path().join(".local").join("share"))?;

    let root_string = fs::canonicalize(&repo_root)?.to_string_lossy().to_string();
    fs::write(session_dir.join("broken.jsonl"), "not-json\n")?;
    fs::write(
        session_dir.join("stale.jsonl"),
        format!(
            "{{\"timestamp\":\"2026-04-14T04:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"stale\",\"cwd\":\"{root_string}\"}}}}\n\
             {{\"timestamp\":\"2026-04-14T04:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\",\"message\":\"任务：旧任务\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}}}\n"
        ),
    )?;
    fs::write(
        session_dir.join("latest.jsonl"),
        format!(
            "{{\"timestamp\":\"2026-04-14T05:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"latest\",\"cwd\":\"{root_string}\"}}}}\n\
             {{\"timestamp\":\"2026-04-14T05:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\",\"message\":\"任务：新任务\\n分支：新分支\\n状态：branch:新分支 -> ready\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}}}\n"
        ),
    )?;
    fs::write(
        session_dir.join("stale-cwd.jsonl"),
        concat!(
            "{\"timestamp\":\"2026-04-14T06:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"stale-cwd\",\"cwd\":\"/path/does/not/exist/worktree\"}}\n",
            "{\"timestamp\":\"2026-04-14T06:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"任务：不应触发崩溃\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}\n"
        ),
    )?;

    let now = std::time::SystemTime::now();
    let stale_time = filetime::FileTime::from_system_time(now - std::time::Duration::from_secs(60));
    let latest_time = filetime::FileTime::from_system_time(now);
    let stale_cwd_time =
        filetime::FileTime::from_system_time(now + std::time::Duration::from_secs(60));
    filetime::set_file_mtime(session_dir.join("stale.jsonl"), stale_time)?;
    filetime::set_file_mtime(session_dir.join("latest.jsonl"), latest_time)?;
    filetime::set_file_mtime(session_dir.join("stale-cwd.jsonl"), stale_cwd_time)?;

    let mut command = Command::cargo_bin("aidoit")?;
    command
        .current_dir(&repo_root)
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join(".local").join("share"))
        .arg("status");
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("新任务"))
        .stdout(predicate::str::contains("新分支"))
        .stdout(predicate::str::contains("旧任务").not());

    Ok(())
}

#[test]
fn 从子目录启动也会归一到仓库根() -> Result<()> {
    let home = tempdir()?;
    let repo_root = home.path().join("demo-repo");
    let subdir = repo_root.join("src");
    let session_dir = home.path().join(".codex").join("sessions").join("demo");
    fs::create_dir_all(repo_root.join(".git"))?;
    fs::create_dir_all(&subdir)?;
    fs::create_dir_all(&session_dir)?;

    let transcript = session_dir.join("session.jsonl");
    let root_string = fs::canonicalize(&repo_root)?.to_string_lossy().to_string();
    let session_meta = json!({
        "timestamp": "2026-04-14T04:00:00.000Z",
        "type": "session_meta",
        "payload": {
            "id": "session-1",
            "timestamp": "2026-04-14T04:00:00.000Z",
            "cwd": root_string,
        }
    });
    let event_msg = json!({
        "timestamp": "2026-04-14T04:00:01.000Z",
        "type": "event_msg",
        "payload": {
            "type": "user_message",
            "message": "任务：完成 transcript 闭环\n分支：实现 raw_events\n归属：branch:实现 raw_events -> task:完成 transcript 闭环\n状态：branch:实现 raw_events -> ready",
            "images": [],
            "local_images": [],
            "text_elements": []
        }
    });
    fs::write(&transcript, format!("{session_meta}\n{event_msg}\n"))?;

    let mut root_command = Command::cargo_bin("aidoit")?;
    root_command
        .current_dir(&repo_root)
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join(".local").join("share"))
        .arg("status");
    root_command
        .assert()
        .success()
        .stdout(predicate::str::contains("完成 transcript 闭环"))
        .stdout(predicate::str::contains("实现 raw_events"));

    let mut subdir_command = Command::cargo_bin("aidoit")?;
    subdir_command
        .current_dir(&subdir)
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join(".local").join("share"))
        .arg("status");
    subdir_command
        .assert()
        .success()
        .stdout(predicate::str::contains("完成 transcript 闭环"))
        .stdout(predicate::str::contains("实现 raw_events"));

    Ok(())
}

#[test]
fn 自动发现到别的_unit_transcript_时会按_transcript_所属_unit_导入() -> Result<()> {
    let home = tempdir()?;
    let main_root = home.path().join("demo-repo");
    let linked_root = home.path().join("demo-worktree");
    let session_dir = home.path().join(".codex").join("sessions").join("demo");
    let db_path = home.path().join("shared.sqlite3");
    fs::create_dir_all(main_root.join(".git").join("refs").join("heads"))?;
    fs::create_dir_all(main_root.join(".git").join("worktrees").join("feature"))?;
    fs::create_dir_all(&linked_root)?;
    fs::create_dir_all(&session_dir)?;

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

    let linked_root_string = fs::canonicalize(&linked_root)?
        .to_string_lossy()
        .to_string();
    let transcript = session_dir.join("latest-linked.jsonl");
    fs::write(
        &transcript,
        format!(
            "{{\"timestamp\":\"2026-04-14T05:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"latest\",\"cwd\":\"{linked_root_string}\"}}}}\n\
             {{\"timestamp\":\"2026-04-14T05:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\",\"message\":\"任务：linked 任务\\n分支：linked 分支\\n状态：branch:linked 分支 -> ready\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}}}\n"
        ),
    )?;

    let mut command = Command::cargo_bin("aidoit")?;
    command
        .current_dir(&main_root)
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join(".local").join("share"))
        .arg("--db-path")
        .arg(&db_path)
        .arg("status");
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("linked 任务").not())
        .stdout(predicate::str::contains("linked 分支").not());

    let store = Store::open(&db_path)?;
    let main_unit = discover_execution_unit(&main_root)?;
    let linked_unit = discover_execution_unit(&linked_root)?;
    assert!(
        store
            .list_nodes_for_unit(&main_unit.project_id, &main_unit.unit_id)?
            .is_empty()
    );
    assert!(
        store
            .list_nodes_for_unit(&linked_unit.project_id, &linked_unit.unit_id)?
            .iter()
            .any(|node| node.title == "linked 任务")
    );

    Ok(())
}

#[test]
fn inspect_历史不会混入别的_unit_的_project_级原则事件() -> Result<()> {
    let home = tempdir()?;
    let main_root = home.path().join("demo-repo");
    let linked_root = home.path().join("demo-worktree");
    let db_path = home.path().join("shared.sqlite3");
    fs::create_dir_all(main_root.join(".git").join("refs").join("heads"))?;
    fs::create_dir_all(main_root.join(".git").join("worktrees").join("feature"))?;
    fs::create_dir_all(&linked_root)?;

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

    let main_unit = discover_execution_unit(&main_root)?;
    let linked_unit = discover_execution_unit(&linked_root)?;
    let principle_id = stable_node_id_for_unit(&main_unit, NodeKind::Principle, "统一 schema");

    let main_transcript = home.path().join("main.jsonl");
    fs::write(
        &main_transcript,
        format!(
            "{{\"timestamp\":\"2026-04-14T04:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"main\",\"cwd\":\"{}\"}}}}\n\
             {{\"timestamp\":\"2026-04-14T04:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"原则：统一 schema\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n",
            fs::canonicalize(&main_root)?.to_string_lossy()
        ),
    )?;
    let linked_transcript = home.path().join("linked.jsonl");
    fs::write(
        &linked_transcript,
        format!(
            "{{\"timestamp\":\"2026-04-14T04:10:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"linked\",\"cwd\":\"{}\"}}}}\n\
             {{\"timestamp\":\"2026-04-14T04:10:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"原则：统一 schema\\n状态：principle:统一 schema -> confirmed\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n",
            fs::canonicalize(&linked_root)?.to_string_lossy()
        ),
    )?;

    let mut store = Store::open(&db_path)?;
    import_codex_transcript_for_unit(&mut store, &main_unit, &main_transcript)?;
    import_codex_transcript_for_unit(&mut store, &linked_unit, &linked_transcript)?;
    assert_eq!(
        store
            .get_node(&principle_id)?
            .expect("原则节点应存在")
            .state,
        NodeState::Review(ReviewState::Confirmed)
    );

    let mut command = Command::cargo_bin("aidoit")?;
    command
        .current_dir(&main_root)
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join(".local").join("share"))
        .arg("--db-path")
        .arg(&db_path)
        .arg("inspect")
        .arg(&principle_id);
    command
        .assert()
        .success()
        .stdout(predicate::str::contains("- 状态: confirmed"))
        .stdout(predicate::str::contains(
            "node_captured principle 统一 schema",
        ))
        .stdout(predicate::str::contains("state_changed 统一 schema -> confirmed").not());

    Ok(())
}

#[test]
fn git_场景下显式_transcript_若无法解析归属_unit_则直接失败() -> Result<()> {
    let home = tempdir()?;
    let repo_root = home.path().join("demo-repo");
    let db_path = home.path().join("aidoit.db");
    let transcript = home.path().join("broken-session.jsonl");
    fs::create_dir_all(repo_root.join(".git"))?;
    fs::write(
        &transcript,
        concat!(
            "{\"timestamp\":\"2026-04-14T04:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"broken\",\"cwd\":\"/path/does/not/exist/worktree\"}}\n",
            "{\"timestamp\":\"2026-04-14T04:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"任务：不应静默导入\",\"images\":[],\"local_images\":[],\"text_elements\":[]}}\n"
        ),
    )?;

    let mut command = Command::cargo_bin("aidoit")?;
    command
        .current_dir(&repo_root)
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join(".local").join("share"))
        .arg("--transcript")
        .arg(&transcript)
        .arg("--db-path")
        .arg(&db_path)
        .arg("status");
    command
        .assert()
        .failure()
        .stderr(predicate::str::contains("transcript"))
        .stderr(predicate::str::contains("execution unit"));

    Ok(())
}

#[test]
fn units_会列出_main_repo_与_linked_worktree_摘要() -> Result<()> {
    let layout = CliProjectLayout::create()?;
    let db_path = layout.root().join("shared.sqlite3");
    let main_unit = discover_execution_unit(layout.main_root())?;
    let linked_unit = discover_execution_unit(layout.linked_root())?;
    let mut store = Store::open(&db_path)?;

    let main_transcript = layout.root().join("main.jsonl");
    fs::write(
        &main_transcript,
        format!(
            "{{\"timestamp\":\"2026-04-14T06:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"main\",\"cwd\":\"{}\"}}}}\n\
             {{\"timestamp\":\"2026-04-14T06:00:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"任务：推进主线\\n状态：task:推进主线 -> ready\\n分支：补小修\\n状态：branch:补小修 -> ready\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n",
            layout.main_root_string()
        ),
    )?;
    let linked_transcript = layout.root().join("linked.jsonl");
    fs::write(
        &linked_transcript,
        format!(
            "{{\"timestamp\":\"2026-04-14T06:10:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"linked\",\"cwd\":\"{}\"}}}}\n\
             {{\"timestamp\":\"2026-04-14T06:10:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"任务：继续当前 worktree\\n状态：task:继续当前 worktree -> ready\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n",
            layout.linked_root_string()
        ),
    )?;
    import_codex_transcript_for_unit(&mut store, &main_unit, &main_transcript)?;
    import_codex_transcript_for_unit(&mut store, &linked_unit, &linked_transcript)?;

    let output = Command::cargo_bin("aidoit")?
        .current_dir(layout.main_root())
        .env("HOME", layout.root())
        .env("XDG_DATA_HOME", layout.root().join(".local").join("share"))
        .arg("--db-path")
        .arg(&db_path)
        .arg("units")
        .output()?;
    assert!(output.status.success(), "units 执行失败: {output:?}");

    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("执行单元"));
    assert!(stdout.contains("main_repo"));
    assert!(stdout.contains("linked_worktree"));
    assert!(stdout.contains("当前"));
    assert!(stdout.contains("refs/heads/main"));
    assert!(stdout.contains("refs/heads/feature"));
    assert!(stdout.contains("ready=2"));
    assert!(stdout.contains("继续当前 worktree"));
    assert_eq!(stdout.matches("激活").count(), 1);
    let active_line = stdout
        .lines()
        .find(|line| line.contains("激活"))
        .expect("应存在激活标记");
    assert!(active_line.contains("main_repo"));
    assert!(!active_line.contains("linked_worktree"));

    let persisted_units = Store::open(&db_path)?.list_execution_units(&main_unit.project_id)?;
    assert_eq!(
        persisted_units.iter().filter(|unit| unit.is_active).count(),
        1
    );
    assert_eq!(
        persisted_units
            .iter()
            .find(|unit| unit.is_active)
            .map(|unit| unit.unit_id.as_str()),
        Some(main_unit.unit_id.as_str())
    );

    Ok(())
}

#[test]
fn units_在_linked_worktree_失效后只展示当前可枚举的_unit() -> Result<()> {
    let layout = CliProjectLayout::create()?;
    let db_path = layout.root().join("shared.sqlite3");
    let main_unit = discover_execution_unit(layout.main_root())?;
    let linked_unit = discover_execution_unit(layout.linked_root())?;
    let mut store = Store::open(&db_path)?;

    let main_transcript = layout.root().join("main-stale.jsonl");
    fs::write(
        &main_transcript,
        format!(
            "{{\"timestamp\":\"2026-04-14T06:40:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"main-stale\",\"cwd\":\"{}\"}}}}\n\
             {{\"timestamp\":\"2026-04-14T06:40:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"任务：推进主线\\n状态：task:推进主线 -> ready\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n",
            layout.main_root_string()
        ),
    )?;
    let linked_transcript = layout.root().join("linked-stale.jsonl");
    fs::write(
        &linked_transcript,
        format!(
            "{{\"timestamp\":\"2026-04-14T06:50:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"linked-stale\",\"cwd\":\"{}\"}}}}\n\
             {{\"timestamp\":\"2026-04-14T06:50:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"任务：继续当前 worktree\\n状态：task:继续当前 worktree -> ready\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n",
            layout.linked_root_string()
        ),
    )?;
    import_codex_transcript_for_unit(&mut store, &main_unit, &main_transcript)?;
    import_codex_transcript_for_unit(&mut store, &linked_unit, &linked_transcript)?;

    fs::remove_file(layout.linked_root().join(".git"))?;

    let output = Command::cargo_bin("aidoit")?
        .current_dir(layout.main_root())
        .env("HOME", layout.root())
        .env("XDG_DATA_HOME", layout.root().join(".local").join("share"))
        .arg("--db-path")
        .arg(&db_path)
        .arg("units")
        .output()?;
    assert!(output.status.success(), "units 执行失败: {output:?}");

    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("main_repo"));
    assert!(!stdout.contains("linked_worktree"));
    assert!(stdout.contains("ready=1"));
    assert_eq!(stdout.matches("激活").count(), 1);

    Ok(())
}

#[test]
fn global_agenda_不同_lens_会给出不同排序() -> Result<()> {
    let layout = CliProjectLayout::create()?;
    let db_path = layout.root().join("shared.sqlite3");
    let main_unit = discover_execution_unit(layout.main_root())?;
    let linked_unit = discover_execution_unit(layout.linked_root())?;
    let mut store = Store::open(&db_path)?;

    let main_transcript = layout.root().join("main-agenda.jsonl");
    fs::write(
        &main_transcript,
        format!(
            "{{\"timestamp\":\"2026-04-14T06:20:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"main-agenda\",\"cwd\":\"{}\"}}}}\n\
             {{\"timestamp\":\"2026-04-14T06:20:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"任务：推进主线\\n任务：处理线上阻塞\\n归属：task:处理线上阻塞 -> task:推进主线\\n状态：task:推进主线 -> ready\\n状态：task:处理线上阻塞 -> blocked\\n分支：补小修\\n状态：branch:补小修 -> ready\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n",
            layout.main_root_string()
        ),
    )?;
    let linked_transcript = layout.root().join("linked-agenda.jsonl");
    fs::write(
        &linked_transcript,
        format!(
            "{{\"timestamp\":\"2026-04-14T06:30:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"linked-agenda\",\"cwd\":\"{}\"}}}}\n\
             {{\"timestamp\":\"2026-04-14T06:30:01.000Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\",\"message\":\"任务：继续当前 worktree\\n状态：task:继续当前 worktree -> ready\\n分支：跟进联调\\n状态：branch:跟进联调 -> ready\",\"phase\":\"commentary\",\"memory_citation\":null}}}}\n",
            layout.linked_root_string()
        ),
    )?;
    import_codex_transcript_for_unit(&mut store, &main_unit, &main_transcript)?;
    import_codex_transcript_for_unit(&mut store, &linked_unit, &linked_transcript)?;

    let urgent = run_global_agenda(layout.linked_root(), layout.root(), &db_path, "urgent")?;
    let easy = run_global_agenda(layout.linked_root(), layout.root(), &db_path, "easy")?;
    let mainline = run_global_agenda(layout.linked_root(), layout.root(), &db_path, "mainline")?;
    let low_switch =
        run_global_agenda(layout.linked_root(), layout.root(), &db_path, "low-switch")?;

    assert!(urgent.contains("排序依据"));
    assert!(urgent.contains("main_repo"));
    assert!(urgent.contains("summary"));
    assert!(urgent.contains("处理线上阻塞"));
    assert!(urgent.contains(&layout.main_root_string()));
    assert!(easy.contains("branch"));
    assert!(easy.contains(&layout.main_root_string()));
    assert!(mainline.contains("task"));
    assert!(mainline.contains(&layout.main_root_string()));
    assert!(low_switch.contains("linked_worktree"));
    assert!(low_switch.contains(&layout.linked_root_string()));

    let urgent_top = first_recommendation_line(&urgent);
    let easy_top = first_recommendation_line(&easy);
    let mainline_top = first_recommendation_line(&mainline);
    let low_switch_top = first_recommendation_line(&low_switch);

    assert!(urgent_top.contains("summary"));
    assert!(urgent_top.contains("main_repo"));
    assert!(urgent_top.contains(&layout.main_root_string()));
    assert!(easy_top.contains("branch 补小修"));
    assert!(easy_top.contains(&layout.main_root_string()));
    assert!(mainline_top.contains("task 推进主线"));
    assert!(mainline_top.contains(&layout.main_root_string()));
    assert!(low_switch_top.contains("task 继续当前 worktree"));
    assert!(low_switch_top.contains(&layout.linked_root_string()));
    assert_ne!(urgent_top, easy_top);
    assert_ne!(easy_top, mainline_top);
    assert_ne!(mainline_top, low_switch_top);

    Ok(())
}

#[test]
fn global_agenda_非法_lens_会失败() -> Result<()> {
    let layout = CliProjectLayout::create()?;
    let db_path = layout.root().join("shared.sqlite3");

    let mut command = Command::cargo_bin("aidoit")?;
    command
        .current_dir(layout.main_root())
        .env("HOME", layout.root())
        .env("XDG_DATA_HOME", layout.root().join(".local").join("share"))
        .arg("--db-path")
        .arg(&db_path)
        .arg("global")
        .arg("agenda")
        .arg("--lens")
        .arg("fastest");
    command
        .assert()
        .failure()
        .stderr(predicate::str::contains("urgent"))
        .stderr(predicate::str::contains("easy"))
        .stderr(predicate::str::contains("mainline"))
        .stderr(predicate::str::contains("low-switch"));

    Ok(())
}

#[test]
fn units_与_global_agenda_在空项目时给出清晰空态() -> Result<()> {
    let home = tempdir()?;
    let repo_root = home.path().join("demo-repo");
    let db_path = home.path().join("empty.sqlite3");
    fs::create_dir_all(repo_root.join(".git").join("refs").join("heads"))?;
    fs::write(
        repo_root.join(".git").join("HEAD"),
        "ref: refs/heads/main\n",
    )?;
    fs::write(
        repo_root
            .join(".git")
            .join("refs")
            .join("heads")
            .join("main"),
        "1111111111111111111111111111111111111111\n",
    )?;
    fs::create_dir_all(home.path().join(".local").join("share"))?;

    let mut units_command = Command::cargo_bin("aidoit")?;
    units_command
        .current_dir(&repo_root)
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join(".local").join("share"))
        .arg("--db-path")
        .arg(&db_path)
        .arg("units");
    units_command
        .assert()
        .success()
        .stdout(predicate::str::contains("main_repo"))
        .stdout(predicate::str::contains("linked_worktree").not())
        .stdout(predicate::str::contains("ready=0"));

    let mut agenda_command = Command::cargo_bin("aidoit")?;
    agenda_command
        .current_dir(&repo_root)
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", home.path().join(".local").join("share"))
        .arg("--db-path")
        .arg(&db_path)
        .arg("global")
        .arg("agenda")
        .arg("--lens")
        .arg("urgent");
    agenda_command
        .assert()
        .success()
        .stdout(predicate::str::contains("暂无可推荐项"));

    Ok(())
}

fn run_global_agenda(
    current_dir: &std::path::Path,
    home: &std::path::Path,
    db_path: &PathBuf,
    lens: &str,
) -> Result<String> {
    let output = Command::cargo_bin("aidoit")?
        .current_dir(current_dir)
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join(".local").join("share"))
        .arg("--db-path")
        .arg(db_path)
        .arg("global")
        .arg("agenda")
        .arg("--lens")
        .arg(lens)
        .output()?;
    assert!(
        output.status.success(),
        "global agenda 执行失败: {output:?}"
    );
    Ok(String::from_utf8(output.stdout)?)
}

fn first_recommendation_line(output: &str) -> &str {
    output
        .lines()
        .find(|line| line.starts_with("1. "))
        .expect("应至少包含一条推荐")
}

struct CliProjectLayout {
    temp: tempfile::TempDir,
    main_root: PathBuf,
    linked_root: PathBuf,
}

impl CliProjectLayout {
    fn create() -> Result<Self> {
        let temp = tempdir()?;
        let main_root = temp.path().join("demo-repo");
        let linked_root = temp.path().join("demo-worktree");
        fs::create_dir_all(main_root.join(".git").join("refs").join("heads"))?;
        fs::create_dir_all(main_root.join(".git").join("worktrees").join("feature"))?;
        fs::create_dir_all(&linked_root)?;

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
        fs::create_dir_all(temp.path().join(".local").join("share"))?;

        Ok(Self {
            temp,
            main_root,
            linked_root,
        })
    }

    fn root(&self) -> &std::path::Path {
        self.temp.path()
    }

    fn main_root(&self) -> &std::path::Path {
        &self.main_root
    }

    fn linked_root(&self) -> &std::path::Path {
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
