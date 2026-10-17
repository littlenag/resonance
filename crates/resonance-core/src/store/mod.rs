//! The store API: engine-neutral persistence (§8.2.1, FR-160–FR-168).
//!
//! Everything Resonance persists goes through [`GraphStore`]. The trait speaks only of
//! nodes, edges, properties, blobs, file records, metadata and operations; no SQL, engine
//! types, row IDs or table names appear here (FR-161). Higher-level operations (`trace`,
//! `why`, context packs, the repair list) are written against it in this crate (FR-163).

#[cfg(feature = "conformance")]
pub mod conformance;
pub mod traverse;

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;
use crate::model::{Edge, EdgeType, Layer, LinkStatus, Node, NodeType};
use crate::ops::Op;

pub use traverse::{Direction, Hop, Traversal};

/// The store schema version this build reads and writes (FR-006, FR-258).
pub const SCHEMA_VERSION: u32 = 1;

/// Metadata keys every store carries (FR-162).
pub mod meta {
    pub const SCHEMA_VERSION: &str = "schema_version";
    pub const STAGE: &str = "stage";
    pub const PARTICIPATION: &str = "participation";
    pub const LAST_RECONCILE: &str = "last_reconcile";
    pub const HLC: &str = "hlc";
    /// The commit the graph was last brought up to date with (FR-068).
    pub const RECONCILED_COMMIT: &str = "git.reconciledCommit";
}

/// Which graph a write handle may modify (MVP-018). Only the primary graph exists in the MVP;
/// parallel graphs (FR-116) will get handles that can write only their own graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphScope {
    Primary,
}

impl GraphScope {
    pub fn name(self) -> &'static str {
        match self {
            GraphScope::Primary => "primary",
        }
    }
}

/// What the store knows about one file on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    /// Repository-relative path with `/` separators.
    pub path: String,
    /// Content hash (MVP-008).
    pub blob: String,
    pub size: u64,
    /// Modification time when last examined, in nanoseconds since the epoch.
    pub mtime_ns: i64,
    /// When the record was written, in nanoseconds since the epoch.
    pub recorded_ns: i64,
}

/// A filter over nodes.
#[derive(Debug, Clone, Default)]
pub struct NodeQuery {
    pub ntype: Option<NodeType>,
    /// Only nodes whose top-level property `key` equals `value`.
    pub prop_eq: Option<(String, Value)>,
    pub include_retired: bool,
}

impl NodeQuery {
    pub fn of_type(ntype: NodeType) -> Self {
        NodeQuery {
            ntype: Some(ntype),
            ..Default::default()
        }
    }
    pub fn with_prop(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.prop_eq = Some((key.to_string(), value.into()));
        self
    }
    pub fn including_retired(mut self) -> Self {
        self.include_retired = true;
        self
    }
}

/// A filter over edges.
#[derive(Debug, Clone, Default)]
pub struct EdgeQuery {
    pub src: Option<String>,
    pub dst: Option<String>,
    pub etype: Option<EdgeType>,
    pub status: Option<LinkStatus>,
    pub layer: Option<Layer>,
    pub include_removed: bool,
}

impl EdgeQuery {
    pub fn from(src: &str) -> Self {
        EdgeQuery {
            src: Some(src.to_string()),
            ..Default::default()
        }
    }
    pub fn to(dst: &str) -> Self {
        EdgeQuery {
            dst: Some(dst.to_string()),
            ..Default::default()
        }
    }
    pub fn all() -> Self {
        EdgeQuery::default()
    }
    pub fn of_type(mut self, etype: EdgeType) -> Self {
        self.etype = Some(etype);
        self
    }
    pub fn with_status(mut self, status: LinkStatus) -> Self {
        self.status = Some(status);
        self
    }
    pub fn in_layer(mut self, layer: Layer) -> Self {
        self.layer = Some(layer);
        self
    }
    pub fn including_removed(mut self) -> Self {
        self.include_removed = true;
        self
    }
}

/// The header of a snapshot (FR-074, FR-165, MVP-027).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotMeta {
    /// The adapter's format identifier, for example `resonance-snapshot-sqlite`.
    pub format: String,
    pub format_version: u32,
    pub schema_version: u32,
    /// The SHA-256 of the sorted IDs of the folded deltas.
    pub epoch: String,
    /// The IDs of every delta folded into this snapshot, sorted.
    pub folded_deltas: Vec<String>,
}

/// A snapshot: the operations it folded, in fold order (FR-157), and its header. The authored
/// and identity data are the fold of these operations (FR-150).
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub meta: SnapshotMeta,
    pub ops: Vec<Op>,
}

/// A recorded operation and its position in the log.
#[derive(Debug, Clone, PartialEq)]
pub struct OpRecord {
    pub seq: u64,
    pub op: Op,
}

