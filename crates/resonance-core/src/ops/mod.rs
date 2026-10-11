//! Edit-algebra operations (§9, MVP-010).
//!
//! Every authored mutation is a batch of operations. [`Tx`] applies each operation to the
//! store and appends it to the operation log in the same transaction, so the log and the
//! materialized state never disagree. Applying an operation is a pure function of the
//! operation and the current state, which keeps a later deterministic fold (FR-150) possible.

pub mod envelope;
pub mod hlc;

use serde_json::{Value, json};

pub use envelope::{Actor, ActorKind, Op, kind};
pub use hlc::Hlc;

use crate::error::{Error, Result};
use crate::model::{Edge, EdgeType, Layer, LinkStatus, Node, NodeType, Props, ids};
use crate::store::{GraphRead, GraphWrite, SCHEMA_VERSION, meta};

/// A write transaction that records every change as an operation.
pub struct Tx<'a> {
    w: &'a mut dyn GraphWrite,
    actor: Actor,
    change_set: String,
    clock: Option<Hlc>,
    applied: usize,
}

impl<'a> Tx<'a> {
    pub fn new(w: &'a mut dyn GraphWrite, actor: Actor) -> Result<Self> {
        let clock = w.get_meta(meta::HLC)?.as_deref().and_then(Hlc::parse);
        Ok(Tx {
            w,
            actor,
            change_set: ids::new_id(),
            clock,
            applied: 0,
        })
    }

    /// Reads, which see this transaction's own writes.
    pub fn r(&self) -> &dyn GraphRead {
        &*self.w
    }

    /// Direct access for derived data, which is not recorded as operations.
    pub fn w(&mut self) -> &mut dyn GraphWrite {
        &mut *self.w
    }

    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    /// Change the actor for the operations that follow (for example, automatic reconcile
    /// changes inside a command run by a person).
    pub fn set_actor(&mut self, actor: Actor) {
        self.actor = actor;
    }

    /// Number of operations applied so far.
    pub fn applied(&self) -> usize {
        self.applied
    }

    /// Apply one operation and append it to the log.
    pub fn apply(&mut self, kind: &str, payload: Value) -> Result<()> {
        let hlc = Hlc::next(self.clock, hlc::now_ms());
        self.clock = Some(hlc);
        let op = Op {
            op_id: ids::new_id(),
            kind: kind.to_string(),
            actor: self.actor.clone(),
            hlc: hlc.to_string(),
            change_set: self.change_set.clone(),
            schema: SCHEMA_VERSION,
            payload,
        };
        apply(self.w, &op)?;
        self.w.append_op(&op)?;
        self.applied += 1;
        Ok(())
    }

    /// Persist the clock. Call once, at the end of the transaction.
    pub fn finish(self) -> Result<usize> {
        if let Some(clock) = self.clock {
            self.w.set_meta(meta::HLC, &clock.to_string())?;
        }
        Ok(self.applied)
    }

    // Convenience constructors for the common operations.

    pub fn create_node(
        &mut self,
        id: &str,
        ntype: NodeType,
        layer: Layer,
        props: Props,
    ) -> Result<()> {
        self.apply(
            kind::NODE_CREATE,
            json!({"id": id, "type": ntype, "layer": layer, "props": props}),
        )
    }

    pub fn set_prop(&mut self, id: &str, key: &str, value: Value) -> Result<()> {
        self.apply(
            kind::NODE_SET,
            json!({"id": id, "key": key, "value": value}),
        )
    }

    pub fn unset_prop(&mut self, id: &str, key: &str) -> Result<()> {
        self.apply(kind::NODE_UNSET, json!({"id": id, "key": key}))
    }

    pub fn retire_node(&mut self, id: &str) -> Result<()> {
        self.apply(kind::NODE_RETIRE, json!({"id": id}))
    }

    pub fn add_edge(&mut self, edge: &Edge) -> Result<()> {
        self.apply(
            kind::EDGE_ADD,
            json!({
                "id": edge.id, "type": edge.etype, "src": edge.src, "dst": edge.dst,
                "layer": edge.layer, "status": edge.status, "props": edge.props,
            }),
        )
    }

