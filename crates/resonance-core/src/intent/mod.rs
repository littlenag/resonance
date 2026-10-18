//! Intent: importing requirements from Markdown and keeping them in sync (MVP-011, FR-122).
//! Authoring requirements, decisions and annotations is in [`crate::api`] (FR-120, FR-123).

pub mod formats;
pub mod markdown;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::Result;
use crate::model::{Layer, NodeType, Props, content_hash, ids};
use crate::ops::Tx;
use crate::repo::abs_path;
use crate::store::{GraphRead, NodeQuery};

/// Metadata keys used by intent import.
pub mod meta {
    pub const FINGERPRINT: &str = "intent.fingerprint";
    pub const UNREADABLE: &str = "intent.unreadable";
    pub const CONFLICTS: &str = "intent.conflicts";
}

/// An intent source that could not be read (FR-013). Its requirements are kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnreadableSource {
    pub path: String,
    pub reason: String,
}

/// A key that a document defines but that was authored through the CLI or MCP first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyConflict {
    pub key: String,
    pub path: String,
    pub line: usize,
}

/// What an import changed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentReport {
    /// Whether the sources were read at all (they are skipped when nothing changed).
    pub scanned: bool,
    pub added: Vec<String>,
    /// Requirements whose text changed; their links become stale (S7).
    pub changed: Vec<String>,
    pub retired: Vec<String>,
    pub reactivated: Vec<String>,
    pub unreadable: Vec<UnreadableSource>,
    pub conflicts: Vec<KeyConflict>,
    /// Statements that look like intent but have no stable identifier (FR-122).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unidentified: Vec<formats::Unidentified>,
    /// Items defined in the sources, after the import.
    pub total: usize,
    /// Node IDs whose content changed (their links become stale). Not serialized.
    #[serde(skip)]
    pub changed_ids: Vec<String>,
}

/// The kind of a requirement, from its key prefix.
pub fn kind_of(key: &str) -> &'static str {
    if key.starts_with("NFR-") {
        "non-functional"
    } else {
        "functional"
    }
}

/// The node ID of an imported item: deterministic, so that clones agree (FR-031, FR-186).
pub fn item_id(ntype: NodeType, key: &str) -> String {
    match ntype {
        NodeType::Decision => format!("dec:{key}"),
        NodeType::Design => format!("design:{key}"),
        NodeType::Constraint => format!("constraint:{key}"),
        _ => ids::requirement_id(key),
    }
}

/// The node types that sources can define.
const IMPORTED: [NodeType; 3] = [NodeType::Requirement, NodeType::Decision, NodeType::Design];

