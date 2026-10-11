//! Intent: importing requirements from Markdown and keeping them in sync (MVP-011, FR-122).
//! Authoring requirements, decisions and annotations is in [`crate::api`] (FR-120, FR-123).

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
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
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
    /// Requirements defined in the sources, after the import.
    pub total: usize,
}

/// The kind of a requirement, from its key prefix.
pub fn kind_of(key: &str) -> &'static str {
    if key.starts_with("NFR-") {
        "non-functional"
    } else {
        "functional"
    }
}

struct Definition {
    key: String,
    text: String,
    spec_status: Option<String>,
    path: String,
    line: usize,
}

/// Bring imported requirements in line with the configured sources. Unless `force`, does
/// nothing when no source changed since the last import.
pub fn sync(tx: &mut Tx<'_>, root: &Path, sources: &[String], force: bool) -> Result<IntentReport> {
    let mut report = IntentReport::default();
    let mut readable: Vec<(String, String)> = Vec::new();
    let mut fingerprint = String::new();
    for path in sources {
        match fs::read(abs_path(root, path)) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => {
                    fingerprint.push_str(&format!("{path}={};", content_hash(text.as_bytes())));
                    readable.push((path.clone(), text));
                }
                Err(_) => report.unreadable.push(UnreadableSource {
                    path: path.clone(),
                    reason: "not valid UTF-8".into(),
                }),
            },
            Err(e) => report.unreadable.push(UnreadableSource {
                path: path.clone(),
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

    // The first definition of each key wins: sources in configured order, then by line.
    let mut defs: BTreeMap<String, Definition> = BTreeMap::new();
    let mut also: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for (path, text) in &readable {
        for r in markdown::parse(text) {
            if defs.contains_key(&r.key) {
                also.entry(r.key.clone())
                    .or_default()
                    .push(json!({"path": path, "line": r.line}));
                continue;
            }
            defs.insert(
                r.key.clone(),
                Definition {
                    key: r.key,
                    text: r.text,
                    spec_status: r.spec_status,
                    path: path.clone(),
                    line: r.line,
                },
            );
        }
    }
    let unreadable_paths: BTreeSet<&str> =
        report.unreadable.iter().map(|u| u.path.as_str()).collect();

    for def in defs.values() {
        let id = ids::requirement_id(&def.key);
        let source = json!({"path": def.path, "line": def.line});
        let also_defined = Value::Array(also.get(&def.key).cloned().unwrap_or_default());
        match tx.r().get_node(&id)? {
            None => {
                let mut p = Props::new();
                p.insert("key".into(), json!(def.key));
                p.insert("text".into(), json!(def.text));
                p.insert("kind".into(), json!(kind_of(&def.key)));
                p.insert("specStatus".into(), json!(def.spec_status));
                p.insert("status".into(), json!("active"));
                p.insert("origin".into(), json!("import"));
                p.insert("source".into(), source);
                p.insert("alsoDefinedIn".into(), also_defined);
                tx.create_node(&id, NodeType::Requirement, Layer::Authored, p)?;
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
                if node.str_prop("text") != Some(def.text.as_str()) {
                    tx.set_prop(&id, "text", json!(def.text))?;
                    report.changed.push(def.key.clone());
                }
                let updates = [
                    ("specStatus", json!(def.spec_status)),
                    ("source", source),
                    ("alsoDefinedIn", also_defined),
                ];
                for (k, v) in updates {
                    if node.props.get(k) != Some(&v) {
                        tx.set_prop(&id, k, v)?;
                    }
                }
                if node.str_prop("status") == Some("retired") {
                    tx.set_prop(&id, "status", json!("active"))?;
                    report.reactivated.push(def.key.clone());
                }
            }
        }
    }

    // Requirements that disappeared from every readable source are retired, never deleted.
    let imported = tx
        .r()
        .find_nodes(&NodeQuery::of_type(NodeType::Requirement).with_prop("origin", "import"))?;
    for node in imported {
        let key = node.str_prop("key").unwrap_or_default().to_string();
        if defs.contains_key(&key) || node.str_prop("status") == Some("retired") {
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
