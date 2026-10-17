//! `MemoryStore`: an in-memory implementation of `GraphStore` (FR-167).
//!
//! It exists to keep the store boundary honest: everything in `resonance-core` must work with
//! an engine that shares no code with the SQLite adapter (S21 AC1). It also makes tests fast.
//! Writes are copy-on-write: a transaction works on a copy of the state, which replaces the
//! state only when the transaction succeeds (FR-022). Snapshots are canonical JSON files.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use resonance_core::error::{Error, Result};
use resonance_core::history::canonical;
use resonance_core::model::{Edge, Layer, Node};
use resonance_core::ops::Op;
use resonance_core::store::{
    EdgeQuery, FileRecord, GraphRead, GraphScope, GraphStore, GraphWrite, NodeQuery, OpRecord,
    SCHEMA_VERSION, Snapshot, SnapshotMeta, meta,
};
use serde_json::{Value, json};

/// The format identifier of snapshots written by this adapter (FR-165).
pub const SNAPSHOT_FORMAT: &str = "resonance-snapshot-json";
pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;

/// Everything the store holds. Cloned at the start of each transaction.
#[derive(Debug, Clone, Default)]
struct State {
    nodes: BTreeMap<String, Node>,
    edges: BTreeMap<String, Edge>,
    /// Content-addressed blobs; `None` when only the hash and size are kept (MVP-016).
    blobs: BTreeMap<String, Option<Vec<u8>>>,
    files: BTreeMap<String, FileRecord>,
    meta: BTreeMap<String, String>,
    ops: BTreeMap<u64, Op>,
    op_ids: BTreeMap<String, u64>,
    next_seq: u64,
}

/// The in-memory store.
#[derive(Debug, Default)]
pub struct MemoryStore {
    state: State,
}

impl MemoryStore {
    pub fn new() -> Self {
        MemoryStore::default()
    }
}

fn node_matches(n: &Node, q: &NodeQuery) -> bool {
    if q.ntype.is_some_and(|t| t != n.ntype) {
        return false;
    }
    if !q.include_retired && n.retired {
        return false;
    }
    if let Some((k, v)) = &q.prop_eq {
        return n.props.get(k) == Some(v);
    }
    true
}

fn edge_matches(e: &Edge, q: &EdgeQuery) -> bool {
    q.src.as_ref().is_none_or(|s| *s == e.src)
        && q.dst.as_ref().is_none_or(|d| *d == e.dst)
        && q.etype.is_none_or(|t| t == e.etype)
        && q.status.is_none_or(|s| s == e.status)
        && q.layer.is_none_or(|l| l == e.layer)
        && (q.include_removed || !e.removed)
}

impl GraphRead for State {
    fn get_node(&self, id: &str) -> Result<Option<Node>> {
        Ok(self.nodes.get(id).cloned())
    }
    fn find_nodes(&self, q: &NodeQuery) -> Result<Vec<Node>> {
        Ok(self
            .nodes
            .values()
            .filter(|n| node_matches(n, q))
            .cloned()
            .collect())
    }
    fn get_edge(&self, id: &str) -> Result<Option<Edge>> {
        Ok(self.edges.get(id).cloned())
    }
    fn find_edges(&self, q: &EdgeQuery) -> Result<Vec<Edge>> {
        Ok(self
            .edges
            .values()
            .filter(|e| edge_matches(e, q))
            .cloned()
            .collect())
    }
    fn get_blob(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.blobs.get(hash).cloned().flatten())
    }
    fn has_blob(&self, hash: &str) -> Result<bool> {
        Ok(self.blobs.contains_key(hash))
    }
    fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        Ok(self.files.get(path).cloned())
    }
    fn list_files(&self) -> Result<Vec<FileRecord>> {
        Ok(self.files.values().cloned().collect())
    }
    fn get_meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self.meta.get(key).cloned())
    }
    fn meta_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        Ok(self
            .meta
            .range(prefix.to_string()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }
    fn ops_after(&self, after: u64, limit: usize) -> Result<Vec<OpRecord>> {
        Ok(self
            .ops
            .range(after + 1..)
            .take(limit)
            .map(|(seq, op)| OpRecord {
                seq: *seq,
                op: op.clone(),
            })
            .collect())
    }
    fn last_op_seq(&self) -> Result<u64> {
        Ok(self.next_seq)
    }
}

struct MemoryWrite {
    state: State,
    scope: GraphScope,
}

