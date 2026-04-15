mod codex;
mod extract;

use std::{fmt::Write, path::Path};

use anyhow::{Context, Result};

use crate::{
    domain::{ExecutionUnit, resolve_execution_unit},
    store::{IngestCheckpoint, Store},
};

pub use codex::{
    DiscoveredTranscript, find_latest_transcript_for_project_in,
    find_latest_transcript_for_project_with_unit_in, find_latest_transcript_for_repo,
    resolve_transcript_execution_unit,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub scanned_lines: u64,
    pub message_count: usize,
    pub emitted_events: usize,
    pub inserted_raw_events: usize,
    pub last_line_no: u64,
}

pub fn import_codex_transcript_for_unit(
    store: &mut Store,
    execution_unit: &ExecutionUnit,
    transcript_path: impl AsRef<Path>,
) -> Result<ImportReport> {
    let transcript_path = transcript_path.as_ref();
    let transcript_key = transcript_path_key(transcript_path);
    let checkpoint = store
        .checkpoint_for_unit(
            &execution_unit.project_id,
            &execution_unit.unit_id,
            &transcript_key,
        )?
        .unwrap_or_else(|| {
            IngestCheckpoint::for_execution_unit(
                execution_unit,
                &transcript_key,
                0,
                "1970-01-01T00:00:00Z",
            )
        });
    let parsed = codex::read_new_messages(transcript_path, checkpoint.last_line_no)
        .with_context(|| format!("读取 transcript 失败: {}", transcript_path.display()))?;

    let mut events = Vec::new();
    for message in &parsed.messages {
        events.extend(extract::extract_message_events(
            execution_unit,
            &transcript_key,
            &parsed.session_id,
            message.line_no,
            &message.timestamp,
            &message.message,
        )?);
    }

    let next_checkpoint = IngestCheckpoint::for_execution_unit(
        execution_unit,
        &transcript_key,
        parsed.total_lines,
        parsed
            .messages
            .last()
            .map(|message| message.timestamp.as_str())
            .unwrap_or(checkpoint.updated_at.as_str()),
    );
    let outcome = store.ingest_batch(&events, &next_checkpoint)?;

    Ok(ImportReport {
        scanned_lines: parsed.total_lines.saturating_sub(checkpoint.last_line_no),
        message_count: parsed.messages.len(),
        emitted_events: events.len(),
        inserted_raw_events: outcome.inserted_raw_events,
        last_line_no: parsed.total_lines,
    })
}

pub fn import_codex_transcript(
    store: &mut Store,
    repo_root: &str,
    transcript_path: impl AsRef<Path>,
) -> Result<ImportReport> {
    let execution_unit = resolve_execution_unit(repo_root)
        .with_context(|| format!("无法解析 execution unit: {repo_root}"))?
        .unwrap_or_else(|| ExecutionUnit::legacy_main(repo_root.to_string()));
    import_codex_transcript_for_unit(store, &execution_unit, transcript_path)
}

#[cfg(unix)]
fn transcript_path_key(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;

    let mut key = String::from("unix:");
    for byte in path.as_os_str().as_bytes() {
        let _ = write!(&mut key, "{byte:02x}");
    }
    key
}

#[cfg(windows)]
fn transcript_path_key(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;

    let mut key = String::from("windows:");
    for unit in path.as_os_str().encode_wide() {
        let _ = write!(&mut key, "{unit:04x}");
    }
    key
}

#[cfg(not(any(unix, windows)))]
fn transcript_path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn 非_utf8_path_key_不会碰撞() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let raw_a = OsString::from_vec(b"session-\x80.jsonl".to_vec());
        let raw_b = OsString::from_vec(b"session-\x81.jsonl".to_vec());
        let path_a = Path::new(&raw_a);
        let path_b = Path::new(&raw_b);

        assert_ne!(transcript_path_key(path_a), transcript_path_key(path_b));
    }
}
