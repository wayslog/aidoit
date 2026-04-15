use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde_json::to_string;

use crate::domain::{ExecutionUnit, ExecutionUnitKind, RawEventKind, RawEventPayload, StoredEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestCheckpoint {
    pub project_id: String,
    pub unit_id: String,
    pub project_root: String,
    pub unit_root: String,
    pub unit_kind: ExecutionUnitKind,
    pub branch_ref: Option<String>,
    pub head_oid: Option<String>,
    pub is_active: bool,
    pub repo_root: String,
    pub transcript_path: String,
    pub last_line_no: u64,
    pub updated_at: String,
}

impl IngestCheckpoint {
    pub fn for_execution_unit(
        execution_unit: &ExecutionUnit,
        transcript_path: impl Into<String>,
        last_line_no: u64,
        updated_at: impl Into<String>,
    ) -> Self {
        Self {
            project_id: execution_unit.project_id.clone(),
            unit_id: execution_unit.unit_id.clone(),
            project_root: execution_unit.project_root.clone(),
            unit_root: execution_unit.unit_root.clone(),
            unit_kind: execution_unit.unit_kind,
            branch_ref: execution_unit.branch_ref.clone(),
            head_oid: execution_unit.head_oid.clone(),
            is_active: execution_unit.is_active,
            repo_root: execution_unit.project_root.clone(),
            transcript_path: transcript_path.into(),
            last_line_no,
            updated_at: updated_at.into(),
        }
    }

    pub fn new(
        repo_root: impl Into<String>,
        transcript_path: impl Into<String>,
        last_line_no: u64,
        updated_at: impl Into<String>,
    ) -> Self {
        let execution_unit = ExecutionUnit::legacy_main(repo_root.into());
        Self::for_execution_unit(&execution_unit, transcript_path, last_line_no, updated_at)
    }
}

pub fn upsert_execution_unit_from_event(tx: &Transaction<'_>, event: &StoredEvent) -> Result<()> {
    upsert_execution_unit(
        tx,
        &ExecutionUnit {
            project_id: event.project_id.clone(),
            unit_id: event.unit_id.clone(),
            project_root: event.project_root.clone(),
            unit_root: event.unit_root.clone(),
            unit_kind: event.unit_kind,
            branch_ref: event.branch_ref.clone(),
            head_oid: event.head_oid.clone(),
            is_active: false,
        },
        &event.occurred_at,
    )
}

pub fn upsert_execution_unit_from_checkpoint(
    tx: &Transaction<'_>,
    checkpoint: &IngestCheckpoint,
) -> Result<()> {
    upsert_execution_unit(
        tx,
        &ExecutionUnit {
            project_id: checkpoint.project_id.clone(),
            unit_id: checkpoint.unit_id.clone(),
            project_root: checkpoint.project_root.clone(),
            unit_root: checkpoint.unit_root.clone(),
            unit_kind: checkpoint.unit_kind,
            branch_ref: checkpoint.branch_ref.clone(),
            head_oid: checkpoint.head_oid.clone(),
            is_active: checkpoint.is_active,
        },
        &checkpoint.updated_at,
    )
}

pub fn upsert_project_execution_units(
    tx: &Transaction<'_>,
    project_id: &str,
    units: &[ExecutionUnit],
    updated_at: &str,
) -> Result<()> {
    tx.execute(
        "UPDATE execution_units SET is_active = 0 WHERE project_id = ?1",
        [project_id],
    )?;
    for unit in units {
        upsert_execution_unit(tx, unit, updated_at)?;
    }
    Ok(())
}

fn upsert_execution_unit(
    tx: &Transaction<'_>,
    execution_unit: &ExecutionUnit,
    updated_at: &str,
) -> Result<()> {
    if execution_unit.is_active {
        tx.execute(
            "
            UPDATE execution_units
            SET is_active = 0
            WHERE project_id = ?1
              AND unit_id <> ?2
            ",
            (&execution_unit.project_id, &execution_unit.unit_id),
        )?;
    }

    tx.execute(
        "
        INSERT INTO execution_units (
            unit_id,
            project_id,
            project_root,
            unit_root,
            unit_kind,
            branch_ref,
            head_oid,
            is_active,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(unit_id) DO UPDATE SET
            project_id = excluded.project_id,
            project_root = excluded.project_root,
            unit_root = excluded.unit_root,
            unit_kind = excluded.unit_kind,
            branch_ref = excluded.branch_ref,
            head_oid = excluded.head_oid,
            is_active = excluded.is_active,
            updated_at = excluded.updated_at
        ",
        (
            &execution_unit.unit_id,
            &execution_unit.project_id,
            &execution_unit.project_root,
            &execution_unit.unit_root,
            serde_json::to_string(&execution_unit.unit_kind)?,
            &execution_unit.branch_ref,
            &execution_unit.head_oid,
            if execution_unit.is_active {
                1_i64
            } else {
                0_i64
            },
            updated_at,
        ),
    )?;

    Ok(())
}

pub fn insert_raw_event(tx: &Transaction<'_>, event: &StoredEvent) -> Result<bool> {
    let payload_json = to_string(&event.payload)?;
    let inserted = tx.execute(
        "
        INSERT OR IGNORE INTO raw_events (
            event_id,
            project_id,
            unit_id,
            project_root,
            unit_root,
            unit_kind,
            branch_ref,
            head_oid,
            repo_root,
            transcript_path,
            source_line_no,
            session_id,
            occurred_at,
            kind,
            payload_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ",
        (
            &event.event_id,
            &event.project_id,
            &event.unit_id,
            &event.project_root,
            &event.unit_root,
            serde_json::to_string(&event.unit_kind)?,
            &event.branch_ref,
            &event.head_oid,
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
            project_id,
            unit_id,
            project_root,
            unit_root,
            unit_kind,
            branch_ref,
            head_oid,
            transcript_path,
            last_line_no,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ON CONFLICT(project_id, unit_id, transcript_path) DO UPDATE SET
            project_root = excluded.project_root,
            unit_root = excluded.unit_root,
            unit_kind = excluded.unit_kind,
            branch_ref = excluded.branch_ref,
            head_oid = excluded.head_oid,
            last_line_no = excluded.last_line_no,
            updated_at = excluded.updated_at
        ",
        (
            &checkpoint.project_id,
            &checkpoint.unit_id,
            &checkpoint.project_root,
            &checkpoint.unit_root,
            serde_json::to_string(&checkpoint.unit_kind)?,
            &checkpoint.branch_ref,
            &checkpoint.head_oid,
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
    let execution_unit = ExecutionUnit::legacy_main(repo_root.to_string());
    load_checkpoint_for_unit(
        conn,
        &execution_unit.project_id,
        &execution_unit.unit_id,
        transcript_path,
    )
}

pub fn load_checkpoint_for_unit(
    conn: &Connection,
    project_id: &str,
    unit_id: &str,
    transcript_path: &str,
) -> Result<Option<IngestCheckpoint>> {
    let checkpoint = conn
        .query_row(
            "
            SELECT
                project_id,
                unit_id,
                project_root,
                unit_root,
                unit_kind,
                branch_ref,
                head_oid,
                transcript_path,
                last_line_no,
                updated_at
            FROM ingest_checkpoints
            WHERE project_id = ?1 AND unit_id = ?2 AND transcript_path = ?3
            ",
            (project_id, unit_id, transcript_path),
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        )
        .optional()?;

    let checkpoint = checkpoint
        .map(
            |(
                project_id,
                unit_id,
                project_root,
                unit_root,
                unit_kind,
                branch_ref,
                head_oid,
                transcript_path,
                last_line_no,
                updated_at,
            )|
             -> Result<IngestCheckpoint> {
                Ok(IngestCheckpoint {
                    project_id,
                    unit_id,
                    project_root: project_root.clone(),
                    unit_root,
                    unit_kind: serde_json::from_str::<ExecutionUnitKind>(&unit_kind)?,
                    branch_ref,
                    head_oid,
                    is_active: false,
                    repo_root: project_root,
                    transcript_path,
                    last_line_no: last_line_no as u64,
                    updated_at,
                })
            },
        )
        .transpose()?;

    Ok(checkpoint)
}

pub fn count_raw_events(conn: &Connection) -> Result<usize> {
    let count = conn.query_row("SELECT COUNT(*) FROM raw_events", [], |row| {
        row.get::<_, i64>(0)
    })?;
    Ok(count as usize)
}

pub fn list_raw_events(conn: &Connection) -> Result<Vec<StoredEvent>> {
    list_raw_events_with_filter(
        conn,
        "
        SELECT
            event_id,
            project_id,
            unit_id,
            project_root,
            unit_root,
            unit_kind,
            branch_ref,
            head_oid,
            repo_root,
            transcript_path,
            source_line_no,
            session_id,
            occurred_at,
            kind,
            payload_json
        FROM raw_events
        ORDER BY occurred_at, source_line_no, event_id
        ",
        [],
    )
}

pub fn list_raw_events_for_project(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<StoredEvent>> {
    list_raw_events_with_filter(
        conn,
        "
        SELECT
            event_id,
            project_id,
            unit_id,
            project_root,
            unit_root,
            unit_kind,
            branch_ref,
            head_oid,
            repo_root,
            transcript_path,
            source_line_no,
            session_id,
            occurred_at,
            kind,
            payload_json
        FROM raw_events
        WHERE project_id = ?1
        ORDER BY occurred_at, source_line_no, event_id
        ",
        [project_id],
    )
}

pub fn list_raw_events_for_unit(
    conn: &Connection,
    project_id: &str,
    unit_id: &str,
) -> Result<Vec<StoredEvent>> {
    list_raw_events_with_filter(
        conn,
        "
        SELECT
            event_id,
            project_id,
            unit_id,
            project_root,
            unit_root,
            unit_kind,
            branch_ref,
            head_oid,
            repo_root,
            transcript_path,
            source_line_no,
            session_id,
            occurred_at,
            kind,
            payload_json
        FROM raw_events
        WHERE project_id = ?1 AND unit_id = ?2
        ORDER BY occurred_at, source_line_no, event_id
        ",
        (project_id, unit_id),
    )
}

fn list_raw_events_with_filter<P>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> Result<Vec<StoredEvent>>
where
    P: rusqlite::Params,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, String>(11)?,
            row.get::<_, String>(12)?,
            row.get::<_, String>(13)?,
            row.get::<_, String>(14)?,
        ))
    })?;

    let mut events = Vec::new();
    for row in rows {
        let (
            event_id,
            project_id,
            unit_id,
            project_root,
            unit_root,
            unit_kind,
            branch_ref,
            head_oid,
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
            project_id,
            unit_id,
            project_root,
            unit_root,
            unit_kind: serde_json::from_str::<ExecutionUnitKind>(&unit_kind)?,
            branch_ref,
            head_oid,
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

pub fn list_execution_units(conn: &Connection, project_id: &str) -> Result<Vec<ExecutionUnit>> {
    let mut stmt = conn.prepare(
        "
        SELECT
            project_id,
            unit_id,
            project_root,
            unit_root,
            unit_kind,
            branch_ref,
            head_oid,
            is_active
        FROM execution_units
        WHERE project_id = ?1
        ORDER BY unit_root
        ",
    )?;
    let rows = stmt.query_map([project_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, i64>(7)?,
        ))
    })?;

    let mut units = Vec::new();
    for row in rows {
        let (
            project_id,
            unit_id,
            project_root,
            unit_root,
            unit_kind,
            branch_ref,
            head_oid,
            is_active,
        ) = row?;
        units.push(ExecutionUnit {
            project_id,
            unit_id,
            project_root,
            unit_root,
            unit_kind: serde_json::from_str::<ExecutionUnitKind>(&unit_kind)?,
            branch_ref,
            head_oid,
            is_active: is_active == 1,
        });
    }

    Ok(units)
}
