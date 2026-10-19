//! The link lifecycle: endpoint hashes, staleness, orphaning and restoring (§6.5, FR-001–FR-003,
//! MVP-005, MVP-012).

use serde_json::{Value, json};

use crate::anchors::{Anchor, AnchorState};
use crate::error::Result;
use crate::model::{Edge, EdgeType, Layer, LinkStatus, Node, NodeType, Props, content_hash};
use crate::ops::{Tx, hlc};
use crate::store::{EdgeQuery, GraphRead};

/// Authored edge types whose status follows their endpoints.
pub const LINK_TYPES: [EdgeType; 8] = [
    EdgeType::Satisfies,
    EdgeType::Verifies,
    EdgeType::Constrains,
    EdgeType::Annotates,
    EdgeType::DerivedFrom,
    EdgeType::Supersedes,
    EdgeType::Realizes,
    EdgeType::Extension,
];

/// The anchor of a code unit or test, if it has one.
pub fn anchor_of(node: &Node) -> Option<Anchor> {
    node.props
        .get("anchor")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// The hash of the content an edge depends on, per node type (MVP-012).
pub fn node_hash(node: &Node) -> Option<String> {
    match node.ntype {
        NodeType::Requirement => node.str_prop("text").map(|t| content_hash(t.as_bytes())),
        NodeType::CodeUnit | NodeType::Test => anchor_of(node).map(|a| a.quote_hash),
        NodeType::File => node.str_prop("blob").map(String::from),
        NodeType::Decision => {
            let parts: Vec<&str> = ["title", "context", "decision", "consequences"]
                .iter()
                .map(|k| node.str_prop(k).unwrap_or(""))
                .collect();
            Some(content_hash(parts.join("\u{1f}").as_bytes()))
        }
        NodeType::Annotation => node.str_prop("text").map(|t| content_hash(t.as_bytes())),
        NodeType::Design | NodeType::Constraint | NodeType::Conflict | NodeType::Extension => {
            let parts: Vec<&str> = ["title", "text", "description", "scope", "name"]
                .iter()
                .map(|k| node.str_prop(k).unwrap_or(""))
                .collect();
            Some(content_hash(parts.join("\u{1f}").as_bytes()))
        }
        NodeType::Evidence => None,
    }
}

/// Whether a node can be the endpoint of a live (non-orphaned) link: it exists, is not
/// retired and, if it is anchored, its anchor is attached.
pub fn is_live(node: &Node) -> bool {
    if node.retired {
        return false;
    }
    if node.ntype.is_anchored() {
        return anchor_of(node).is_some_and(|a| a.state == AnchorState::Attached);
    }
    true
}

/// The endpoint-hash properties of a new or refreshed edge.
pub fn endpoint_hashes(r: &dyn GraphRead, src: &str, dst: &str) -> Result<(Value, Value)> {
    let h = |id: &str| -> Result<Value> {
        Ok(r.get_node(id)?
            .as_ref()
            .and_then(node_hash)
            .map(Value::String)
            .unwrap_or(Value::Null))
    };
    Ok((h(src)?, h(dst)?))
}

/// Properties every authored edge carries (§6.3, FR-134).
pub fn new_link_props(
    r: &dyn GraphRead,
    src: &str,
    dst: &str,
    provenance: &str,
    author: &str,
    confidence: Option<f64>,
) -> Result<Props> {
    let (src_hash, dst_hash) = endpoint_hashes(r, src, dst)?;
    let mut p = Props::new();
    p.insert("provenance".into(), json!(provenance));
    p.insert("author".into(), json!(author));
    p.insert("createdAt".into(), json!(hlc::rfc3339(hlc::now_ms())));
    if let Some(c) = confidence {
        p.insert("confidence".into(), json!(c));
    }
    p.insert("srcHash".into(), src_hash);
    p.insert("dstHash".into(), dst_hash);
    Ok(p)
}

/// A new authored edge, starting as `asserted` (FR-134).
pub fn new_link(etype: EdgeType, src: &str, dst: &str, props: Props) -> Edge {
    Edge {
        id: crate::model::new_id(),
        etype,
        src: src.to_string(),
        dst: dst.to_string(),
        layer: Layer::Authored,
        status: LinkStatus::Asserted,
        removed: false,
        props,
    }
}

/// Links (authored, not removed) incident to `node`.
pub fn links_of(r: &dyn GraphRead, node: &str) -> Result<Vec<Edge>> {
    let mut edges = r.find_edges(&EdgeQuery::from(node))?;
    edges.extend(r.find_edges(&EdgeQuery::to(node))?);
    edges.retain(|e| LINK_TYPES.contains(&e.etype) && e.layer == Layer::Authored);
    edges.sort_by(|a, b| a.id.cmp(&b.id));
    edges.dedup_by(|a, b| a.id == b.id);
    Ok(edges)
}

/// Counts of status changes, for reports.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusChanges {
    pub stale: usize,
    pub orphaned: usize,
    pub restored: usize,
}