/// Bring imported requirements in line with the configured sources. Unless `force`, does
/// nothing when no source changed since the last import.
pub fn sync(tx: &mut Tx<'_>, root: &Path, sources: &[String], force: bool) -> Result<IntentReport> {
    let mut report = IntentReport::default();
    let mut parsed: Vec<formats::Parsed> = Vec::new();
    let mut fingerprint = String::new();
    for entry in sources {
        let (explicit, path) = formats::split_source(entry);
        let abs = abs_path(root, path);
        if abs.is_dir() {
            let p = formats::parse_adr_dir(root, path);
            fingerprint.push_str(&format!("{entry}=[{}];", p.fingerprint));
            parsed.push(p);
            continue;
        }
        let format = explicit.unwrap_or_else(|| formats::detect(path, false));
        match fs::read(&abs) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => {
                    fingerprint.push_str(&format!("{entry}={};", content_hash(text.as_bytes())));
                    parsed.push(formats::parse_text(format, path, &text));
                }
                Err(_) => report.unreadable.push(UnreadableSource {
                    path: path.to_string(),
                    reason: "not valid UTF-8".into(),
                }),
            },
            Err(e) => report.unreadable.push(UnreadableSource {
                path: path.to_string(),
                reason: if e.kind() == std::io::ErrorKind::NotFound {
                    "file not found".into()
                } else {
                    e.to_string()
                },
            }),
        }
    }
    for u in &report.unreadable {
        fingerprint.push_str(&format!("{}=unreadable;", u.path));
    }
    let previous = tx.r().get_meta(meta::FINGERPRINT)?;
    if !force && previous.as_deref() == Some(fingerprint.as_str()) {
        return Ok(report);
    }
    report.scanned = true;

    // The first definition of each item wins: sources in configured order, then by line.
    let mut defs: BTreeMap<String, formats::Item> = BTreeMap::new();
    let mut also: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for p in parsed {
        report.unidentified.extend(p.unidentified);
        for item in p.items {
            let id = item_id(item.ntype, &item.key);
            if defs.contains_key(&id) {
                also.entry(id)
                    .or_default()
                    .push(json!({"path": item.path, "line": item.line}));
                continue;
            }
            defs.insert(id, item);
        }
    }
    let unreadable_paths: BTreeSet<&str> =
        report.unreadable.iter().map(|u| u.path.as_str()).collect();

    for (id, def) in &defs {
        let source = json!({"path": def.path, "line": def.line});
        let also_defined = Value::Array(also.get(id).cloned().unwrap_or_default());
        match tx.r().get_node(id)? {
            None => {
                let mut p = Props::new();
                p.insert("key".into(), json!(def.key));
                for (k, v) in &def.fields {
                    p.insert(k.clone(), v.clone());
                }
                if def.ntype == NodeType::Requirement {
                    p.insert("kind".into(), json!(kind_of(&def.key)));
                    p.insert("specStatus".into(), json!(def.spec_status));
                    p.insert("status".into(), json!("active"));
                } else if !p.contains_key("status") {
                    p.insert("status".into(), json!("active"));
                }
                p.insert("origin".into(), json!("import"));
                p.insert("source".into(), source);
                p.insert("alsoDefinedIn".into(), also_defined);
                tx.create_node(id, def.ntype, Layer::Authored, p)?;
                report.added.push(def.key.clone());
            }
            Some(node) if node.str_prop("origin") != Some("import") => {
                report.conflicts.push(KeyConflict {
                    key: def.key.clone(),
                    path: def.path.clone(),
                    line: def.line,
                });
            }
            Some(node) => {
                let mut content_changed = false;
                for (k, v) in &def.fields {
                    if k == "status" {
                        continue;
                    }
                    if node.props.get(k) != Some(v) {
                        tx.set_prop(id, k, v.clone())?;
                        content_changed = true;
                    }
                }
                if content_changed {
                    report.changed.push(def.key.clone());
                    report.changed_ids.push(id.clone());
                }
                let mut updates = vec![("source", source), ("alsoDefinedIn", also_defined)];
                if def.ntype == NodeType::Requirement {
                    updates.push(("specStatus", json!(def.spec_status)));
                }
                if let Some(st) = def.fields.get("status")
                    && node.str_prop("status") != st.as_str()
                    && node.str_prop("status") != Some("retired")
                {
                    updates.push(("status", st.clone()));
                }
                for (k, v) in updates {
                    if node.props.get(k) != Some(&v) {
                        tx.set_prop(id, k, v)?;
                    }
                }
                if node.str_prop("status") == Some("retired") {
                    let back = def
                        .fields
                        .get("status")
                        .cloned()
                        .unwrap_or_else(|| json!("active"));
                    tx.set_prop(id, "status", back)?;
                    report.reactivated.push(def.key.clone());
                }
            }
        }
    }

    // Requirements that disappeared from every readable source are retired, never deleted.
    let mut imported = Vec::new();
    for t in IMPORTED {
        imported.extend(
            tx.r()
                .find_nodes(&NodeQuery::of_type(t).with_prop("origin", "import"))?,
        );
    }
    for node in imported {
        let key = node.str_prop("key").unwrap_or_default().to_string();
        if defs.contains_key(&node.id) || node.str_prop("status") == Some("retired") {
            continue;
        }
        let from = node
            .props
            .get("source")
            .and_then(|s| s.get("path"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if unreadable_paths.contains(from) {
            continue; // FR-013: an unreadable source keeps what it defined.
        }
        tx.set_prop(&node.id, "status", json!("retired"))?;
        report.retired.push(key);
    }

    report.total = defs.len();
    let w = tx.w();
    w.set_meta(meta::FINGERPRINT, &fingerprint)?;
    w.set_meta(
        meta::UNREADABLE,
        &serde_json::to_string(&report.unreadable)?,
    )?;
    w.set_meta(meta::CONFLICTS, &serde_json::to_string(&report.conflicts)?)?;
    Ok(report)
}

/// Sources that could not be read at the last import (for the repair list).
pub fn unreadable_sources(r: &dyn GraphRead) -> Result<Vec<UnreadableSource>> {
    Ok(match r.get_meta(meta::UNREADABLE)? {
        Some(s) => serde_json::from_str(&s).unwrap_or_default(),
        None => Vec::new(),
    })
}

/// Keys defined in a document that were already authored elsewhere (for the repair list).
pub fn key_conflicts(r: &dyn GraphRead) -> Result<Vec<KeyConflict>> {
    Ok(match r.get_meta(meta::CONFLICTS)? {
        Some(s) => serde_json::from_str(&s).unwrap_or_default(),
        None => Vec::new(),
    })
}
