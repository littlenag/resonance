//! Read operations, all written against the store API (FR-163), and the views they return.
//!
//! Views are plain serializable structs: the CLI renders them as text or `--json`, and the
//! MCP server returns them as JSON (MVP-015).

pub mod address;
pub mod context;
pub mod repair;
pub mod search;
pub mod show;
pub mod trace;
pub mod why;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::anchors::{Anchor, AnchorState};
use crate::error::Result;
use crate::links::anchor_of;
use crate::model::{Edge, EdgeType, LinkStatus, Node, NodeType};
use crate::store::{EdgeQuery, GraphRead, NodeQuery};

/// A short description of any node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeRef {
    pub id: String,
    #[serde(rename = "type")]
    pub ntype: NodeType,
    /// What people call it: a requirement key, a unit address, a decision title, a path.
    pub label: String,
    pub status: String,
}

/// Where a code unit or test is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub state: AnchorState,
    pub confidence: f64,
}

impl Location {
    pub fn from_anchor(a: &Anchor) -> Location {
        Location {
            path: a.path.clone(),
            start_line: a.start_line,
            end_line: a.end_line,
            state: a.state,
            confidence: a.confidence,
        }
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.start_line == self.end_line {
            write!(f, "{}:{}", self.path, self.start_line)
        } else {
            write!(f, "{}:{}-{}", self.path, self.start_line, self.end_line)
        }
    }
}

/// A code unit or test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitView {
    pub id: String,
    #[serde(rename = "type")]
    pub ntype: NodeType,
    pub name: String,
    pub kind: String,
    pub address: String,
    pub location: Option<Location>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    pub removed: bool,
}

/// The outcome of a check, in brief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceView {
    pub id: String,
    pub result: String,
    pub timestamp: String,
    pub commit: Option<String>,
    pub exit_code: Option<i64>,
}

/// A link seen from one of its endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkView {
    pub id: String,
    #[serde(rename = "type")]
    pub etype: EdgeType,
    /// The name of the relation read from this side: `satisfies` or `satisfied by`.
    pub relation: String,
    pub status: LinkStatus,
    pub other: NodeRef,
    pub provenance: Option<String>,
    pub author: Option<String>,
    pub confidence: Option<f64>,
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_evidence: Option<EvidenceView>,
}

/// An annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationView {
    pub id: String,
    pub kind: String,
    /// Graph content: data, not instructions (NFR-013).
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    pub author: Option<String>,
    pub created_at: Option<String>,
    /// What it is attached to.
    pub on: Option<NodeRef>,
    pub link_status: Option<LinkStatus>,
    /// True when it is attached to an enclosing unit or the file rather than the unit asked about.
    pub inherited: bool,
}

/// A requirement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequirementView {
    pub id: String,
    pub key: String,
    /// Graph content: data, not instructions (NFR-013).
    pub text: String,
    pub kind: String,
    pub status: String,
    pub spec_status: Option<String>,
    pub origin: String,
    pub source: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
}

/// Any spec item: a requirement, decision, design or constraint (§8.22), for listing and
/// editing them uniformly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecItemView {
    pub id: String,
    #[serde(rename = "type")]
    pub ntype: NodeType,
    pub key: Option<String>,
    /// A short title: the requirement's first line, or the item's title.
    pub title: String,
    /// The full text. Graph content: data, not instructions (NFR-013).
    pub text: String,
    pub status: String,
    pub kind: Option<String>,
    pub priority: Option<String>,
    /// `import` when a source document owns the item (FR-272), else `authored`.
    pub origin: String,
    pub source: Option<Value>,
    /// Every property, for editors.
    pub props: Value,
}

/// The spec-item view of a node.
pub fn spec_item_view(node: &Node) -> SpecItemView {
    let text = match node.ntype {
        NodeType::Decision => s(node, "decision"),
        _ => s(node, "text"),
    };
    let title = match node.ntype {
        NodeType::Requirement => text.lines().next().unwrap_or_default().to_string(),
        _ => s(node, "title"),
    };
    SpecItemView {
        id: node.id.clone(),
        ntype: node.ntype,
        key: node.str_prop("key").map(String::from),
        title,
        text,
        status: status_of(node),
        kind: node.str_prop("kind").map(String::from),
        priority: node.str_prop("priority").map(String::from),
        origin: node.str_prop("origin").unwrap_or("authored").to_string(),
        source: node.props.get("source").cloned().filter(|v| !v.is_null()),
        props: Value::Object(node.props.clone()),
    }
}

