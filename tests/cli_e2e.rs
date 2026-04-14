use std::{fs, path::PathBuf};

use anyhow::Result;
use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::json;
use tempfile::tempdir;

use aidoit::domain::{NodeKind, stable_node_id};
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

    let now = std::time::SystemTime::now();
    let stale_time = filetime::FileTime::from_system_time(now - std::time::Duration::from_secs(60));
    let latest_time = filetime::FileTime::from_system_time(now);
    filetime::set_file_mtime(session_dir.join("stale.jsonl"), stale_time)?;
    filetime::set_file_mtime(session_dir.join("latest.jsonl"), latest_time)?;

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