impl std::ops::AddAssign for StatusChanges {
    fn add_assign(&mut self, o: Self) {
        self.stale += o.stale;
        self.orphaned += o.orphaned;
        self.restored += o.restored;
    }
}

/// An endpoint of these links disappeared without an identity operation: orphan them,
/// remembering their status so that MVP-005 can restore it (FR-003, FR-034).
pub fn orphan_links(tx: &mut Tx<'_>, node: &str) -> Result<StatusChanges> {
    let mut changes = StatusChanges::default();
    for e in links_of(tx.r(), node)? {
        if e.status == LinkStatus::Orphaned {
            continue;
        }
        tx.set_edge(&e.id, "priorStatus", json!(e.status))?;
        tx.set_edge(&e.id, "status", json!(LinkStatus::Orphaned))?;
        changes.orphaned += 1;
    }
    Ok(changes)
}

/// Return orphaned links whose endpoints are all live again to the status they had before
/// (MVP-005). Links that were orphaned when created through a re-attach are left alone.
pub fn restore_links(tx: &mut Tx<'_>, node: &str) -> Result<StatusChanges> {
    let mut changes = StatusChanges::default();
    for e in links_of(tx.r(), node)? {
        if e.status != LinkStatus::Orphaned {
            continue;
        }
        let live = |id: &str| -> Result<bool> {
            Ok(match tx.r().get_node(id)? {
                Some(n) => is_live(&n),
                None => false,
            })
        };
        if !live(&e.src)? || !live(&e.dst)? {
            continue;
        }
        let prior: LinkStatus = e
            .props
            .get("priorStatus")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(LinkStatus::Asserted);
        tx.set_edge(&e.id, "status", json!(prior))?;
        tx.set_edge(&e.id, "priorStatus", Value::Null)?;
        changes.restored += 1;
    }
    Ok(changes)
}

/// Apply the staleness rules (MVP-012) to the links of a node whose content may have changed:
///
/// - a `verified` link whose either endpoint changed becomes `stale` (FR-002);
/// - any link whose `Requirement` endpoint text changed becomes `stale` (S7 AC1);
/// - edits under an `asserted` link leave it `asserted`.
pub fn refresh_statuses(tx: &mut Tx<'_>, node: &str) -> Result<StatusChanges> {
    let mut changes = StatusChanges::default();
    for e in links_of(tx.r(), node)? {
        if matches!(e.status, LinkStatus::Orphaned | LinkStatus::Stale) {
            continue;
        }
        let src = tx.r().get_node(&e.src)?;
        let dst = tx.r().get_node(&e.dst)?;
        let (Some(src), Some(dst)) = (src, dst) else {
            continue;
        };
        let differs = |n: &Node, key: &str| -> bool {
            let stored = e.props.get(key).and_then(Value::as_str);
            match (stored, node_hash(n)) {
                (Some(s), Some(h)) => s != h,
                _ => false,
            }
        };
        let src_changed = differs(&src, "srcHash");
        let dst_changed = differs(&dst, "dstHash");
        let requirement_changed = (src.ntype == NodeType::Requirement && src_changed)
            || (dst.ntype == NodeType::Requirement && dst_changed);
        let verified_changed = e.status == LinkStatus::Verified && (src_changed || dst_changed);
        // An item and the document region it came from: either side changing means the agent
        // has to bring them back in step (FR-124).
        let source_changed = e.etype == EdgeType::DerivedFrom
            && matches!(dst.ntype, NodeType::CodeUnit | NodeType::File)
            && (src_changed || dst_changed);
        if requirement_changed || verified_changed || source_changed {
            tx.set_edge(&e.id, "status", json!(LinkStatus::Stale))?;
            changes.stale += 1;
        }
    }
    Ok(changes)
}

/// Refresh a link's endpoint hashes to the current content (on confirm and verification).
pub fn refresh_hashes(tx: &mut Tx<'_>, edge: &Edge) -> Result<()> {
    let (src_hash, dst_hash) = endpoint_hashes(tx.r(), &edge.src, &edge.dst)?;
    tx.set_edge(&edge.id, "srcHash", src_hash)?;
    tx.set_edge(&edge.id, "dstHash", dst_hash)
}
