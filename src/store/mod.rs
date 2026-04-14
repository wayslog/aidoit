mod raw_events;
mod read_models;
mod schema;

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use crate::domain::{NodeKind, RawEventPayload, StoredEvent};

pub use raw_events::{IngestCheckpoint, load_checkpoint};
pub use read_models::{NodeView, RelationView};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestOutcome {
    pub inserted_raw_events: usize,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        initialize_connection(&conn)?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        initialize_connection(&conn)?;
        Ok(Self { conn })
    }

    pub fn initialize(&self) -> Result<()> {
        schema::initialize(&self.conn)
    }

    pub fn has_table(&self, table_name: &str) -> Result<bool> {
        let exists = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table_name],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(exists == 1)
    }

    pub fn foreign_keys_enabled(&self) -> Result<bool> {
        let enabled = self
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))?;
        Ok(enabled == 1)
    }

    pub fn ingest_batch(
        &mut self,
        events: &[StoredEvent],
        checkpoint: &IngestCheckpoint,
    ) -> Result<IngestOutcome> {
        let tx = self.conn.transaction()?;
        let mut inserted_raw_events = 0;
        let mut pending_projection = Vec::new();

        for event in events {
            if raw_events::insert_raw_event(&tx, event)? {
                inserted_raw_events += 1;
                pending_projection.push(event);
            }
        }

        pending_projection.sort_by(|left, right| {
            left.source_line_no
                .cmp(&right.source_line_no)
                .then(event_priority(left).cmp(&event_priority(right)))
                .then(left.event_id.cmp(&right.event_id))
        });
        for event in pending_projection {
            read_models::apply_event(&tx, event)?;
        }

        raw_events::upsert_checkpoint(&tx, checkpoint)?;
        tx.commit()?;

        Ok(IngestOutcome {
            inserted_raw_events,
        })
    }

    pub fn checkpoint(
        &self,
        repo_root: &str,
        transcript_path: &str,
    ) -> Result<Option<IngestCheckpoint>> {
        raw_events::load_checkpoint(&self.conn, repo_root, transcript_path)
    }

    pub fn raw_event_count(&self) -> Result<usize> {
        raw_events::count_raw_events(&self.conn)
    }

    pub fn node_count(&self) -> Result<usize> {
        read_models::count_nodes(&self.conn)
    }

    pub fn list_raw_events(&self) -> Result<Vec<StoredEvent>> {
        raw_events::list_raw_events(&self.conn)
    }

    pub fn list_nodes(&self) -> Result<Vec<NodeView>> {
        read_models::list_nodes(&self.conn)
    }

    pub fn list_nodes_by_kind(&self, kind: NodeKind) -> Result<Vec<NodeView>> {
        read_models::list_nodes_by_kind(&self.conn, kind)
    }

    pub fn get_node(&self, node_id: &str) -> Result<Option<NodeView>> {
        read_models::get_node(&self.conn, node_id)
    }

    pub fn list_relations(&self) -> Result<Vec<RelationView>> {
        read_models::list_relations(&self.conn)
    }
}

fn event_priority(event: &StoredEvent) -> u8 {
    match event.payload {
        RawEventPayload::NodeCaptured { .. } => 0,
        RawEventPayload::StateChanged { .. } => 1,
        RawEventPayload::RelationCaptured { .. } => 2,
    }
}

fn initialize_connection(conn: &Connection) -> Result<()> {
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    Ok(())
}
