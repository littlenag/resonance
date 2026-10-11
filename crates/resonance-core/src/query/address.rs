//! Resolves node references: ULID, requirement key, `path::name`, unit name or file path.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Error, Result};
use crate::model::{Edge, Node, NodeType, ids};
use crate::repo::Repo;
use crate::store::{EdgeQuery, GraphRead, NodeQuery};

use super::label_of;

/// What a reference resolved to.
#[derive(Debug, Clone)]
pub enum Resolved {
    Node(Node),
    Edge(Edge),
}

/// Minimum length of an ID prefix.
const MIN_PREFIX: usize = 6;

fn looks_like_ulid_prefix(s: &str) -> bool {
    s.len() >= MIN_PREFIX
        && s.len() <= 26
        && s.chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
}

/// Resolve a reference typed by a person or agent (mvp.md §3.9, "Node addressing").
///
/// Exact forms win outright: a node or edge ID, a requirement key, a unit address
/// (`path::name`, `path::name#2`). Otherwise every looser reading is tried (a file path, a
/// bare unit name, an ID prefix, a decision title); one match resolves, several are
/// reported as ambiguous with the candidates listed.
pub fn resolve(r: &dyn GraphRead, repo: &Repo, cwd: &Path, reference: &str) -> Result<Resolved> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(Error::Usage("an empty reference".into()));
    }
    if let Some(n) = r.get_node(reference)? {
        return Ok(Resolved::Node(n));
    }
    if let Some(e) = r.get_edge(reference)? {
        return Ok(Resolved::Edge(e));
    }
    if ids::is_requirement_key(reference)
        && let Some(n) = r.get_node(&ids::requirement_id(reference))?
    {
        return Ok(Resolved::Node(n));
    }
    if let Some((path, name)) = reference.rsplit_once("::") {
        return resolve_address(r, repo, cwd, reference, path, name).map(Resolved::Node);
    }

    let mut candidates: BTreeMap<String, Resolved> = BTreeMap::new();
    // A file path, relative to the repository root or the current directory.
    let mut paths = vec![reference.trim_start_matches("./").to_string()];
    if let Ok(p) = repo.relative_path(cwd, reference) {
        paths.push(p);
    }
    for p in paths {
        if let Some(n) = r.get_node(&ids::file_id(&p))?
            && !n.retired
        {
            candidates.insert(n.id.clone(), Resolved::Node(n));
        }
    }
    // A bare unit or test name.
    for t in [NodeType::CodeUnit, NodeType::Test] {
        for n in r.find_nodes(&NodeQuery::of_type(t).with_prop("name", reference))? {
            candidates.insert(n.id.clone(), Resolved::Node(n));
        }
    }
    // A decision title.
    for n in r.find_nodes(&NodeQuery::of_type(NodeType::Decision))? {
        if n.str_prop("title")
            .is_some_and(|t| t.eq_ignore_ascii_case(reference))
        {
            candidates.insert(n.id.clone(), Resolved::Node(n));
        }
    }
    // An ID prefix (like a short git hash).
    if looks_like_ulid_prefix(reference) {
        for t in [
            NodeType::Decision,
            NodeType::CodeUnit,
            NodeType::Test,
            NodeType::Annotation,
            NodeType::Evidence,
        ] {
            for n in r.find_nodes(&NodeQuery::of_type(t).including_retired())? {
                if n.id.starts_with(reference) {
                    candidates.insert(n.id.clone(), Resolved::Node(n));
                }
            }
        }
        for e in r.find_edges(&EdgeQuery::all().including_removed())? {
            if e.id.starts_with(reference) {
                candidates.insert(e.id.clone(), Resolved::Edge(e));
            }
        }
    }

    match candidates.len() {
        0 => Err(Error::NotFound(format!(
            "`{reference}` (expected an ID, a requirement key such as FR-042, a unit address such as src/lib.rs::open, a unit name, or a file path)"
        ))),
        1 => Ok(candidates.into_values().next().expect("one candidate")),
        _ => Err(Error::Ambiguous {
            reference: reference.to_string(),
            candidates: candidates
                .values()
                .map(|c| describe(r, c))
                .collect::<Result<_>>()?,
        }),
    }
}

fn describe(r: &dyn GraphRead, c: &Resolved) -> Result<String> {
    Ok(match c {
        Resolved::Node(n) => format!("{} {} ({})", n.ntype, label_of(r, n)?, n.id),
        Resolved::Edge(e) => format!("{} link {} -> {} ({})", e.etype, e.src, e.dst, e.id),
    })
}

fn resolve_address(
    r: &dyn GraphRead,
    repo: &Repo,
    cwd: &Path,
    reference: &str,
    path: &str,
    name: &str,
) -> Result<Node> {
    let (name, ordinal) = match name.rsplit_once('#') {
        Some((n, k)) if k.parse::<usize>().is_ok() => (n, k.parse::<usize>().unwrap_or(1)),
        _ => (name, 1),
    };
    let mut paths = vec![path.trim_start_matches("./").to_string()];
    if let Ok(p) = repo.relative_path(cwd, path) {
        paths.push(p);
    }
    paths.dedup();
    for p in paths {
        let mut same: Vec<Node> = Vec::new();
        for t in [NodeType::CodeUnit, NodeType::Test] {
            same.extend(
                r.find_nodes(&NodeQuery::of_type(t).with_prop("path", p.as_str()))?
                    .into_iter()
                    .filter(|n| n.str_prop("name") == Some(name)),
            );
        }
        same.sort_by(|a, b| a.id.cmp(&b.id));
        if let Some(n) = ordinal.checked_sub(1).and_then(|i| same.into_iter().nth(i)) {
            return Ok(n);
        }
    }
    Err(Error::NotFound(format!("the unit `{reference}`")))
}

/// Resolve to a node; an edge is an error.
pub fn resolve_node(r: &dyn GraphRead, repo: &Repo, cwd: &Path, reference: &str) -> Result<Node> {
    match resolve(r, repo, cwd, reference)? {
        Resolved::Node(n) => Ok(n),
        Resolved::Edge(e) => Err(Error::Invalid(format!(
            "`{reference}` is a link ({} {}); a node is needed here",
            e.etype, e.id
        ))),
    }
}

/// Resolve to an edge; a node is an error.
pub fn resolve_edge(r: &dyn GraphRead, repo: &Repo, cwd: &Path, reference: &str) -> Result<Edge> {
    match resolve(r, repo, cwd, reference)? {
        Resolved::Edge(e) => Ok(e),
        Resolved::Node(n) => Err(Error::Invalid(format!(
            "`{reference}` is a {} ({}); a link ID is needed here (see `resonance show`)",
            n.ntype, n.id
        ))),
    }
}
