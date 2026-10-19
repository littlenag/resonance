//! Code to requirements, decisions, tests and annotations (S3).

use serde::{Deserialize, Serialize};

use super::trace::DecisionLink;
use super::{
    AnnotationView, LinkView, NodeRef, RequirementView, UnitView, annotations_on, decision_view,
    enclosing, link_view, node_ref, node_ref_by_id, requirement_view, unit_view,
};
use crate::error::{Error, Result};
use crate::model::{EdgeType, Node, NodeType};
use crate::store::{EdgeQuery, GraphRead};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequirementLink {
    pub link: LinkView,
    pub requirement: RequirementView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestLink {
    pub link: LinkView,
    pub test: UnitView,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InheritedDecision {
    #[serde(flatten)]
    pub decision: DecisionLink,
    /// The enclosing unit or file the decision is attached to, when not this unit itself.
    pub via: Option<NodeRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhyView {
    pub node: NodeRef,
    pub unit: Option<UnitView>,
    /// Requirements it satisfies.
    pub requirements: Vec<RequirementLink>,
    /// Decisions that constrain it or what encloses it.
    pub decisions: Vec<InheritedDecision>,
    /// Tests that cover it (`verifies` into it).
    pub tests: Vec<TestLink>,
    /// For a test: what it verifies.
    pub verifies: Vec<LinkView>,
    /// Annotations on it and on the units and file that contain it.
    pub annotations: Vec<AnnotationView>,
    /// The units and file that contain it, innermost first.
    pub enclosed_by: Vec<NodeRef>,
}

/// One call returns the requirements a code unit satisfies, the decisions that constrain
/// it, the tests that cover it, and the annotations on it, including annotations on its
/// containing units (S3 AC1).
pub fn why(r: &dyn GraphRead, node: &Node) -> Result<WhyView> {
    if !matches!(
        node.ntype,
        NodeType::CodeUnit | NodeType::Test | NodeType::File
    ) {
        return Err(Error::Invalid(format!(
            "why starts from a code unit, test or file; this is a {} (try resonance_trace or resonance_show)",
            node.ntype
        )));
    }
    let mut requirements = Vec::new();
    for e in r.find_edges(&EdgeQuery::from(&node.id).of_type(EdgeType::Satisfies))? {
        if let Some(req) = r.get_node(&e.dst)? {
            requirements.push(RequirementLink {
                link: link_view(r, &e, &node.id)?,
                requirement: requirement_view(&req),
            });
        }
    }

    requirements
        .sort_by(|a, b| crate::query::search::cmp_keys(&a.requirement.key, &b.requirement.key));

    let enclosing_ids = enclosing(r, &node.id)?;
    let mut decisions = Vec::new();
    let mut annotations = annotations_on(r, &node.id, false)?;
    let scopes = std::iter::once((node.id.clone(), false))
        .chain(enclosing_ids.iter().map(|id| (id.clone(), true)));
    for (scope, inherited) in scopes {
        for e in r.find_edges(&EdgeQuery::to(&scope).of_type(EdgeType::Constrains))? {
            if let Some(d) = r.get_node(&e.src)? {
                decisions.push(InheritedDecision {
                    decision: DecisionLink {
                        link: link_view(r, &e, &scope)?,
                        decision: decision_view(&d),
                    },
                    via: if inherited {
                        Some(node_ref_by_id(r, &scope)?)
                    } else {
                        None
                    },
                });
            }
        }
        if inherited {
            annotations.extend(annotations_on(r, &scope, true)?);
        }
    }

    let mut tests = Vec::new();
    for e in r.find_edges(&EdgeQuery::to(&node.id).of_type(EdgeType::Verifies))? {
        if let Some(t) = r.get_node(&e.src)? {
            tests.push(TestLink {
                link: link_view(r, &e, &node.id)?,
                test: unit_view(r, &t)?,
            });
        }
    }
    let verifies = r
        .find_edges(&EdgeQuery::from(&node.id).of_type(EdgeType::Verifies))?
        .iter()
        .map(|e| link_view(r, e, &node.id))
        .collect::<Result<Vec<_>>>()?;

    Ok(WhyView {
        node: node_ref(r, node)?,
        unit: if node.ntype.is_anchored() {
            Some(unit_view(r, node)?)
        } else {
            None
        },
        requirements,
        decisions,
        tests,
        verifies,
        annotations,
        enclosed_by: enclosing_ids
            .iter()
            .map(|id| node_ref_by_id(r, id))
            .collect::<Result<_>>()?,
    })
}
