use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde_json::to_string;

use crate::domain::{RawEventKind, RawEventPayload, StoredEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestCheckpoint {
    pub repo_root: String,
    pub transcript_path: String,
    pub last_line_no: u64,
    pub updated_at: String,
}

impl IngestCheckpoint {
    pub fn new(
        repo_root: impl Into<String>,
        transcript_path: impl Into<String>,
        last_line_no: u64,
        updated_at: impl Into<String>,
    ) -> Self {
        Self {
            repo_root: repo_root.into(),
            transcript_path: transcript_path.into(),
            last_line_no,
            updated_at: updated_at.into(),
        }
    }
}

pub fn insert_raw_event(tx: &Transaction<'_>, event: &StoredEvent) -> Result<bool> {
    let payload_json = to_string(&event.payload)?;
    let inserted = tx.execute(
        "
        INSERT OR IGNORE INTO raw_events (
            event_id,
            repo_root,
            transcript_path,
            source_line_no,
            session_id,
            occurred_at,
            kind,
            payload_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        (
            &event.event_id,
            &event.repo_root,
            &event.transcript_path,
            event.source_line_no as i64,
            &event.session_id,
            &event.occurred_at,
            serde_json::to_string(&event.kind)?,
            payload_json,
        ),
    )?;

    Ok(inserted == 1)
}

pub fn upsert_checkpoint(tx: &Transaction<'_>, checkpoint: &IngestCheckpoint) -> Result<()> {
    tx.execute(
        "
        INSERT INTO ingest_checkpoints (
            repo_root,
            transcript_path,
            last_line_no,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(repo_root, transcript_path) DO UPDATE SET
            last_line_no = excluded.last_line_no,
            updated_at = excluded.updated_at
        ",
        (
            &checkpoint.repo_root,
            &checkpoint.transcript_path,
            checkpoint.last_line_no as i64,
            &checkpoint.updated_at,
        ),
    )?;

    Ok(())
}

pub fn load_checkpoint(
    conn: &Connection,
    repo_root: &str,
    transcript_path: &str,
) -> Result<Option<IngestCheckpoint>> {
    let checkpoint = conn
        .query_row(
            "
            SELECT repo_root, transcript_path, last_line_no, updated_at
            FROM ingest_checkpoints
            WHERE repo_root = ?1 AND transcript_path = ?2
            ",
            (repo_root, transcript_path),
            |row| {
                Ok(IngestCheckpoint {
                    repo_root: row.get(0)?,
                    transcript_path: row.get(1)?,
                    last_line_no: row.get::<_, i64>(2)? as u64,
                    updated_at: row.get(3)?,
                })
            },
        )
        .optional()?;

    Ok(checkpoint)
}

pub fn count_raw_events(conn: &Connection) -> Result<usize> {
    let count = conn.query_row("SELECT COUNT(*) FROM raw_events", [], |row| {
        row.get::<_, i64>(0)
    })?;
    Ok(count as usize)
}

pub fn list_raw_events(conn: &Connection) -> Result<Vec<StoredEvent>> {
    let mut stmt = conn.prepare(
        "
        SELECT event_id, repo_root, transcript_path, source_line_no, session_id, occurred_at, kind, payload_json
        FROM raw_events
        ORDER BY occurred_at, source_line_no, event_id
        ",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
        ))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (
            event_id,
            repo_root,
            transcript_path,
            source_line_no,
            session_id,
            occurred_at,
            kind,
            payload_json,
        ) = row?;
        events.push(StoredEvent {
            event_id,
            repo_root,
            transcript_path,
            source_line_no: source_line_no as u64,
            session_id,
            occurred_at,
            kind: serde_json::from_str::<RawEventKind>(&kind)?,
            payload: serde_json::from_str::<RawEventPayload>(&payload_json)?,
        });
    }

    Ok(events)
}
