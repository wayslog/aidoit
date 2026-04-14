mod codex;
mod extract;

use std::path::Path;

use anyhow::{Context, Result};

use crate::store::{IngestCheckpoint, Store};

pub use codex::find_latest_transcript_for_repo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub scanned_lines: u64,
    pub message_count: usize,
    pub emitted_events: usize,
    pub inserted_raw_events: usize,
    pub last_line_no: u64,
}

pub fn import_codex_transcript(
    store: &mut Store,
    repo_root: &str,
    transcript_path: impl AsRef<Path>,
) -> Result<ImportReport> {
    let transcript_path = transcript_path.as_ref();
    let checkpoint = store
        .checkpoint(repo_root, transcript_path.to_string_lossy().as_ref())?
        .unwrap_or_else(|| {
            IngestCheckpoint::new(
                repo_root,
                transcript_path.to_string_lossy(),
                0,
                "1970-01-01T00:00:00Z",
            )
        });
    let parsed = codex::read_new_messages(transcript_path, checkpoint.last_line_no)
        .with_context(|| format!("读取 transcript 失败: {}", transcript_path.display()))?;

    let mut events = Vec::new();
    for message in &parsed.messages {
        events.extend(extract::extract_message_events(
            repo_root,
            transcript_path.to_string_lossy().as_ref(),
            &parsed.session_id,
            message.line_no,
            &message.timestamp,
            &message.message,
        )?);
    }

    let next_checkpoint = IngestCheckpoint::new(
        repo_root,
        transcript_path.to_string_lossy(),
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