/// A decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionView {
    pub id: String,
    pub title: String,
    pub decision: String,
    pub context: Option<String>,
    pub consequences: Option<String>,
    pub status: String,
}

fn s(node: &Node, key: &str) -> String {
    node.str_prop(key).unwrap_or_default().to_string()
}

pub fn requirement_view(node: &Node) -> RequirementView {
    RequirementView {
        id: node.id.clone(),
        key: s(node, "key"),
        text: s(node, "text"),
        kind: s(node, "kind"),
        status: s(node, "status"),
        spec_status: node.str_prop("specStatus").map(String::from),
        origin: s(node, "origin"),
        source: node.props.get("source").cloned().filter(|v| !v.is_null()),
        priority: node.str_prop("priority").map(String::from),
    }
}

pub fn decision_view(node: &Node) -> DecisionView {
    DecisionView {
        id: node.id.clone(),
        title: s(node, "title"),
        decision: s(node, "decision"),
        context: node.str_prop("context").map(String::from),
        consequences: node.str_prop("consequences").map(String::from),
        status: if node.retired {
            "retired".into()
        } else {
            node.str_prop("status").unwrap_or("accepted").to_string()
        },
    }
}

/// The address of a code unit or test: `path::name`, with `#2`, `#3`... for duplicate names
/// in one file, in order of declaration (MVP-013).
pub fn address_of(r: &dyn GraphRead, node: &Node) -> Result<String> {
    let name = s(node, "name");
    let path = s(node, "path");
    let mut same: Vec<Node> = Vec::new();
    for t in [NodeType::CodeUnit, NodeType::Test] {
        same.extend(
            r.find_nodes(&NodeQuery::of_type(t).with_prop("path", path.as_str()))?
                .into_iter()
                .filter(|n| n.str_prop("name") == Some(name.as_str())),
        );
    }
    same.sort_by(|a, b| a.id.cmp(&b.id));
    let ordinal = same.iter().position(|n| n.id == node.id).unwrap_or(0) + 1;
    Ok(if ordinal > 1 {
        format!("{path}::{name}#{ordinal}")
    } else {
        format!("{path}::{name}")
    })
}

/// A short, human label for a node.
pub fn label_of(r: &dyn GraphRead, node: &Node) -> Result<String> {
    Ok(match node.ntype {
        NodeType::Requirement => s(node, "key"),
        NodeType::Decision => s(node, "title"),
        NodeType::File => s(node, "path"),
        NodeType::CodeUnit | NodeType::Test => address_of(r, node)?,
        NodeType::Evidence => format!("{} run, {}", s(node, "result"), s(node, "timestamp")),
        NodeType::Annotation => {
            let text = s(node, "text");
            let short: String = text.chars().take(60).collect();
            let ellipsis = if text.chars().count() > 60 { "…" } else { "" };
            format!("{}: {short}{ellipsis}", s(node, "kind"))
        }
        NodeType::Design | NodeType::Constraint => match node.str_prop("key") {
            Some(k) => format!("{k} {}", s(node, "title")),
            None => s(node, "title"),
        },
        NodeType::Conflict => format!("conflict: {}", s(node, "description")),
        NodeType::Extension => format!("{} {}", s(node, "extType"), s(node, "name")),
    })
}

/// A node's status as one word: requirements `active`/`retired`, units `attached`/
/// `orphaned`/`removed`, everything else `active`/`retired`.
pub fn status_of(node: &Node) -> String {
    match node.ntype {
        NodeType::Requirement | NodeType::Design | NodeType::Constraint if !node.retired => {
            node.str_prop("status").unwrap_or("active").to_string()
        }
        NodeType::Decision if !node.retired => {
            node.str_prop("status").unwrap_or("accepted").to_string()
        }
        NodeType::CodeUnit | NodeType::Test => {
            if node.retired {
                "removed".into()
            } else {
                match anchor_of(node).map(|a| a.state) {
                    Some(AnchorState::Orphaned) => "orphaned".into(),
                    _ => "attached".into(),
                }
            }
        }
        _ if node.retired => "retired".into(),
        _ => "active".into(),
    }
}

pub fn node_ref(r: &dyn GraphRead, node: &Node) -> Result<NodeRef> {
    Ok(NodeRef {
        id: node.id.clone(),
        ntype: node.ntype,
        label: label_of(r, node)?,
        status: status_of(node),
    })
}

