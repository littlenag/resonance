//! Bounded traversal over the store API (FR-162), with a generic breadth-first implementation.

use std::collections::{HashSet, VecDeque};

use super::{EdgeQuery, GraphRead};
use crate::error::Result;
use crate::model::{Edge, EdgeType, LinkStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Follow edges from source to destination.
    Out,
    /// Follow edges from destination to source.
    In,
    Both,
}

/// A depth-limited expansion from one node, filtered by edge type and status.
#[derive(Debug, Clone)]
pub struct Traversal {
    pub start: String,
    pub direction: Direction,
    /// Empty means every type.
    pub edge_types: Vec<EdgeType>,
    /// Empty means every status.
    pub statuses: Vec<LinkStatus>,
    pub max_depth: usize,
}

impl Traversal {
    pub fn new(start: &str, direction: Direction, max_depth: usize) -> Self {
        Traversal {
            start: start.to_string(),
            direction,
            edge_types: Vec::new(),
            statuses: Vec::new(),
            max_depth,
        }
    }
    pub fn types(mut self, types: &[EdgeType]) -> Self {
        self.edge_types = types.to_vec();
        self
    }
    pub fn statuses(mut self, statuses: &[LinkStatus]) -> Self {
        self.statuses = statuses.to_vec();
        self
    }
}

/// One step of a traversal: the edge followed and the node it reached, at a depth (1-based).
#[derive(Debug, Clone, PartialEq)]
pub struct Hop {
    pub depth: usize,
    pub edge: Edge,
    pub node: String,
}

/// Breadth-first expansion. Each node is reached once, at its smallest depth; removed edges
/// are never followed.
pub fn breadth_first<R: GraphRead + ?Sized>(store: &R, t: &Traversal) -> Result<Vec<Hop>> {
    let mut hops = Vec::new();
    let mut seen: HashSet<String> = HashSet::from([t.start.clone()]);
    let mut queue = VecDeque::from([(t.start.clone(), 0usize)]);
    while let Some((node, depth)) = queue.pop_front() {
        if depth >= t.max_depth {
            continue;
        }
        let mut edges = Vec::new();
        if matches!(t.direction, Direction::Out | Direction::Both) {
            edges.extend(store.find_edges(&EdgeQuery::from(&node))?);
        }
        if matches!(t.direction, Direction::In | Direction::Both) {
            edges.extend(store.find_edges(&EdgeQuery::to(&node))?);
        }
        edges.sort_by(|a, b| a.id.cmp(&b.id));
        for edge in edges {
            if !t.edge_types.is_empty() && !t.edge_types.contains(&edge.etype) {
                continue;
            }
            if !t.statuses.is_empty() && !t.statuses.contains(&edge.status) {
                continue;
            }
            let next = edge.other(&node).to_string();
            if !seen.insert(next.clone()) {
                continue;
            }
            queue.push_back((next.clone(), depth + 1));
            hops.push(Hop {
                depth: depth + 1,
                edge,
                node: next,
            });
        }
    }
    Ok(hops)
}
