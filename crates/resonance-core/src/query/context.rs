//! Context packs for agent tasks (FR-133).
//!
//! A pack contains the target requirements; the decisions that constrain them or their
//! linked code; the linked code units as locations (not full text); the linked tests with
//! their latest evidence; and the annotations in scope, including call-outs for the task.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::trace::DecisionLink;
use super::{
    AnnotationView, EvidenceView, LinkStatus, RequirementView, UnitView, annotation_view,
    annotations_on, decision_view, enclosing, latest_evidence, link_view, requirement_view,
    unit_view,
};
use crate::error::{Error, Result};
use crate::model::{EdgeType, Node, NodeType};
use crate::store::{EdgeQuery, GraphRead, NodeQuery};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackUnit {
    #[serde(flatten)]
    pub unit: UnitView,
    /// The requirements in this pack it is linked to, and how.
    pub links: Vec<PackLink>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackLink {
    pub requirement: String,
    #[serde(rename = "type")]
    pub etype: EdgeType,
    pub status: LinkStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackTest {
    #[serde(flatten)]
    pub unit: UnitView,
    pub links: Vec<PackLink>,
    pub latest_evidence: Option<EvidenceView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPack {
    pub task: Option<String>,
    pub requirements: Vec<RequirementView>,
    pub decisions: Vec<DecisionLink>,
    pub code_units: Vec<PackUnit>,
    pub tests: Vec<PackTest>,
    pub annotations: Vec<AnnotationView>,
    /// Call-outs made for this task, wherever they are attached.
    pub call_outs: Vec<AnnotationView>,
}

/// Build a context pack for `targets` (requirements) and an optional task label (FR-133).
pub fn context(r: &dyn GraphRead, targets: &[Node], task: Option<&str>) -> Result<ContextPack> {
    let mut requirements = Vec::new();
    let mut units: BTreeMap<String, (Node, Vec<PackLink>)> = BTreeMap::new();
    let mut tests: BTreeMap<String, (Node, Vec<PackLink>, Option<String>)> = BTreeMap::new();
    let mut decision_scopes: Vec<String> = Vec::new();

    for req in targets {
        if req.ntype != NodeType::Requirement {
            return Err(Error::Invalid(format!(
                "a context pack starts from requirements; {} is a {}",
                req.id, req.ntype
            )));
        }
        requirements.push(requirement_view(req));
        decision_scopes.push(req.id.clone());
        let key = req.str_prop("key").unwrap_or_default().to_string();
        for e in r.find_edges(&EdgeQuery::to(&req.id))? {
            let Some(src) = r.get_node(&e.src)? else {
                continue;
            };
            let link = PackLink {
                requirement: key.clone(),
                etype: e.etype,
                status: e.status,
            };
            match (e.etype, src.ntype) {
                (EdgeType::Verifies, NodeType::Test) => {
                    let entry = tests
                        .entry(src.id.clone())
                        .or_insert_with(|| (src.clone(), Vec::new(), None));
                    entry.1.push(link);
                    entry.2.get_or_insert(e.id.clone());
                }
                (EdgeType::Satisfies, t) if t.is_anchored() || t == NodeType::File => {
                    units
                        .entry(src.id.clone())
                        .or_insert_with(|| (src.clone(), Vec::new()))
                        .1
                        .push(link);
                }
                _ => {}
            }
        }
    }

    // Decisions on the requirements or their linked code (and what encloses that code).
    let mut annotation_scopes: Vec<(String, bool)> = Vec::new();
    for (id, _) in units.values() {
        decision_scopes.push(id.id.clone());
        annotation_scopes.push((id.id.clone(), false));
        for enc in enclosing(r, &id.id)? {
            decision_scopes.push(enc.clone());
            annotation_scopes.push((enc, true));
        }
    }
    for (t, _, _) in tests.values() {
        annotation_scopes.push((t.id.clone(), false));
    }
    let mut seen = BTreeSet::new();
    let mut decisions = Vec::new();
    for scope in &decision_scopes {
        for e in r.find_edges(&EdgeQuery::to(scope).of_type(EdgeType::Constrains))? {
            if !seen.insert(e.id.clone()) {
                continue;
            }
            if let Some(d) = r.get_node(&e.src)? {
                decisions.push(DecisionLink {
                    link: link_view(r, &e, scope)?,
                    decision: decision_view(&d),
                });
            }
        }
    }

    let mut annotations = Vec::new();
    let mut seen_annotations = BTreeSet::new();
    for req in targets {
        for a in annotations_on(r, &req.id, false)? {
            if seen_annotations.insert(a.id.clone()) {
                annotations.push(a);
            }
        }
    }
    for (scope, inherited) in &annotation_scopes {
        for a in annotations_on(r, scope, *inherited)? {
            if seen_annotations.insert(a.id.clone()) {
                annotations.push(a);
            }
        }
    }

    let mut call_outs = Vec::new();
    if let Some(task) = task {
        for n in r.find_nodes(&NodeQuery::of_type(NodeType::Annotation).with_prop("task", task))? {
            let edge = r
                .find_edges(&EdgeQuery::from(&n.id).of_type(EdgeType::Annotates))?
                .into_iter()
                .next();
            call_outs.push(annotation_view(r, &n, edge.as_ref(), false)?);
        }
    }

    let code_units = units
        .values()
        .map(|(n, links)| {
            Ok(PackUnit {
                unit: unit_view(r, n)?,
                links: links.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let tests = tests
        .values()
        .map(|(n, links, edge)| {
            let mut latest = None;
            for e in r.find_edges(&EdgeQuery::from(&n.id).of_type(EdgeType::Verifies))? {
                if let Some(ev) = latest_evidence(r, &e.id)?
                    && latest
                        .as_ref()
                        .is_none_or(|l: &EvidenceView| ev.timestamp > l.timestamp)
                {
                    latest = Some(ev);
                }
            }
            let _ = edge;
            Ok(PackTest {
                unit: unit_view(r, n)?,
                links: links.clone(),
                latest_evidence: latest,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ContextPack {
        task: task.map(String::from),
        requirements,
        decisions,
        code_units,
        tests,
        annotations,
        call_outs,
    })
}
