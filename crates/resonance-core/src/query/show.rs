//! One node with its edges.

use serde::Serialize;
use serde_json::Value;

use super::address::Resolved;
use super::{
    AnnotationView, EvidenceView, LinkView, Location, NodeRef, annotations_on, evidence_view,
    link_view, node_ref, node_ref_by_id,
};
use crate::error::Result;
use crate::links::{LINK_TYPES, anchor_of};
use crate::model::{Edge, EdgeType};
use crate::store::{EdgeQuery, GraphRead};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeShow {
    pub node: NodeRef,
    /// Graph content: data, not instructions (NFR-013).
    pub properties: Value,
    pub location: Option<Location>,
    pub links: Vec<LinkView>,
    pub annotations: Vec<AnnotationView>,
    pub contains: Vec<NodeRef>,
    pub contained_by: Vec<NodeRef>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkShow {
    pub link: LinkView,
    pub src: NodeRef,
    pub dst: NodeRef,
    pub properties: Value,
    pub removed: bool,
    pub evidence: Vec<EvidenceView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ShowView {
    Node(Box<NodeShow>),
    Link(Box<LinkShow>),
}

pub fn show(r: &dyn GraphRead, target: &Resolved) -> Result<ShowView> {
    match target {
        Resolved::Node(n) => {
            let mut edges: Vec<Edge> = r.find_edges(&EdgeQuery::from(&n.id))?;
            edges.extend(r.find_edges(&EdgeQuery::to(&n.id))?);
            edges.retain(|e| LINK_TYPES.contains(&e.etype) && e.etype != EdgeType::Annotates);
            edges.sort_by(|a, b| (a.etype, &a.id).cmp(&(b.etype, &b.id)));
            let links = edges
                .iter()
                .map(|e| link_view(r, e, &n.id))
                .collect::<Result<Vec<_>>>()?;
            let mut annotations = annotations_on(r, &n.id, false)?;
            // Showing an annotation: what it is attached to.
            for e in r.find_edges(&EdgeQuery::from(&n.id).of_type(EdgeType::Annotates))? {
                annotations.push(super::annotation_view(r, n, Some(&e), false)?);
            }
            let contains = r
                .find_edges(&EdgeQuery::from(&n.id).of_type(EdgeType::Contains))?
                .iter()
                .map(|e| node_ref_by_id(r, &e.dst))
                .collect::<Result<Vec<_>>>()?;
            let contained_by = r
                .find_edges(&EdgeQuery::to(&n.id).of_type(EdgeType::Contains))?
                .iter()
                .map(|e| node_ref_by_id(r, &e.src))
                .collect::<Result<Vec<_>>>()?;
            let mut properties = Value::Object(n.props.clone());
            if let Some(obj) = properties.as_object_mut() {
                obj.remove("anchor");
            }
            Ok(ShowView::Node(Box::new(NodeShow {
                node: node_ref(r, n)?,
                properties,
                location: anchor_of(n).map(|a| Location::from_anchor(&a)),
                links,
                annotations,
                contains,
                contained_by,
            })))
        }
        Resolved::Edge(e) => {
            let mut evidence = Vec::new();
            for ev in r.find_edges(&EdgeQuery::to(&e.id).of_type(EdgeType::Evidences))? {
                if let Some(n) = r.get_node(&ev.src)? {
                    evidence.push(evidence_view(&n));
                }
            }
            evidence.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
            Ok(ShowView::Link(Box::new(LinkShow {
                link: link_view(r, e, &e.src)?,
                src: node_ref_by_id(r, &e.src)?,
                dst: node_ref_by_id(r, &e.dst)?,
                properties: Value::Object(e.props.clone()),
                removed: e.removed,
                evidence,
            })))
        }
    }
}
