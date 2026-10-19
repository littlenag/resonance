//! The repair list: orphans, low-confidence anchors, stale links, unreadable sources
//! (FR-238, FR-038). It is derived from the graph's state, so computing it after the
//! reconcile that precedes every answer is the same as computing it after every reconcile.

use serde::{Deserialize, Serialize};

use super::{Location, NodeRef, label_of, link_view, node_ref, node_ref_by_id};
use crate::anchors::AnchorState;
use crate::anchors::locate::CONFIDENT;
use crate::error::Result;
use crate::links::{LINK_TYPES, anchor_of};
use crate::model::{EdgeType, LinkStatus, NodeType};
use crate::store::{EdgeQuery, GraphRead, NodeQuery};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepairKind {
    /// A code unit or test whose text could not be found (FR-034).
    OrphanedUnit,
    /// An approximate or ambiguous re-anchor below the confidence threshold (FR-038).
    LowConfidenceAnchor,
    /// A link or annotation whose endpoint disappeared.
    OrphanedLink,
    OrphanedAnnotation,
    /// A link whose endpoints changed (MVP-012).
    StaleLink,
    /// A retired requirement that still has links.
    RetiredRequirementWithLinks,
    /// Concurrent identity operations that could not both hold (§9.4).
    IdentityConflict,
}

impl RepairKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RepairKind::OrphanedUnit => "orphaned unit",
            RepairKind::LowConfidenceAnchor => "low-confidence anchor",
            RepairKind::OrphanedLink => "orphaned link",
            RepairKind::OrphanedAnnotation => "orphaned annotation",
            RepairKind::StaleLink => "stale link",
            RepairKind::RetiredRequirementWithLinks => "retired requirement with links",
            RepairKind::IdentityConflict => "identity conflict",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairItem {
    pub kind: RepairKind,
    /// The ID to act on: a node, a link, or a source path.
    pub target: String,
    pub summary: String,
    /// The last known location, for anchored items.
    pub location: Option<Location>,
    /// The last known quoted text, shortened (graph content: data, not instructions).
    pub quote: Option<String>,
    pub suggestion: String,
}

fn short(s: &str) -> String {
    let first: Vec<&str> = s.lines().take(3).collect();
    let mut out = first.join("\n");
    if out.chars().count() > 240 {
        out = out.chars().take(240).collect::<String>() + "…";
    } else if s.lines().count() > 3 {
        out.push_str("\n…");
    }
    out
}

