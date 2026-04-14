use std::{fs, path::PathBuf};

use anyhow::Result;
use assert_cmd::Command;
use predicates::prelude::*;
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
        .stdout(predicate::str::contains("已提升为任务"));

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