    pub fn set_edge(&mut self, id: &str, key: &str, value: Value) -> Result<()> {
        self.apply(
            kind::EDGE_SET,
            json!({"id": id, "key": key, "value": value}),
        )
    }

    pub fn remove_edge(&mut self, id: &str) -> Result<()> {
        self.apply(kind::EDGE_REMOVE, json!({"id": id}))
    }
}

fn field<'v>(payload: &'v Value, key: &str) -> Result<&'v Value> {
    payload
        .get(key)
        .ok_or_else(|| Error::Store(format!("operation payload is missing `{key}`")))
}

fn str_field<'v>(payload: &'v Value, key: &str) -> Result<&'v str> {
    field(payload, key)?
        .as_str()
        .ok_or_else(|| Error::Store(format!("operation field `{key}` is not a string")))
}

fn parse<T: serde::de::DeserializeOwned>(payload: &Value, key: &str) -> Result<T> {
    Ok(serde_json::from_value(field(payload, key)?.clone())?)
}

fn node_or_err(w: &dyn GraphWrite, id: &str) -> Result<Node> {
    w.get_node(id)?
        .ok_or_else(|| Error::Store(format!("operation refers to unknown node {id}")))
}

fn edge_or_err(w: &dyn GraphWrite, id: &str) -> Result<Edge> {
    w.get_edge(id)?
        .ok_or_else(|| Error::Store(format!("operation refers to unknown edge {id}")))
}

/// The ID of the `evidences` edge from an Evidence node to the edge it supports.
pub fn evidences_edge_id(evidence: &str, edge: &str) -> String {
    format!("evidences:{evidence}>{edge}")
}

