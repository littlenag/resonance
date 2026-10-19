//! Requirement to code to tests, with statuses and latest evidence (S2).

use serde::{Deserialize, Serialize};

use super::{
    AnnotationView, DecisionView, LinkView, RequirementView, UnitView, annotations_on,
    decision_view, link_view, requirement_view, unit_view,
};
use crate::error::{Error, Result};
use crate::model::{EdgeType, Node, NodeType};
use crate::store::{EdgeQuery, GraphRead};

/// One linked unit, test or file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEntry {
    pub link: LinkView,
    /// Set when the linked node is a code unit or test.
    pub unit: Option<UnitView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionLink {
    pub link: LinkView,
    pub decision: DecisionView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceView {
    pub requirement: RequirementView,
    /// Code units and files linked by `satisfies`.
    pub satisfied_by: Vec<TraceEntry>,
    /// Tests linked by `verifies`, with their latest evidence.
    pub verified_by: Vec<TraceEntry>,
    pub constrained_by: Vec<DecisionLink>,
    pub annotations: Vec<AnnotationView>,
}

fn entries(r: &dyn GraphRead, req: &Node, etype: EdgeType) -> Result<Vec<TraceEntry>> {
    let mut out = Vec::new();
    for e in r.find_edges(&EdgeQuery::to(&req.id).of_type(etype))? {
        let unit = match r.get_node(&e.src)? {
            Some(n) if n.ntype.is_anchored() => Some(unit_view(r, &n)?),
            _ => None,
        };
        out.push(TraceEntry {
            link: link_view(r, &e, &req.id)?,
            unit,
        });
    }
    out.sort_by(|a, b| a.link.other.label.cmp(&b.link.other.label));
    Ok(out)
}

/// One call returns the code units linked by `satisfies`, the tests linked by `verifies`,
/// and each link's status and latest evidence (S2 AC1).
pub fn trace(r: &dyn GraphRead, req: &Node) -> Result<TraceView> {
    if req.ntype != NodeType::Requirement {
        return Err(Error::Invalid(format!(
            "trace starts from a requirement; this is a {} (try resonance_why)",
            req.ntype
        )));
    }
    let mut constrained_by = Vec::new();
    for e in r.find_edges(&EdgeQuery::to(&req.id).of_type(EdgeType::Constrains))? {
        if let Some(d) = r.get_node(&e.src)? {
            constrained_by.push(DecisionLink {
                link: link_view(r, &e, &req.id)?,
                decision: decision_view(&d),
            });
        }
    }
    Ok(TraceView {
        requirement: requirement_view(req),
        satisfied_by: entries(r, req, EdgeType::Satisfies)?,
        verified_by: entries(r, req, EdgeType::Verifies)?,
        constrained_by,
        annotations: annotations_on(r, &req.id, false)?,
    })
}