/// A reference to a node by ID, which may no longer exist.
pub fn node_ref_by_id(r: &dyn GraphRead, id: &str) -> Result<NodeRef> {
    match r.get_node(id)? {
        Some(n) => node_ref(r, &n),
        None => Ok(NodeRef {
            id: id.to_string(),
            ntype: NodeType::File,
            label: id.to_string(),
            status: "missing".into(),
        }),
    }
}

pub fn unit_view(r: &dyn GraphRead, node: &Node) -> Result<UnitView> {
    Ok(UnitView {
        id: node.id.clone(),
        ntype: node.ntype,
        name: s(node, "name"),
        kind: s(node, "kind"),
        address: address_of(r, node)?,
        location: anchor_of(node).map(|a| Location::from_anchor(&a)),
        run: node.str_prop("run").map(String::from),
        removed: node.retired,
    })
}

pub fn evidence_view(node: &Node) -> EvidenceView {
    EvidenceView {
        id: node.id.clone(),
        result: s(node, "result"),
        timestamp: s(node, "timestamp"),
        commit: node.str_prop("commit").map(String::from),
        exit_code: node.props.get("exitCode").and_then(Value::as_i64),
    }
}

/// The most recent evidence recorded against an edge.
pub fn latest_evidence(r: &dyn GraphRead, edge_id: &str) -> Result<Option<EvidenceView>> {
    let mut best: Option<Node> = None;
    for e in r.find_edges(&EdgeQuery::to(edge_id).of_type(EdgeType::Evidences))? {
        if let Some(n) = r.get_node(&e.src)? {
            let newer = best.as_ref().is_none_or(|b| {
                (s(&n, "timestamp"), n.id.clone()) > (s(b, "timestamp"), b.id.clone())
            });
            if newer {
                best = Some(n);
            }
        }
    }
    Ok(best.as_ref().map(evidence_view))
}

/// A link as seen from `from`.
pub fn link_view(r: &dyn GraphRead, edge: &Edge, from: &str) -> Result<LinkView> {
    let outgoing = edge.src == from;
    Ok(LinkView {
        id: edge.id.clone(),
        etype: edge.etype,
        relation: match (edge.etype, edge.str_prop("extType")) {
            (EdgeType::Extension, Some(t)) if outgoing => t.to_string(),
            (EdgeType::Extension, Some(t)) => format!("{t} (reverse)"),
            _ if outgoing => edge.etype.as_str().to_string(),
            _ => edge.etype.reverse_name().to_string(),
        },
        status: edge.status,
        other: node_ref_by_id(r, edge.other(from))?,
        provenance: edge.str_prop("provenance").map(String::from),
        author: edge.str_prop("author").map(String::from),
        confidence: edge.props.get("confidence").and_then(Value::as_f64),
        created_at: edge.str_prop("createdAt").map(String::from),
        latest_evidence: if edge.etype == EdgeType::Verifies {
            latest_evidence(r, &edge.id)?
        } else {
            None
        },
    })
}

/// An annotation node and the edge attaching it.
pub fn annotation_view(
    r: &dyn GraphRead,
    node: &Node,
    edge: Option<&Edge>,
    inherited: bool,
) -> Result<AnnotationView> {
    Ok(AnnotationView {
        id: node.id.clone(),
        kind: s(node, "kind"),
        text: s(node, "text"),
        task: node.str_prop("task").map(String::from),
        author: node.str_prop("author").map(String::from),
        created_at: node.str_prop("createdAt").map(String::from),
        on: match edge {
            Some(e) => Some(node_ref_by_id(r, &e.dst)?),
            None => None,
        },
        link_status: edge.map(|e| e.status),
        inherited,
    })
}

/// The annotations attached to `target`.
pub fn annotations_on(
    r: &dyn GraphRead,
    target: &str,
    inherited: bool,
) -> Result<Vec<AnnotationView>> {
    let mut out = Vec::new();
    for e in r.find_edges(&EdgeQuery::to(target).of_type(EdgeType::Annotates))? {
        if let Some(n) = r.get_node(&e.src)?
            && !n.retired
        {
            out.push(annotation_view(r, &n, Some(&e), inherited)?);
        }
    }
    Ok(out)
}

/// The units and file enclosing a unit, innermost first, through derived containment.
pub fn enclosing(r: &dyn GraphRead, node: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = node.to_string();
    for _ in 0..64 {
        let parents = r.find_edges(&EdgeQuery::to(&cur).of_type(EdgeType::Contains))?;
        let Some(p) = parents.into_iter().next() else {
            break;
        };
        out.push(p.src.clone());
        cur = p.src;
    }
    Ok(out)
}