/// Reads. Available on the store and inside a write transaction (where they see the
/// transaction's own writes).
pub trait GraphRead {
    fn get_node(&self, id: &str) -> Result<Option<Node>>;
    fn find_nodes(&self, query: &NodeQuery) -> Result<Vec<Node>>;
    fn get_edge(&self, id: &str) -> Result<Option<Edge>>;
    fn find_edges(&self, query: &EdgeQuery) -> Result<Vec<Edge>>;
    fn get_blob(&self, hash: &str) -> Result<Option<Vec<u8>>>;
    fn has_blob(&self, hash: &str) -> Result<bool>;
    fn get_file(&self, path: &str) -> Result<Option<FileRecord>>;
    fn list_files(&self) -> Result<Vec<FileRecord>>;
    fn get_meta(&self, key: &str) -> Result<Option<String>>;
    /// Metadata entries whose key starts with `prefix`, in key order. Local state such as
    /// delta records and the change log is kept this way.
    fn meta_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>>;
    /// Operations with a sequence number greater than `after`, oldest first, at most `limit`.
    fn ops_after(&self, after: u64, limit: usize) -> Result<Vec<OpRecord>>;
    /// The sequence number of the newest operation, or 0 when the log is empty.
    fn last_op_seq(&self) -> Result<u64>;

    /// Bounded traversal (FR-162). This generic implementation works on any adapter; an
    /// adapter may override it with an optimized one (FR-163).
    fn traverse(&self, t: &Traversal) -> Result<Vec<Hop>> {
        traverse::breadth_first(self, t)
    }
}

/// Writes, through a handle scoped to one graph (MVP-018). Every write handle exists only
/// inside [`GraphStore::write`], so everything done through it is one transaction (FR-022).
pub trait GraphWrite: GraphRead {
    fn scope(&self) -> GraphScope;
    /// Insert or replace a node.
    fn put_node(&mut self, node: &Node) -> Result<()>;
    /// Insert or replace an edge.
    fn put_edge(&mut self, edge: &Edge) -> Result<()>;
    /// Delete an edge outright. Used only for derived edges; authored edges are removed by
    /// marking them (`edge.remove`).
    fn delete_edge(&mut self, id: &str) -> Result<()>;
    /// Delete every node and edge in a layer (FR-023: derived data can be dropped).
    fn clear_layer(&mut self, layer: Layer) -> Result<()>;
    /// Store a content-addressed blob. `bytes` is `None` when only the hash and size are kept
    /// (MVP-016). Putting a hash that already has bytes never drops them.
    fn put_blob(&mut self, hash: &str, size: u64, bytes: Option<&[u8]>) -> Result<()>;
    fn put_file(&mut self, record: &FileRecord) -> Result<()>;
    fn delete_file(&mut self, path: &str) -> Result<()>;
    /// Forget every file record, so the next reconcile re-hashes everything.
    fn clear_files(&mut self) -> Result<()>;
    fn set_meta(&mut self, key: &str, value: &str) -> Result<()>;
    fn delete_meta(&mut self, key: &str) -> Result<()>;
    fn append_op(&mut self, op: &Op) -> Result<()>;
    /// Delete every node, edge and operation of this graph, keeping blobs, file records and
    /// metadata. The first step of a re-fold (FR-150).
    fn clear_graph(&mut self) -> Result<()>;
}

/// The store API (FR-160, FR-162).
pub trait GraphStore: GraphRead {
    /// Run `f` in one transaction against the given graph: all of its writes are applied
    /// atomically, or none are if it returns an error (FR-022).
    fn write(
        &mut self,
        scope: GraphScope,
        f: &mut dyn FnMut(&mut dyn GraphWrite) -> Result<()>,
    ) -> Result<()>;

    /// The schema version recorded in the store, or 0 for a new, empty store.
    fn schema_version(&self) -> Result<u32>;

    /// Bring the store's schema up to [`SCHEMA_VERSION`]. Refuses a store whose schema is
    /// newer than this build understands (FR-006, FR-258).
    fn migrate(&mut self) -> Result<u32>;

    /// A simple integrity check; returns a list of problems, empty when healthy (FR-162).
    fn integrity_check(&self) -> Result<Vec<String>>;

    /// Write a snapshot file in this adapter's format (FR-162, FR-165). Writing the same
    /// snapshot twice produces byte-identical files (FR-152). The file is written to a
    /// temporary name and renamed into place.
    fn write_snapshot(&self, path: &Path, snapshot: &Snapshot) -> Result<()>;

    /// Read a snapshot file written by this adapter. Fails on any other file.
    fn read_snapshot(&self, path: &Path) -> Result<Snapshot>;
}
