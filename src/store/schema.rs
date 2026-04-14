use anyhow::Result;
use rusqlite::Connection;

pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS ingest_checkpoints (
            repo_root TEXT NOT NULL,
            transcript_path TEXT NOT NULL,
            last_line_no INTEGER NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (repo_root, transcript_path)
        );

        CREATE TABLE IF NOT EXISTS raw_events (
            event_id TEXT PRIMARY KEY,
            repo_root TEXT NOT NULL,
            transcript_path TEXT NOT NULL,
            source_line_no INTEGER NOT NULL,
            session_id TEXT NOT NULL,
            occurred_at TEXT NOT NULL,
            kind TEXT NOT NULL,
            payload_json TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_raw_events_repo_path_line
            ON raw_events(repo_root, transcript_path, source_line_no);

        CREATE TABLE IF NOT EXISTS nodes (
            id TEXT PRIMARY KEY,
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

        CREATE INDEX IF NOT EXISTS idx_nodes_repo_kind_state
            ON nodes(repo_root, kind, state_value);

        CREATE TABLE IF NOT EXISTS relations (
            repo_root TEXT NOT NULL,
            source_id TEXT NOT NULL,
            target_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            source_event_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (repo_root, source_id, target_id, kind),
            FOREIGN KEY (source_id) REFERENCES nodes(id) ON DELETE CASCADE,
            FOREIGN KEY (target_id) REFERENCES nodes(id) ON DELETE CASCADE
        );
        ",
    )?;

    Ok(())
}
