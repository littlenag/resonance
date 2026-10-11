//! Engine-neutral export of authored and identity data as JSON Lines (FR-166, FR-164).
//!
//! The first line is a header. Every following line is one edit-algebra operation (§9)
//! that recreates the current state: `node.create` (and `node.retire`) for each authored
//! node, `edge.add` (and `edge.remove`) for each authored edge. Files are included only when
//! something authored refers to them; their content stays in git. Keys are sorted, so the
//! same graph always exports to the same operations apart from their IDs and timestamps.

use std::collections::BTreeSet;
use std::io::Write;

use serde_json::json;

use crate::error::{Error, Result};
use crate::model::{Layer, NodeType, new_id};
use crate::ops::{Actor, Hlc, Op, hlc, kind};
use crate::store::{EdgeQuery, GraphRead, NodeQuery, SCHEMA_VERSION};

/// The export format's identifier and version.
pub const FORMAT: &str = "resonance-export";
pub const FORMAT_VERSION: u32 = 1;

fn io(e: std::io::Error) -> Error {
    Error::io("writing the export", e)
}

/// Write the export; returns the number of operations written.
pub fn write(r: &dyn GraphRead, repository: &str, out: &mut dyn Write) -> Result<usize> {
    let now = hlc::now_ms();
    let header = json!({
        "format": FORMAT,
        "formatVersion": FORMAT_VERSION,
        "schema": SCHEMA_VERSION,
        "repository": repository,
        "exportedAt": hlc::rfc3339(now),
        "resonance": crate::api::VERSION,
    });
    writeln!(out, "{}", serde_json::to_string(&header)?).map_err(io)?;

    let change_set = new_id();
    let actor = Actor {
        kind: crate::ops::ActorKind::Tool,
        id: "resonance export".into(),
    };
    let mut clock = Hlc::next(None, now);
    let mut count = 0usize;
    let mut emit = |kind: &str, payload: serde_json::Value, out: &mut dyn Write| -> Result<()> {
        let op = Op {
            op_id: new_id(),
            kind: kind.to_string(),
            actor: actor.clone(),
            hlc: clock.to_string(),
            change_set: change_set.clone(),
            schema: SCHEMA_VERSION,
            payload,
        };
        clock = Hlc::next(Some(clock), now);
        writeln!(out, "{}", serde_json::to_string(&op)?).map_err(io)?;
        count += 1;
        Ok(())
    };

    let mut edges = r.find_edges(
        &EdgeQuery::all()
            .in_layer(Layer::Authored)
            .including_removed(),
    )?;
    edges.sort_by(|a, b| a.id.cmp(&b.id));
    let referenced: BTreeSet<&str> = edges
        .iter()
        .flat_map(|e| [e.src.as_str(), e.dst.as_str()])
        .collect();

    let order = [
        NodeType::Requirement,
        NodeType::Decision,
        NodeType::File,
        NodeType::CodeUnit,
        NodeType::Test,
        NodeType::Annotation,
        NodeType::Evidence,
    ];
    for t in order {
        for n in r.find_nodes(&NodeQuery::of_type(t).including_retired())? {
            if n.layer == Layer::Derived {
                continue;
            }
            if t == NodeType::File && !referenced.contains(n.id.as_str()) {
                continue;
            }
            emit(
                kind::NODE_CREATE,
                json!({"id": n.id, "type": n.ntype, "layer": n.layer, "props": n.props}),
                out,
            )?;
            if n.retired {
                emit(kind::NODE_RETIRE, json!({"id": n.id}), out)?;
            }
        }
    }
    for e in &edges {
        emit(
            kind::EDGE_ADD,
            json!({
                "id": e.id, "type": e.etype, "src": e.src, "dst": e.dst,
                "layer": e.layer, "status": e.status, "props": e.props,
            }),
            out,
        )?;
        if e.removed {
            emit(kind::EDGE_REMOVE, json!({"id": e.id}), out)?;
        }
    }
    out.flush().map_err(io)?;
    Ok(count)
}