/// Apply one operation to the store's materialized state.
pub fn apply(w: &mut dyn GraphWrite, op: &Op) -> Result<()> {
    let p = &op.payload;
    match op.kind.as_str() {
        kind::NODE_CREATE => {
            let node = Node {
                id: str_field(p, "id")?.to_string(),
                ntype: parse(p, "type")?,
                layer: parse(p, "layer")?,
                retired: false,
                props: parse(p, "props")?,
            };
            w.put_node(&node)
        }
        kind::NODE_SET => {
            let mut node = node_or_err(w, str_field(p, "id")?)?;
            node.props
                .insert(str_field(p, "key")?.to_string(), field(p, "value")?.clone());
            w.put_node(&node)
        }
        kind::NODE_UNSET => {
            let mut node = node_or_err(w, str_field(p, "id")?)?;
            node.props.remove(str_field(p, "key")?);
            w.put_node(&node)
        }
        kind::NODE_RETIRE => {
            let mut node = node_or_err(w, str_field(p, "id")?)?;
            node.retired = true;
            w.put_node(&node)
        }
        kind::EDGE_ADD => {
            let edge = Edge {
                id: str_field(p, "id")?.to_string(),
                etype: parse(p, "type")?,
                src: str_field(p, "src")?.to_string(),
                dst: str_field(p, "dst")?.to_string(),
                layer: parse(p, "layer")?,
                status: parse(p, "status")?,
                removed: false,
                props: parse(p, "props")?,
            };
            w.put_edge(&edge)
        }
        kind::EDGE_REMOVE => {
            let mut edge = edge_or_err(w, str_field(p, "id")?)?;
            edge.removed = true;
            w.put_edge(&edge)
        }
        kind::EDGE_SET => {
            let mut edge = edge_or_err(w, str_field(p, "id")?)?;
            set_edge_field(&mut edge, str_field(p, "key")?, field(p, "value")?)?;
            w.put_edge(&edge)
        }
        kind::IDENTITY_MOVE => {
            // Same node, new location: the payload carries the new anchor (§9.4, FR-033).
            let mut node = node_or_err(w, str_field(p, "id")?)?;
            node.props
                .insert("anchor".into(), field(p, "anchor")?.clone());
            w.put_node(&node)
        }
        kind::IDENTITY_RETARGET => {
            if let Some(edge_id) = p.get("edgeId").and_then(Value::as_str) {
                // Re-attach a link or annotation (FR-037).
                let mut edge = edge_or_err(w, edge_id)?;
                let to = str_field(p, "to")?.to_string();
                match str_field(p, "end")? {
                    "src" => edge.src = to,
                    _ => edge.dst = to,
                }
                if let Some(status) = p.get("status") {
                    edge.status = serde_json::from_value(status.clone())?;
                }
                if let Some(props) = p.get("props").and_then(Value::as_object) {
                    for (k, v) in props {
                        edge.props.insert(k.clone(), v.clone());
                    }
                }
                w.put_edge(&edge)
            } else {
                // Re-anchor a code unit to text it matched approximately (MVP-013 step 4).
                let mut node = node_or_err(w, str_field(p, "id")?)?;
                node.props
                    .insert("anchor".into(), field(p, "anchor")?.clone());
                w.put_node(&node)
            }
        }
        kind::IDENTITY_SPLIT => {
            // One node became several (§9.4). The old node is retired; links are moved by
            // separate edge operations in the same change set.
            let mut node = node_or_err(w, str_field(p, "id")?)?;
            node.retired = true;
            node.props
                .insert("splitInto".into(), field(p, "into")?.clone());
            w.put_node(&node)
        }
        kind::IDENTITY_MERGE => {
            // Several nodes became one (§9.4).
            let into = field(p, "into")?.clone();
            let ids: Vec<String> = parse(p, "ids")?;
            for id in ids {
                let mut node = node_or_err(w, &id)?;
                node.retired = true;
                node.props.insert("mergedInto".into(), into.clone());
                w.put_node(&node)?;
            }
            Ok(())
        }
        kind::EVIDENCE_RECORD => {
            let id = str_field(p, "id")?;
            let node = Node::new(id, NodeType::Evidence, Layer::Authored, parse(p, "props")?);
            w.put_node(&node)?;
            let edges: Vec<String> = parse(p, "edges")?;
            for target in edges {
                let mut props = Props::new();
                props.insert("createdAt".into(), field(p, "props")?["timestamp"].clone());
                w.put_edge(&Edge {
                    id: evidences_edge_id(id, &target),
                    etype: EdgeType::Evidences,
                    src: id.to_string(),
                    dst: target,
                    layer: Layer::Authored,
                    status: LinkStatus::Asserted,
                    removed: false,
                    props,
                })?;
            }
            Ok(())
        }
        kind::FILE_PUT => {
            let path = str_field(p, "path")?;
            let id = ids::file_id(path);
            let mut node = w.get_node(&id)?.unwrap_or_else(|| {
                let mut props = Props::new();
                props.insert("path".into(), json!(path));
                Node::new(&id, NodeType::File, Layer::Authored, props)
            });
            node.retired = false;
            node.props.insert("blob".into(), field(p, "blob")?.clone());
            node.props.insert("size".into(), field(p, "size")?.clone());
            node.props
                .insert("stored".into(), field(p, "stored")?.clone());
            w.put_node(&node)
        }
        kind::FILE_MOVE => {
            // A real file move (FR-036): the old path is retired, the new one exists already
            // through `file.put`; links on the file are re-targeted by `identity.retarget`.
            let id = ids::file_id(str_field(p, "from")?);
            if let Some(mut node) = w.get_node(&id)? {
                node.retired = true;
                node.props.insert("movedTo".into(), field(p, "to")?.clone());
                w.put_node(&node)?;
            }
            Ok(())
        }
        kind::FILE_DELETE => {
            let id = ids::file_id(str_field(p, "path")?);
            if let Some(mut node) = w.get_node(&id)? {
                node.retired = true;
                w.put_node(&node)?;
            }
            Ok(())
        }
        // Unknown kinds from a newer version are preserved in the log but not interpreted (FR-008).
        _ => Ok(()),
    }
}

fn set_edge_field(edge: &mut Edge, key: &str, value: &Value) -> Result<()> {
    match key {
        "status" => edge.status = serde_json::from_value(value.clone())?,
        "src" | "dst" | "type" | "id" | "layer" => {
            return Err(Error::Store(format!(
                "edge.set cannot change `{key}`; use identity.retarget"
            )));
        }
        _ => {
            if value.is_null() {
                edge.props.remove(key);
            } else {
                edge.props.insert(key.to_string(), value.clone());
            }
        }
    }
    Ok(())
}
