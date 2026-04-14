use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde_json::Value;

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
    let home = std::env::var("HOME").context("缺少 HOME 环境变量")?;
    let sessions_root = Path::new(&home).join(".codex").join("sessions");
    if !sessions_root.exists() {
        return Ok(None);
    }
    let normalized_repo_root =
        fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());

    let mut stack = vec![sessions_root];
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
            let file = fs::File::open(&entry_path)?;
            let mut reader = BufReader::new(file);
            let mut first_line = String::new();
            if reader.read_line(&mut first_line)? == 0 {
                continue;
            }
            let value: Value = serde_json::from_str(&first_line)?;
            let cwd = value
                .get("payload")
                .and_then(|payload| payload.get("cwd"))
                .and_then(Value::as_str);
            if !cwd_matches_repo_root(cwd, &normalized_repo_root) {
                continue;
            }
            let modified_at = entry.metadata()?.modified()?;
            candidates.push((modified_at, entry_path));
        }
    }

    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(candidates.pop().map(|(_, path)| path))
}

fn cwd_matches_repo_root(cwd: Option<&str>, repo_root: &Path) -> bool {
    let Some(cwd) = cwd else {
        return false;
    };
    let cwd_path = Path::new(cwd);
    let normalized_cwd = fs::canonicalize(cwd_path).unwrap_or_else(|_| cwd_path.to_path_buf());
    normalized_cwd == repo_root
}
