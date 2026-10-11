//! `SqliteStore`: the SQLite implementation of `GraphStore` (FR-160, FR-161).
//!
//! This is the only crate that depends on SQLite. Nothing SQLite-specific (SQL text, row
//! IDs, table names, `rusqlite` types or errors) crosses its public interface: callers see
//! only `resonance_core::store`.

mod blobs;
mod graph;
mod ops;
mod schema;

use std::path::Path;
use std::time::Duration;

use resonance_core::error::{Error, Result};
use resonance_core::model::{Edge, Layer, Node};
use resonance_core::ops::Op;
use resonance_core::store::{
    EdgeQuery, FileRecord, GraphRead, GraphScope, GraphStore, GraphWrite, NodeQuery, OpRecord,
};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

/// Translate an engine error into the engine-neutral error type (FR-161).
pub(crate) fn db<T>(r: rusqlite::Result<T>) -> Result<T> {
    r.map_err(|e| match e {
        rusqlite::Error::SqliteFailure(f, _)
            if matches!(
                f.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            Error::Store(
                "the graph is busy (another resonance process is writing); try again".into(),
            )
        }
        other => Error::Store(other.to_string()),
    })
}

/// The SQLite store. Reads see the primary graph (MVP-018).
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Open, creating the database file if needed. Call [`GraphStore::migrate`] before use.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = db(Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        ))?;
        Self::configure(conn, true)
    }

    /// A private in-memory store, for tests.
    pub fn open_in_memory() -> Result<Self> {
        Self::configure(db(Connection::open_in_memory())?, false)
    }

    fn configure(conn: Connection, wal: bool) -> Result<Self> {
        // Concurrent CLI and MCP processes are serialized by SQLite (MVP-002).
        db(conn.busy_timeout(Duration::from_secs(5)))?;
        if wal {
            db(conn.pragma_update(None, "journal_mode", "WAL"))?;
        }
        db(conn.pragma_update(None, "synchronous", "NORMAL"))?;
        Ok(SqliteStore { conn })
    }
}

const PRIMARY: &str = "primary";

impl GraphRead for SqliteStore {
    fn get_node(&self, id: &str) -> Result<Option<Node>> {
        graph::get_node(&self.conn, PRIMARY, id)
    }
    fn find_nodes(&self, q: &NodeQuery) -> Result<Vec<Node>> {
        graph::find_nodes(&self.conn, PRIMARY, q)
    }
    fn get_edge(&self, id: &str) -> Result<Option<Edge>> {
        graph::get_edge(&self.conn, PRIMARY, id)
    }
    fn find_edges(&self, q: &EdgeQuery) -> Result<Vec<Edge>> {
        graph::find_edges(&self.conn, PRIMARY, q)
    }
    fn get_blob(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        blobs::get(&self.conn, hash)
    }
    fn has_blob(&self, hash: &str) -> Result<bool> {
        blobs::has(&self.conn, hash)
    }
    fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        graph::get_file(&self.conn, PRIMARY, path)
    }
    fn list_files(&self) -> Result<Vec<FileRecord>> {
        graph::list_files(&self.conn, PRIMARY)
    }
    fn get_meta(&self, key: &str) -> Result<Option<String>> {
        if schema::version(&self.conn)? == 0 {
            return Ok(None);
        }
        graph::get_meta(&self.conn, key)
    }
    fn ops_after(&self, after: u64, limit: usize) -> Result<Vec<OpRecord>> {
        ops::after(&self.conn, PRIMARY, after, limit)
    }
}

/// A write handle: one transaction, scoped to one graph (MVP-018).
struct SqliteWrite<'t> {
    tx: Transaction<'t>,
    scope: GraphScope,
}

impl SqliteWrite<'_> {
    fn graph(&self) -> &'static str {
        self.scope.name()
    }
}

impl GraphRead for SqliteWrite<'_> {
    fn get_node(&self, id: &str) -> Result<Option<Node>> {
        graph::get_node(&self.tx, self.graph(), id)
    }
    fn find_nodes(&self, q: &NodeQuery) -> Result<Vec<Node>> {
        graph::find_nodes(&self.tx, self.graph(), q)
    }
    fn get_edge(&self, id: &str) -> Result<Option<Edge>> {
        graph::get_edge(&self.tx, self.graph(), id)
    }
    fn find_edges(&self, q: &EdgeQuery) -> Result<Vec<Edge>> {
        graph::find_edges(&self.tx, self.graph(), q)
    }
    fn get_blob(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        blobs::get(&self.tx, hash)
    }
    fn has_blob(&self, hash: &str) -> Result<bool> {
        blobs::has(&self.tx, hash)
    }
    fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        graph::get_file(&self.tx, self.graph(), path)
    }
    fn list_files(&self) -> Result<Vec<FileRecord>> {
        graph::list_files(&self.tx, self.graph())
    }
    fn get_meta(&self, key: &str) -> Result<Option<String>> {
        graph::get_meta(&self.tx, key)
    }
    fn ops_after(&self, after: u64, limit: usize) -> Result<Vec<OpRecord>> {
        ops::after(&self.tx, self.graph(), after, limit)
    }
}

impl GraphWrite for SqliteWrite<'_> {
    fn scope(&self) -> GraphScope {
        self.scope
    }
    fn put_node(&mut self, node: &Node) -> Result<()> {
        graph::put_node(&self.tx, self.graph(), node)
    }
    fn put_edge(&mut self, edge: &Edge) -> Result<()> {
        graph::put_edge(&self.tx, self.graph(), edge)
    }
    fn delete_edge(&mut self, id: &str) -> Result<()> {
        graph::delete_edge(&self.tx, self.graph(), id)
    }
    fn clear_layer(&mut self, layer: Layer) -> Result<()> {
        graph::clear_layer(&self.tx, self.graph(), layer.as_str())
    }
    fn put_blob(&mut self, hash: &str, size: u64, bytes: Option<&[u8]>) -> Result<()> {
        blobs::put(&self.tx, hash, size, bytes)
    }
    fn put_file(&mut self, record: &FileRecord) -> Result<()> {
        graph::put_file(&self.tx, self.graph(), record)
    }
    fn delete_file(&mut self, path: &str) -> Result<()> {
        graph::delete_file(&self.tx, self.graph(), path)
    }
    fn clear_files(&mut self) -> Result<()> {
        graph::clear_files(&self.tx, self.graph())
    }
    fn set_meta(&mut self, key: &str, value: &str) -> Result<()> {
        graph::set_meta(&self.tx, key, value)
    }
    fn append_op(&mut self, op: &Op) -> Result<()> {
        ops::append(&self.tx, self.graph(), op)
    }
}

impl GraphStore for SqliteStore {
    fn write(
        &mut self,
        scope: GraphScope,
        f: &mut dyn FnMut(&mut dyn GraphWrite) -> Result<()>,
    ) -> Result<()> {
        if schema::version(&self.conn)? == 0 {
            return Err(Error::Store(
                "the store has not been initialized (migrate first)".into(),
            ));
        }
        // IMMEDIATE takes the write lock up front, so two writers never deadlock on upgrade.
        let tx = db(self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let mut handle = SqliteWrite { tx, scope };
        f(&mut handle)?;
        db(handle.tx.commit())
    }

    fn schema_version(&self) -> Result<u32> {
        schema::version(&self.conn)
    }

    fn migrate(&mut self) -> Result<u32> {
        schema::migrate(&mut self.conn)
    }

    fn integrity_check(&self) -> Result<Vec<String>> {
        graph::integrity_check(&self.conn, PRIMARY)
    }
}