impl GraphRead for MemoryWrite {
    fn get_node(&self, id: &str) -> Result<Option<Node>> {
        self.state.get_node(id)
    }
    fn find_nodes(&self, q: &NodeQuery) -> Result<Vec<Node>> {
        self.state.find_nodes(q)
    }
    fn get_edge(&self, id: &str) -> Result<Option<Edge>> {
        self.state.get_edge(id)
    }
    fn find_edges(&self, q: &EdgeQuery) -> Result<Vec<Edge>> {
        self.state.find_edges(q)
    }
    fn get_blob(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        self.state.get_blob(hash)
    }
    fn has_blob(&self, hash: &str) -> Result<bool> {
        self.state.has_blob(hash)
    }
    fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        self.state.get_file(path)
    }
    fn list_files(&self) -> Result<Vec<FileRecord>> {
        self.state.list_files()
    }
    fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.state.get_meta(key)
    }
    fn meta_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        self.state.meta_prefix(prefix)
    }
    fn ops_after(&self, after: u64, limit: usize) -> Result<Vec<OpRecord>> {
        self.state.ops_after(after, limit)
    }
    fn last_op_seq(&self) -> Result<u64> {
        self.state.last_op_seq()
    }
}

impl GraphWrite for MemoryWrite {
    fn scope(&self) -> GraphScope {
        self.scope
    }
    fn put_node(&mut self, node: &Node) -> Result<()> {
        self.state.nodes.insert(node.id.clone(), node.clone());
        Ok(())
    }
    fn put_edge(&mut self, edge: &Edge) -> Result<()> {
        self.state.edges.insert(edge.id.clone(), edge.clone());
        Ok(())
    }
    fn delete_edge(&mut self, id: &str) -> Result<()> {
        self.state.edges.remove(id);
        Ok(())
    }
    fn clear_layer(&mut self, layer: Layer) -> Result<()> {
        self.state.edges.retain(|_, e| e.layer != layer);
        self.state.nodes.retain(|_, n| n.layer != layer);
        Ok(())
    }
    fn put_blob(&mut self, hash: &str, _size: u64, bytes: Option<&[u8]>) -> Result<()> {
        let entry = self.state.blobs.entry(hash.to_string()).or_insert(None);
        if entry.is_none() {
            *entry = bytes.map(<[u8]>::to_vec);
        }
        Ok(())
    }
    fn put_file(&mut self, record: &FileRecord) -> Result<()> {
        self.state.files.insert(record.path.clone(), record.clone());
        Ok(())
    }
    fn delete_file(&mut self, path: &str) -> Result<()> {
        self.state.files.remove(path);
        Ok(())
    }
    fn clear_files(&mut self) -> Result<()> {
        self.state.files.clear();
        Ok(())
    }
    fn set_meta(&mut self, key: &str, value: &str) -> Result<()> {
        self.state.meta.insert(key.to_string(), value.to_string());
        Ok(())
    }
    fn delete_meta(&mut self, key: &str) -> Result<()> {
        self.state.meta.remove(key);
        Ok(())
    }
    fn append_op(&mut self, op: &Op) -> Result<()> {
        if self.state.op_ids.contains_key(&op.op_id) {
            return Ok(()); // Idempotent by opId (FR-193).
        }
        self.state.next_seq += 1;
        let seq = self.state.next_seq;
        self.state.ops.insert(seq, op.clone());
        self.state.op_ids.insert(op.op_id.clone(), seq);
        Ok(())
    }
    fn clear_graph(&mut self) -> Result<()> {
        self.state.nodes.clear();
        self.state.edges.clear();
        self.state.ops.clear();
        self.state.op_ids.clear();
        Ok(())
    }
}

impl GraphRead for MemoryStore {
    fn get_node(&self, id: &str) -> Result<Option<Node>> {
        self.state.get_node(id)
    }
    fn find_nodes(&self, q: &NodeQuery) -> Result<Vec<Node>> {
        self.state.find_nodes(q)
    }
    fn get_edge(&self, id: &str) -> Result<Option<Edge>> {
        self.state.get_edge(id)
    }
    fn find_edges(&self, q: &EdgeQuery) -> Result<Vec<Edge>> {
        self.state.find_edges(q)
    }
    fn get_blob(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        self.state.get_blob(hash)
    }
    fn has_blob(&self, hash: &str) -> Result<bool> {
        self.state.has_blob(hash)
    }
    fn get_file(&self, path: &str) -> Result<Option<FileRecord>> {
        self.state.get_file(path)
    }
    fn list_files(&self) -> Result<Vec<FileRecord>> {
        self.state.list_files()
    }
    fn get_meta(&self, key: &str) -> Result<Option<String>> {
        self.state.get_meta(key)
    }
    fn meta_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        self.state.meta_prefix(prefix)
    }
    fn ops_after(&self, after: u64, limit: usize) -> Result<Vec<OpRecord>> {
        self.state.ops_after(after, limit)
    }
    fn last_op_seq(&self) -> Result<u64> {
        self.state.last_op_seq()
    }
}

