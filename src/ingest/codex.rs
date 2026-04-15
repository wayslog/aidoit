use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde_json::Value;

use crate::domain::{ExecutionUnit, discover_execution_unit, resolve_execution_unit};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptMessage {
    pub line_no: u64,
    pub timestamp: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTranscript {
    pub session_id: String,
    pub messages: Vec<TranscriptMessage>,
    pub total_lines: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredTranscript {
    pub path: PathBuf,
    pub execution_unit: ExecutionUnit,
}

pub fn read_new_messages(path: &Path, last_line_no: u64) -> Result<ParsedTranscript> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut session_id = None;
    let mut messages = Vec::new();
    let mut total_lines = 0_u64;

    for (index, line_result) in reader.lines().enumerate() {
        let line_no = index as u64 + 1;
        total_lines = line_no;
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(&line).with_context(|| format!("第 {line_no} 行不是合法 JSON"))?;

        if value.get("type").and_then(Value::as_str) == Some("session_meta") {
            session_id = value
                .get("payload")
                .and_then(|payload| payload.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            continue;
        }

        if line_no <= last_line_no {
            continue;
        }

        if value.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }

        let payload = value
            .get("payload")
            .and_then(Value::as_object)
            .with_context(|| format!("第 {line_no} 行缺少 payload"))?;
        let payload_type = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(payload_type, "user_message" | "agent_message") {
            continue;
        }
        let message = payload
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if message.is_empty() {
            continue;
        }

        messages.push(TranscriptMessage {
            line_no,
            timestamp: value
                .get("timestamp")
                .and_then(Value::as_str)
                .unwrap_or("1970-01-01T00:00:00Z")
                .to_string(),
            message,
        });
    }

    Ok(ParsedTranscript {
        session_id: session_id.unwrap_or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("unknown-session")
                .to_string()
        }),
        messages,
        total_lines,
    })
}

pub fn find_latest_transcript_for_repo(repo_root: &Path) -> Result<Option<PathBuf>> {
    let home = BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .context("无法确定 home 目录")?;
    let sessions_root = home.join(".codex").join("sessions");
    let execution_unit = resolve_execution_unit(repo_root)?
        .unwrap_or_else(|| ExecutionUnit::legacy_main(repo_root.to_string_lossy().to_string()));
    Ok(
        find_latest_transcript_for_project_with_unit_in(&execution_unit, &sessions_root)?
            .map(|discovered| discovered.path),
    )
}

pub fn find_latest_transcript_for_project_in(
    current_unit: &ExecutionUnit,
    sessions_root: &Path,
) -> Result<Option<PathBuf>> {
    Ok(
        find_latest_transcript_for_project_with_unit_in(current_unit, sessions_root)?
            .map(|discovered| discovered.path),
    )
}

pub fn find_latest_transcript_for_project_with_unit_in(
    current_unit: &ExecutionUnit,
    sessions_root: &Path,
) -> Result<Option<DiscoveredTranscript>> {
    if !sessions_root.exists() {
        return Ok(None);
    }

    let mut stack = vec![sessions_root.to_path_buf()];
    let mut candidates = Vec::new();

    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let entry_path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(entry_path);
                continue;
            }
            if entry_path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(file) = fs::File::open(&entry_path) else {
                continue;
            };
            let mut reader = BufReader::new(file);
            let mut first_line = String::new();
            let Ok(read_bytes) = reader.read_line(&mut first_line) else {
                continue;
            };
            if read_bytes == 0 {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&first_line) else {
                continue;
            };
            let candidate_unit = match transcript_execution_unit_from_value(&value) {
                Ok(Some(candidate_unit)) => candidate_unit,
                Ok(None) | Err(_) => continue,
            };
            if candidate_unit.project_id != current_unit.project_id {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let Ok(modified_at) = metadata.modified() else {
                continue;
            };
            candidates.push((
                modified_at,
                DiscoveredTranscript {
                    path: entry_path,
                    execution_unit: candidate_unit,
                },
            ));
        }
    }

    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(candidates.pop().map(|(_, discovered)| discovered))
}

pub fn resolve_transcript_execution_unit(path: &Path) -> Result<Option<ExecutionUnit>> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut first_line = String::new();
    if reader.read_line(&mut first_line)? == 0 {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(&first_line)?;
    transcript_execution_unit_from_value(&value)
}

fn transcript_execution_unit_from_value(value: &Value) -> Result<Option<ExecutionUnit>> {
    let cwd = value
        .get("payload")
        .and_then(|payload| payload.get("cwd"))
        .and_then(Value::as_str);
    let Some(cwd) = cwd else {
        return Ok(None);
    };
    Ok(Some(discover_execution_unit(cwd)?))
}
