//! Source tags (FR-123): with `ingest.sourceTags = true`, tagged comments such as
//! `@satisfies FR-042` or `@verifies FR-042` in any text file create links with provenance
//! `imported` from the innermost code unit containing the tag, or else from the file.
//! Finding a tag is a text match, not parsing (NG4). Removing a tag removes its link.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use crate::error::Result;
use crate::links::{self, anchor_of, new_link_props};
use crate::model::{Edge, EdgeType, Layer, LinkStatus, NodeType, ids};
use crate::ops::Tx;
use crate::store::{EdgeQuery, NodeQuery};

/// Metadata key: whether tags were enabled at the last reconcile.
pub const ENABLED: &str = "tags.enabled";

/// The tags in `text`: (byte offset, edge type, requirement key).
pub fn find(text: &str) -> Vec<(usize, EdgeType, String)> {
    let mut out = Vec::new();
    for (tag, etype) in [
        ("@satisfies", EdgeType::Satisfies),
        ("@verifies", EdgeType::Verifies),
    ] {
        let mut from = 0;
        while let Some(at) = text[from..].find(tag) {
            let start = from + at;
            let after = &text[start + tag.len()..];
            from = start + tag.len();
            if !after.starts_with([' ', '\t', ':']) {
                continue;
            }
            let rest = after.trim_start_matches([' ', '\t', ':']);
            // One or more keys, separated by commas or spaces.
            for word in rest
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|w| !w.is_empty())
            {
                let key = word.trim_end_matches(['.', ';', ')', '*']);
                if ids::is_requirement_key(key) {
                    out.push((start, etype, key.to_string()));
                } else {
                    break;
                }
            }
        }
    }
    out.sort();
    out
}

/// The deterministic ID of a tag link, so that clones agree.
fn tag_edge_id(etype: EdgeType, src: &str, dst: &str) -> String {
    format!("tag:{}:{src}>{dst}", etype.as_str())
}

/// Bring the tag links of `path` in line with `text` (`None` when the file is gone).
pub fn sync_file(tx: &mut Tx<'_>, path: &str, text: Option<&str>) -> Result<usize> {
    let file = ids::file_id(path);
    let mut units = Vec::new();
    for t in [NodeType::CodeUnit, NodeType::Test] {
        units.extend(
            tx.r()
                .find_nodes(&NodeQuery::of_type(t).with_prop("path", path))?,
        );
    }
    let mut desired: BTreeMap<String, (EdgeType, String, String)> = BTreeMap::new();
    if let Some(text) = text {
        for (offset, etype, key) in find(text) {
            let req = ids::requirement_id(&key);
            if tx.r().get_node(&req)?.is_none() {
                continue;
            }
            let innermost = units
                .iter()
                .filter(|u| links::is_live(u))
                .filter_map(|u| anchor_of(u).map(|a| (a, u)))
                .filter(|(a, _)| a.start_byte <= offset && offset < a.end_byte)
                .min_by_key(|(a, _)| a.end_byte - a.start_byte)
                .map(|(_, u)| u);
            let (src, src_type) = match innermost {
                Some(u) => (u.id.clone(), u.ntype),
                None => (file.clone(), NodeType::File),
            };
            if etype == EdgeType::Verifies && src_type != NodeType::Test {
                continue; // Only tests verify.
            }
            desired.insert(tag_edge_id(etype, &src, &req), (etype, src, req));
        }
    }
    let mut existing: BTreeSet<String> = BTreeSet::new();
    let sources: Vec<String> = std::iter::once(file.clone())
        .chain(units.iter().map(|u| u.id.clone()))
        .collect();
    for src in &sources {
        for e in tx.r().find_edges(&EdgeQuery::from(src))? {
            if e.str_prop("origin") == Some("tag") && e.str_prop("tagFile") == Some(path) {
                existing.insert(e.id.clone());
            }
        }
    }
    let mut changes = 0;
    for id in existing.iter().filter(|id| !desired.contains_key(*id)) {
        tx.remove_edge(id)?;
        changes += 1;
    }
    for (id, (etype, src, dst)) in &desired {
        if existing.contains(id) {
            continue;
        }
        let mut props = new_link_props(tx.r(), src, dst, "imported", "source tag", None)?;
        props.insert("origin".into(), json!("tag"));
        props.insert("tagFile".into(), json!(path));
        tx.add_edge(&Edge {
            id: id.clone(),
            etype: *etype,
            src: src.clone(),
            dst: dst.clone(),
            layer: Layer::Authored,
            status: LinkStatus::Asserted,
            removed: false,
            props,
        })?;
        changes += 1;
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tags_and_keys() {
        let text = "// @satisfies FR-042, FR-043\nfn x() {}\n# @verifies: NFR-7.\n@satisfiesFR-1\n";
        let found: Vec<(EdgeType, String)> =
            find(text).into_iter().map(|(_, e, k)| (e, k)).collect();
        assert_eq!(
            found,
            [
                (EdgeType::Satisfies, "FR-042".to_string()),
                (EdgeType::Satisfies, "FR-043".to_string()),
                (EdgeType::Verifies, "NFR-7".to_string()),
            ]
        );
    }
}
