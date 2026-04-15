use std::{fs, path::PathBuf};

use anyhow::Result;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

use aidoit::domain::{ExecutionUnit, NodeKind, stable_node_id_for_unit};

// Regression: ISSUE-001 — 列表视图不暴露节点 ID，导致 inspect / set-status / confirm / promote 无法从用户可见输出里继续操作
// Found by /qa on 2026-04-14
// Report: .gstack/qa-reports/qa-report-aidoit-cli-2026-04-14.md

const FIXTURE: &str = "tests/fixtures/codex_session.jsonl";
const REPO_ROOT: &str = "/repo/demo";
const SHORT_ID_LEN: usize = 12;

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

fn short_id(id: &str) -> String {
    id[..id.len().min(SHORT_ID_LEN)].to_string()
}

fn legacy_node_id(kind: NodeKind, title: &str) -> String {
    stable_node_id_for_unit(&ExecutionUnit::legacy_main(REPO_ROOT), kind, title)
}

#[test]
fn 列表视图会暴露短_id_且变更命令接受前缀() -> Result<()> {
    let (_temp, transcript, db_path) = prepare_paths()?;
    let main_task_id = legacy_node_id(NodeKind::Task, "完成 transcript 闭环");
    let ready_branch_id = legacy_node_id(NodeKind::Branch, "实现 raw_events");
    let parked_branch_id = legacy_node_id(NodeKind::Branch, "补充 ingest fixture");
    let promoted_task_id = legacy_node_id(NodeKind::Task, "补充 ingest fixture");
    let principle_id = legacy_node_id(NodeKind::Principle, "transcript 是权威源");

    base_command(&transcript, &db_path)?
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("完成 transcript 闭环"))
        .stdout(predicate::str::contains(format!(
            "(id: {})",
            short_id(&main_task_id)
        )))
        .stdout(predicate::str::contains("实现 raw_events"))
        .stdout(predicate::str::contains(format!(
            "(id: {})",
            short_id(&ready_branch_id)
        )));

    base_command(&transcript, &db_path)?
        .arg("tree")
        .assert()
        .success()
        .stdout(predicate::str::contains("补充 ingest fixture"))
        .stdout(predicate::str::contains(format!(
            "(id: {})",
            short_id(&parked_branch_id)
        )));

    let parked_branch_prefix = short_id(&parked_branch_id);
    base_command(&transcript, &db_path)?
        .arg("set-status")
        .arg(&parked_branch_prefix)
        .arg("ready")
        .assert()
        .success()
        .stdout(predicate::str::contains("状态已更新"))
        .stdout(predicate::str::contains("补充 ingest fixture"))
        .stdout(predicate::str::contains(&parked_branch_prefix));

    base_command(&transcript, &db_path)?
        .arg("agenda")
        .assert()
        .success()
        .stdout(predicate::str::contains("补充 ingest fixture"))
        .stdout(predicate::str::contains(format!(
            "(id: {})",
            parked_branch_prefix
        )));

    base_command(&transcript, &db_path)?
        .arg("promote")
        .arg(short_id(&parked_branch_id))
        .assert()
        .success()
        .stdout(predicate::str::contains("已提升为任务"))
        .stdout(predicate::str::contains("补充 ingest fixture"));

    base_command(&transcript, &db_path)?
        .arg("inspect")
        .arg(short_id(&promoted_task_id))
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "- id: {promoted_task_id}"
        )))
        .stdout(predicate::str::contains("补充 ingest fixture"));

    let principle_prefix = short_id(&principle_id);
    base_command(&transcript, &db_path)?
        .arg("confirm")
        .arg(&principle_prefix)
        .assert()
        .success()
        .stdout(predicate::str::contains("已确认"))
        .stdout(predicate::str::contains("transcript 是权威源"))
        .stdout(predicate::str::contains(&principle_prefix));

    base_command(&transcript, &db_path)?
        .arg("principles")
        .assert()
        .success()
        .stdout(predicate::str::contains("transcript 是权威源"))
        .stdout(predicate::str::contains(format!(
            "(id: {})",
            principle_prefix
        )));

    Ok(())
}
