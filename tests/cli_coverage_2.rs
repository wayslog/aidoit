use std::{fs, path::PathBuf};

use anyhow::Result;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

use aidoit::domain::{ExecutionUnit, NodeKind, stable_node_id_for_unit};

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

fn legacy_node_id(kind: NodeKind, title: &str) -> String {
    stable_node_id_for_unit(&ExecutionUnit::legacy_main(REPO_ROOT), kind, title)
}

#[test]
fn reject_会把原则从_proposed_切到_rejected() -> Result<()> {
    let (_temp, transcript, db_path) = prepare_paths()?;
    let principle_id = legacy_node_id(NodeKind::Principle, "transcript 是权威源");

    base_command(&transcript, &db_path)?
        .arg("reject")
        .arg(&principle_id)
        .assert()
        .success()
        .stdout(predicate::str::contains("已拒绝"))
        .stdout(predicate::str::contains("transcript 是权威源"));

    base_command(&transcript, &db_path)?
        .arg("inspect")
        .arg(&principle_id)
        .assert()
        .success()
        .stdout(predicate::str::contains("- 状态: rejected"))
        .stdout(predicate::str::contains(
            "state_changed transcript 是权威源 -> rejected",
        ));

    base_command(&transcript, &db_path)?
        .arg("principles")
        .assert()
        .success()
        .stdout(predicate::str::contains("暂无已确认原则或协定"));

    Ok(())
}

#[test]
fn cli_会拒绝非法状态迁移() -> Result<()> {
    let (_temp, transcript, db_path) = prepare_paths()?;
    let branch_id = legacy_node_id(NodeKind::Branch, "实现 raw_events");

    base_command(&transcript, &db_path)?
        .arg("set-status")
        .arg(&branch_id)
        .arg("blocked")
        .assert()
        .success()
        .stdout(predicate::str::contains("状态已更新"));

    base_command(&transcript, &db_path)?
        .arg("set-status")
        .arg(&branch_id)
        .arg("done")
        .assert()
        .failure()
        .stderr(predicate::str::contains("非法状态迁移"));

    base_command(&transcript, &db_path)?
        .arg("inspect")
        .arg(&branch_id)
        .assert()
        .success()
        .stdout(predicate::str::contains("- 状态: blocked"))
        .stdout(predicate::str::contains(
            "state_changed 实现 raw_events -> blocked",
        ))
        .stdout(predicate::str::contains("state_changed 实现 raw_events -> done").not());

    Ok(())
}
