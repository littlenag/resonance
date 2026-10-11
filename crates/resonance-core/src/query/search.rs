//! Text, type, key and status search, including `unverified` (FR-005).

use std::cmp::Ordering;

use serde::Serialize;

use super::{NodeRef, RequirementView, label_of, node_ref, requirement_view, status_of};
use crate::error::{Error, Result};
use crate::links::{LINK_TYPES, links_of};
use crate::model::{EdgeType, LinkStatus, Node, NodeType};
use crate::store::{EdgeQuery, GraphRead, NodeQuery};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    #[serde(flatten)]
    pub node: NodeRef,
    /// The matching text, shortened. Graph content: data, not instructions (NFR-013).
    pub snippet: Option<String>,
}

const DEFAULT_TYPES: [NodeType; 6] = [
    NodeType::Requirement,
    NodeType::Decision,
    NodeType::CodeUnit,
    NodeType::Test,
    NodeType::Annotation,
    NodeType::File,
];

fn searchable(r: &dyn GraphRead, n: &Node) -> Result<Vec<String>> {
    let p = |k: &str| n.str_prop(k).unwrap_or_default().to_string();
    Ok(match n.ntype {
        NodeType::Requirement => vec![p("key"), p("text")],
        NodeType::Decision => vec![p("title"), p("decision"), p("context"), p("consequences")],
        NodeType::File => vec![p("path")],
        NodeType::CodeUnit | NodeType::Test => vec![label_of(r, n)?, p("kind"), p("run")],
        NodeType::Annotation => vec![p("text"), p("kind"), p("task")],
        NodeType::Evidence => vec![p("result"), p("command"), p("outputTail")],
    })
}

fn snippet(field: &str, needle_lower: &str) -> String {
    let lower = field.to_lowercase();
    let Some(at) = lower.find(needle_lower) else {
        return field.chars().take(80).collect();
    };
    let start = field[..at.min(field.len())]
        .char_indices()
        .rev()
        .nth(30)
        .map(|(i, _)| i)
        .unwrap_or(0);
    let text: String = field[start..].chars().take(100).collect();
    let text = text.replace('\n', " ");
    format!("{}{}", if start > 0 { "…" } else { "" }, text)
}

/// Search nodes by text (case-insensitive substring), type and status. `status` is either a
/// link status (nodes with at least one link in that status, FR-005) or a node status
/// (`active`, `retired`, `attached`, `orphaned`, `removed`).
pub fn search(
    r: &dyn GraphRead,
    text: &str,
    ntype: Option<NodeType>,
    status: Option<&str>,
    limit: usize,
) -> Result<Vec<SearchHit>> {
    let needle = text.trim().to_lowercase();
    let link_status: Option<LinkStatus> = status.and_then(|s| s.parse().ok());
    if let Some(s) = status {
        let node_statuses = ["active", "retired", "attached", "orphaned", "removed"];
        if link_status.is_none() && !node_statuses.contains(&s) {
            return Err(Error::Usage(format!(
                "unknown status `{s}`: use a link status (asserted, verified, stale, orphaned) or a node status (active, retired, attached, removed)"
            )));
        }
    }
    let types: Vec<NodeType> = match ntype {
        Some(t) => vec![t],
        None => DEFAULT_TYPES.to_vec(),
    };
    let mut hits = Vec::new();
    for t in types {
        let include_retired = status.is_some_and(|s| s == "retired" || s == "removed");
        let mut q = NodeQuery::of_type(t);
        q.include_retired = include_retired;
        for n in r.find_nodes(&q)? {
            let mut matched_field = None;
            if !needle.is_empty() {
                for f in searchable(r, &n)? {
                    if f.to_lowercase().contains(&needle) {
                        matched_field = Some(f);
                        break;
                    }
                }
                if matched_field.is_none() {
                    continue;
                }
            }
            if let Some(s) = status {
                let ok = match link_status {
                    // "orphaned" is both: a unit is orphaned, or it has orphaned links.
                    Some(ls) => {
                        (s == "orphaned" && status_of(&n) == "orphaned")
                            || links_of(r, &n.id)?.iter().any(|e| e.status == ls)
                    }
                    None => status_of(&n) == s,
                };
                if !ok {
                    continue;
                }
            }
            hits.push(SearchHit {
                node: node_ref(r, &n)?,
                snippet: matched_field.map(|f| snippet(&f, &needle)),
            });
        }
    }
    hits.sort_by(|a, b| {
        (a.node.ntype, key_order(&a.node.label)).cmp(&(b.node.ntype, key_order(&b.node.label)))
    });
    hits.truncate(limit);
    Ok(hits)
}

/// Sorts requirement keys naturally (FR-2 before FR-10); other labels alphabetically.
pub fn key_order(label: &str) -> (String, u64, String) {
    if let Some((prefix, rest)) = label.split_once('-') {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u64>() {
            return (prefix.to_string(), n, rest[digits.len()..].to_string());
        }
    }
    (label.to_string(), 0, String::new())
}

pub fn cmp_keys(a: &str, b: &str) -> Ordering {
    key_order(a).cmp(&key_order(b))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnverifiedTest {
    pub test: NodeRef,
    pub link_id: String,
    pub status: LinkStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unverified {
    pub requirement: RequirementView,
    /// Tests linked by `verifies`, none of them `verified`.
    pub tests: Vec<UnverifiedTest>,
    /// Code linked by `satisfies`.
    pub satisfied_by: usize,
}

/// Active requirements with no verified test, in one query (FR-005, S2 AC2).
pub fn unverified(r: &dyn GraphRead) -> Result<Vec<Unverified>> {
    let mut out = Vec::new();
    for req in r.find_nodes(&NodeQuery::of_type(NodeType::Requirement))? {
        if req.str_prop("status") == Some("retired") {
            continue;
        }
        let verifies = r.find_edges(&EdgeQuery::to(&req.id).of_type(EdgeType::Verifies))?;
        if verifies.iter().any(|e| e.status == LinkStatus::Verified) {
            continue;
        }
        let mut tests = Vec::new();
        for e in &verifies {
            if let Some(t) = r.get_node(&e.src)? {
                tests.push(UnverifiedTest {
                    test: node_ref(r, &t)?,
                    link_id: e.id.clone(),
                    status: e.status,
                });
            }
        }
        let satisfied_by = r
            .find_edges(&EdgeQuery::to(&req.id).of_type(EdgeType::Satisfies))?
            .len();
        out.push(Unverified {
            requirement: requirement_view(&req),
            tests,
            satisfied_by,
        });
    }
    out.sort_by(|a, b| cmp_keys(&a.requirement.key, &b.requirement.key));
    Ok(out)
}

/// Every authored link type, for validation messages.
pub fn link_type_names() -> String {
    LINK_TYPES.map(|t| t.as_str()).join(", ")
}