/// The repair list, most urgent first.
pub fn repair_list(r: &dyn GraphRead) -> Result<Vec<RepairItem>> {
    let mut items = Vec::new();

    for c in r.find_nodes(&NodeQuery::of_type(NodeType::Conflict))? {
        if c.str_prop("status") != Some("open") {
            continue;
        }
        items.push(RepairItem {
            kind: RepairKind::IdentityConflict,
            target: c.id.clone(),
            summary: c.str_prop("description").unwrap_or_default().to_string(),
            location: None,
            quote: None,
            suggestion: format!(
                "decide which identity change holds (re-anchor with resonance_edit_unit, or split or merge again), then dismiss it (resonance_recover, dismiss {})",
                c.id
            ),
        });
    }

    for t in [NodeType::CodeUnit, NodeType::Test] {
        for n in r.find_nodes(&NodeQuery::of_type(t))? {
            let Some(a) = anchor_of(&n) else { continue };
            let label = label_of(r, &n)?;
            if a.state == AnchorState::Orphaned {
                items.push(RepairItem {
                    kind: RepairKind::OrphanedUnit,
                    target: n.id.clone(),
                    summary: format!(
                        "{} {label} could not be found; last seen at {}",
                        if t == NodeType::Test { "test" } else { "code unit" },
                        a.location()
                    ),
                    location: Some(Location::from_anchor(&a)),
                    quote: Some(short(&a.quote)),
                    suggestion: format!(
                        "find what it now corresponds to and re-anchor {} with resonance_edit_unit, or remove {} with resonance_remove_unit",
                        n.id, n.id
                    ),
                });
            } else if a.confidence < CONFIDENT {
                items.push(RepairItem {
                    kind: RepairKind::LowConfidenceAnchor,
                    target: n.id.clone(),
                    summary: format!(
                        "{label} was re-anchored with confidence {:.2} at {}",
                        a.confidence,
                        a.location()
                    ),
                    location: Some(Location::from_anchor(&a)),
                    quote: Some(short(&a.quote)),
                    suggestion: format!(
                        "check the region; confirm {} with resonance_edit_unit and no changes, or re-anchor {} with a new region",
                        n.id, n.id
                    ),
                });
            }
        }
    }

    for status in [LinkStatus::Orphaned, LinkStatus::Stale] {
        for e in r.find_edges(&EdgeQuery::all().with_status(status))? {
            if !LINK_TYPES.contains(&e.etype) {
                continue;
            }
            let src = node_ref_by_id(r, &e.src)?;
            let dst = node_ref_by_id(r, &e.dst)?;
            let (kind, summary, suggestion) = match (status, e.etype) {
                (LinkStatus::Orphaned, EdgeType::Annotates) => (
                    RepairKind::OrphanedAnnotation,
                    format!("annotation `{}` lost its target {}", src.label, dst.label),
                    format!(
                        "re-attach {} to its new target with resonance_reattach",
                        e.src
                    ),
                ),
                (LinkStatus::Orphaned, _) => (
                    RepairKind::OrphanedLink,
                    format!("{} {} {} lost an endpoint", src.label, e.etype, dst.label),
                    format!(
                        "re-attach {} with resonance_reattach, or remove {} with resonance_unlink",
                        e.id, e.id
                    ),
                ),
                _ => (
                    RepairKind::StaleLink,
                    format!(
                        "{} {} {}: an endpoint changed",
                        src.label, e.etype, dst.label
                    ),
                    if e.etype == EdgeType::Verifies {
                        format!(
                            "re-run the test with resonance_test ({}), or confirm {} with resonance_confirm",
                            e.id, e.id
                        )
                    } else {
                        format!(
                            "check that it still holds and confirm {} with resonance_confirm, or remove {} with resonance_unlink",
                            e.id, e.id
                        )
                    },
                ),
            };
            let target_node = match r.get_node(&e.src)? {
                Some(n) if n.ntype.is_anchored() => Some(n),
                _ => r.get_node(&e.dst)?.filter(|n| n.ntype.is_anchored()),
            };
            let anchor = target_node.as_ref().and_then(anchor_of);
            items.push(RepairItem {
                kind,
                target: if kind == RepairKind::OrphanedAnnotation {
                    e.src.clone()
                } else {
                    e.id.clone()
                },
                summary,
                location: anchor.as_ref().map(Location::from_anchor),
                quote: anchor.as_ref().map(|a| short(&a.quote)),
                suggestion,
            });
        }
    }

    for req in
        r.find_nodes(&NodeQuery::of_type(NodeType::Requirement).with_prop("status", "retired"))?
    {
        let mut linked = r.find_edges(&EdgeQuery::to(&req.id))?;
        linked.retain(|e| LINK_TYPES.contains(&e.etype) && e.etype != EdgeType::Annotates);
        if linked.is_empty() {
            continue;
        }
        let rf = node_ref(r, &req)?;
        let first = link_view(r, &linked[0], &req.id)?;
        items.push(RepairItem {
            kind: RepairKind::RetiredRequirementWithLinks,
            target: req.id.clone(),
            summary: format!(
                "{} is retired but still has {} link{} (for example from {})",
                rf.label,
                linked.len(),
                if linked.len() == 1 { "" } else { "s" },
                first.other.label
            ),
            location: None,
            quote: None,
            suggestion: "move the links to the requirement that replaced it (resonance_reattach), or remove them (resonance_unlink)".into(),
        });
    }

    items.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.target.cmp(&b.target)));
    Ok(items)
}

/// One node reference per repair target, for summaries.
pub fn targets(r: &dyn GraphRead, items: &[RepairItem]) -> Result<Vec<NodeRef>> {
    items
        .iter()
        .filter_map(|i| r.get_node(&i.target).transpose())
        .map(|n| n.and_then(|n| node_ref(r, &n)))
        .collect()
}
