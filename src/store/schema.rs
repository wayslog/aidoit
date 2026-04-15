use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension};

const CURRENT_SCHEMA_VERSION: &str = "1";

pub fn initialize(conn: &Connection) -> Result<()> {
    let has_user_tables = conn.query_row(
        "
        SELECT EXISTS(
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table'
              AND name NOT LIKE 'sqlite_%'
        )
        ",
        [],
        |row| row.get::<_, i64>(0),
    )? == 1;

    if !has_user_tables {
        create_schema(conn)?;
        return Ok(());
    }

    let has_metadata_table = conn.query_row(
        "
        SELECT EXISTS(
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table' AND name = 'schema_metadata'
        )
        ",
        [],
        |row| row.get::<_, i64>(0),
    )? == 1;

    if !has_metadata_table {
        bail!("数据库缺少 schema version，请删除现有数据库后重建");
    }

    let version = conn
        .query_row(
            "SELECT value FROM schema_metadata WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    match version.as_deref() {
        Some(CURRENT_SCHEMA_VERSION) => {
            create_schema_objects(conn)?;
            Ok(())
        }
        Some(_) | None => bail!("数据库 schema version 过旧或缺失，请删除现有数据库后重建"),
    }
}

fn create_schema(conn: &Connection) -> Result<()> {
    create_schema_objects(conn)?;
    conn.execute(
        "
        INSERT INTO schema_metadata (key, value)
        VALUES ('schema_version', ?1)
        ",
        [CURRENT_SCHEMA_VERSION],
    )?;
    Ok(())
}

fn create_schema_objects(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS execution_units (
            unit_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            project_root TEXT NOT NULL,
            unit_root TEXT NOT NULL,
            unit_kind TEXT NOT NULL,
            branch_ref TEXT,
            head_oid TEXT,
            is_active INTEGER NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_execution_units_project
            ON execution_units(project_id, unit_root);

        CREATE TABLE IF NOT EXISTS ingest_checkpoints (
            project_id TEXT NOT NULL,
            unit_id TEXT NOT NULL,
            project_root TEXT NOT NULL,
            unit_root TEXT NOT NULL,
            unit_kind TEXT NOT NULL,
            branch_ref TEXT,
            head_oid TEXT,
            transcript_path TEXT NOT NULL,
            last_line_no INTEGER NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (project_id, unit_id, transcript_path)
        );

        CREATE TABLE IF NOT EXISTS raw_events (
            event_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            unit_id TEXT NOT NULL,
            project_root TEXT NOT NULL,
            unit_root TEXT NOT NULL,
            unit_kind TEXT NOT NULL,
            branch_ref TEXT,
            head_oid TEXT,
            repo_root TEXT NOT NULL,
            transcript_path TEXT NOT NULL,
            source_line_no INTEGER NOT NULL,
            session_id TEXT NOT NULL,
            occurred_at TEXT NOT NULL,
            kind TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_raw_events_project_unit_path_line
            ON raw_events(project_id, unit_id, transcript_path, source_line_no);

        CREATE TABLE IF NOT EXISTS nodes (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            scope TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            unit_id TEXT,
            repo_root TEXT NOT NULL,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            state_group TEXT NOT NULL,
            state_value TEXT NOT NULL,
            summary TEXT,
            source_event_id TEXT NOT NULL,
            last_event_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_nodes_project_scope_kind_state
            ON nodes(project_id, scope, scope_id, kind, state_value);

        CREATE TABLE IF NOT EXISTS relations (
            project_id TEXT NOT NULL,
            scope TEXT NOT NULL,
            scope_id TEXT NOT NULL,
            unit_id TEXT,
            repo_root TEXT NOT NULL,
            source_id TEXT NOT NULL,
            target_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            source_event_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (project_id, scope, scope_id, source_id, target_id, kind),
            FOREIGN KEY (source_id) REFERENCES nodes(id) ON DELETE CASCADE,
            FOREIGN KEY (target_id) REFERENCES nodes(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_relations_project_scope
            ON relations(project_id, scope, scope_id, kind);
        ",
    )?;

    Ok(())
}