impl GraphStore for MemoryStore {
    fn write(
        &mut self,
        scope: GraphScope,
        f: &mut dyn FnMut(&mut dyn GraphWrite) -> Result<()>,
    ) -> Result<()> {
        if self.schema_version()? == 0 {
            return Err(Error::Store(
                "the store has not been initialized (migrate first)".into(),
            ));
        }
        let mut handle = MemoryWrite {
            state: self.state.clone(),
            scope,
        };
        f(&mut handle)?;
        self.state = handle.state;
        Ok(())
    }

    fn schema_version(&self) -> Result<u32> {
        match self.state.meta.get(meta::SCHEMA_VERSION) {
            None => Ok(0),
            Some(v) => v
                .parse()
                .map_err(|_| Error::Store(format!("unreadable schema version `{v}`"))),
        }
    }

    fn migrate(&mut self) -> Result<u32> {
        let current = self.schema_version()?;
        if current > SCHEMA_VERSION {
            return Err(Error::NewerSchema {
                found: current,
                supported: SCHEMA_VERSION,
            });
        }
        self.state
            .meta
            .insert(meta::SCHEMA_VERSION.into(), SCHEMA_VERSION.to_string());
        Ok(SCHEMA_VERSION)
    }

    fn integrity_check(&self) -> Result<Vec<String>> {
        let s = &self.state;
        let mut problems = Vec::new();
        for e in s.edges.values().filter(|e| !e.removed) {
            let dst_ok = if e.etype == resonance_core::model::EdgeType::Evidences {
                s.edges.contains_key(&e.dst)
            } else {
                s.nodes.contains_key(&e.dst)
            };
            if !s.nodes.contains_key(&e.src) || !dst_ok {
                problems.push(format!(
                    "{} edge {} refers to something that does not exist",
                    e.etype, e.id
                ));
            }
        }
        Ok(problems)
    }

    fn write_snapshot(&self, path: &Path, snapshot: &Snapshot) -> Result<()> {
        let mut text = canonical::to_string(&json!({
            "format": SNAPSHOT_FORMAT,
            "formatVersion": SNAPSHOT_FORMAT_VERSION,
            "schemaVersion": snapshot.meta.schema_version,
            "epoch": snapshot.meta.epoch,
            "foldedDeltas": snapshot.meta.folded_deltas,
        }))?;
        text.push('\n');
        for op in &snapshot.ops {
            text.push_str(&canonical::to_string(op)?);
            text.push('\n');
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text).map_err(|e| Error::io(format!("writing {}", tmp.display()), e))?;
        fs::rename(&tmp, path).map_err(|e| Error::io(format!("writing {}", path.display()), e))
    }

    fn read_snapshot(&self, path: &Path) -> Result<Snapshot> {
        let bytes =
            fs::read(path).map_err(|e| Error::io(format!("reading {}", path.display()), e))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| Error::Invalid(format!("{} is not a snapshot", path.display())))?;
        let mut lines = text.lines();
        let header: Value = lines
            .next()
            .and_then(|l| serde_json::from_str(l).ok())
            .ok_or_else(|| Error::Invalid(format!("{} is not a snapshot", path.display())))?;
        if header["format"] != SNAPSHOT_FORMAT {
            return Err(Error::Invalid(format!(
                "{} is not a snapshot this adapter reads",
                path.display()
            )));
        }
        let meta = SnapshotMeta {
            format: SNAPSHOT_FORMAT.into(),
            format_version: header["formatVersion"].as_u64().unwrap_or(0) as u32,
            schema_version: header["schemaVersion"].as_u64().unwrap_or(0) as u32,
            epoch: header["epoch"].as_str().unwrap_or_default().to_string(),
            folded_deltas: serde_json::from_value(header["foldedDeltas"].clone())?,
        };
        let mut ops = Vec::new();
        for line in lines.filter(|l| !l.trim().is_empty()) {
            ops.push(serde_json::from_str(line)?);
        }
        Ok(Snapshot { meta, ops })
    }
}
