//! The change log (FR-274–FR-278, MVP-040): a concise, local log of people's changes to spec
//! items, which agents read from a remembered sequence number and clear as they handle them.
//!
//! Entries live in the live store's metadata (`change:<seq>`), never in the snapshot or the
//! deltas. Sequence numbers increase per working tree and are never reused; a newer change to
//! an item with a pending entry replaces that entry with a new one, so there is at most one
//! entry per item (FR-275). Removing an entry deletes it from the log only: the operations stay
//! in the operation log (FR-276).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::model::NodeType;
use crate::ops::{ActorKind, Op, kind};
use crate::repo::Repo;
use crate::store::{GraphRead, GraphScope, GraphStore};

/// Metadata key prefix of change-log entries: `change:<seq, zero-padded>`.
pub const PREFIX: &str = "change:";
/// The last sequence number handed out.
const SEQ: &str = "changes.seq";

/// Where a change came from (MVP-040).
pub mod origin {
    pub const CLI: &str = "cli";
    pub const TUI: &str = "tui";
    pub const MCP: &str = "mcp";
    pub const IMPORT: &str = "import";
    pub const GIT: &str = "git";
}

/// The item an entry is about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRef {
    pub id: String,
    pub key: String,
    #[serde(rename = "type")]
    pub ntype: String,
    /// At most 60 characters.
    pub title: String,
}

/// One change-log entry (FR-275).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub seq: u64,
    pub at: String,
    pub actor: String,
    pub item: ItemRef,
    /// `created`, `edited`, `retired` or `superseded`.
    pub kind: String,
    pub fields: Vec<String>,
    /// One line, at most 100 characters.
    pub summary: String,
    /// `tui`, `cli`, `mcp`, `import` or `git`.
    pub origin: String,
    pub op_ids: Vec<String>,
    /// The item's fields before the change (absent for a creation); `changes show` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<BTreeMap<String, Value>>,
    /// The item's fields after the change; `changes show` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<BTreeMap<String, Value>>,
}

impl Entry {
    /// The entry without its before and after, for lists.
    pub fn brief(&self) -> Entry {
        Entry {
            before: None,
            after: None,
            ..self.clone()
        }
    }
}

fn key_of(seq: u64) -> String {
    format!("{PREFIX}{seq:012}")
}

/// Pending entries, oldest first, after `since` when given.
pub fn list(r: &dyn GraphRead, since: Option<u64>) -> Result<Vec<Entry>> {
    let mut out: Vec<Entry> = r
        .meta_prefix(PREFIX)?
        .into_iter()
        .filter_map(|(_, v)| serde_json::from_str::<Entry>(&v).ok())
        .filter(|e| since.is_none_or(|s| e.seq > s))
        .collect();
    out.sort_by_key(|e| e.seq);
    Ok(out)
}

/// One entry, in full.
pub fn get(r: &dyn GraphRead, seq: u64) -> Result<Entry> {
    r.get_meta(&key_of(seq))?
        .and_then(|v| serde_json::from_str(&v).ok())
        .ok_or_else(|| {
            Error::NotFound(format!(
                "change-log entry {seq} (it may have been handled already)"
            ))
        })
}

/// The number of pending entries.
pub fn pending_count(r: &dyn GraphRead) -> Result<usize> {
    Ok(r.meta_prefix(PREFIX)?.len())
}

/// What `changes done` removed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    pub removed: Vec<u64>,
    /// Sequence numbers that were not pending.
    pub unknown: Vec<u64>,
    pub remaining: usize,
}

/// Remove handled entries (FR-276). The note, if any, goes to the local journal with them.
pub fn done(
    repo: &Repo,
    store: &mut dyn GraphStore,
    seqs: &[u64],
    note: Option<&str>,
    by: &str,
) -> Result<Done> {
    let mut out = Done::default();
    let mut gone = Vec::new();
    for &seq in seqs {
        match get(&*store, seq) {
            Ok(e) => {
                gone.push(e);
                out.removed.push(seq);
            }
            Err(_) => out.unknown.push(seq),
        }
    }
    store.write(GraphScope::Primary, &mut |w| {
        for e in &gone {
            w.delete_meta(&key_of(e.seq))?;
        }
        Ok(())
    })?;
    if !gone.is_empty() {
        let dir = repo.journal_dir();
        fs::create_dir_all(&dir).map_err(|e| Error::io("creating the journal", e))?;
        let path = dir.join("changes-done.jsonl");
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| Error::io(format!("opening {}", path.display()), e))?;
        for e in &gone {
            let line = serde_json::json!({
                "seq": e.seq, "item": e.item.key, "doneAt": crate::history::now(), "by": by, "note": note,
            });
            writeln!(f, "{line}").map_err(|e| Error::io("writing the journal", e))?;
        }
    }
    out.remaining = pending_count(&*store)?;
    Ok(out)
}

/// Whether changes by this actor, from this origin, are logged (MVP-040).
pub fn logged(actor: ActorKind, origin: &str, include_agents: bool) -> bool {
    match origin {
        origin::IMPORT | origin::GIT => true,
        _ => match actor {
            ActorKind::Human => true,
            ActorKind::Agent => include_agents,
            ActorKind::Tool => false,
        },
    }
}

/// The node an operation changes, for node operations.
fn node_of(op: &Op) -> Option<&str> {
    match op.kind.as_str() {
        kind::NODE_CREATE | kind::NODE_SET | kind::NODE_UNSET | kind::NODE_RETIRE => {
            op.payload["id"].as_str()
        }
        _ => None,
    }
}

/// An item's fields, folded from its operations.
fn fold_props(state: &mut Option<BTreeMap<String, Value>>, op: &Op) {
    let p = &op.payload;
    match op.kind.as_str() {
        kind::NODE_CREATE => {
            let mut props: BTreeMap<String, Value> = p["props"]
                .as_object()
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            props.retain(|_, v| !v.is_null());
            *state = Some(props);
        }
        kind::NODE_SET => {
            if let (Some(s), Some(k)) = (state.as_mut(), p["key"].as_str()) {
                if p["value"].is_null() {
                    s.remove(k);
                } else {
                    s.insert(k.to_string(), p["value"].clone());
                }
            }
        }
        kind::NODE_UNSET => {
            if let (Some(s), Some(k)) = (state.as_mut(), p["key"].as_str()) {
                s.remove(k);
            }
        }
        kind::NODE_RETIRE => {
            if let Some(s) = state.as_mut() {
                s.insert("status".into(), Value::String("retired".into()));
            }
        }
        _ => {}
    }
}

fn clip(s: &str, max: usize) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        let mut t: String = one_line.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

/// Record entries for the spec items `ops` changed, when their actor and origin are logged
/// (MVP-040). Returns the new sequence numbers.
pub fn record(
    store: &mut dyn GraphStore,
    ops: &[Op],
    origin: &str,
    include_agents: bool,
) -> Result<Vec<u64>> {
    // Operations per spec item, in order.
    let mut per_item: BTreeMap<String, Vec<&Op>> = BTreeMap::new();
    for op in ops {
        if !logged(op.actor.kind, origin, include_agents) {
            continue;
        }
        let Some(id) = node_of(op) else { continue };
        let ntype = match op.kind.as_str() {
            kind::NODE_CREATE => op.payload["type"]
                .as_str()
                .and_then(|t| t.parse::<NodeType>().ok()),
            _ => store.get_node(id)?.map(|n| n.ntype),
        };
        if ntype.is_some_and(|t| NodeType::SPEC_ITEMS.contains(&t)) {
            per_item.entry(id.to_string()).or_default().push(op);
        }
    }
    if per_item.is_empty() {
        return Ok(Vec::new());
    }
    // Before: the fold of each item's earlier operations, from the operation log.
    let batch: BTreeSet<&str> = per_item
        .values()
        .flatten()
        .map(|o| o.op_id.as_str())
        .collect();
    let mut before: BTreeMap<String, Option<BTreeMap<String, Value>>> =
        per_item.keys().map(|k| (k.clone(), None)).collect();
    for rec in store.ops_after(0, usize::MAX)? {
        if batch.contains(rec.op.op_id.as_str()) {
            continue;
        }
        if let Some(id) = node_of(&rec.op)
            && let Some(state) = before.get_mut(id)
        {
            fold_props(state, &rec.op);
        }
    }
    let pending = list(&*store, None)?;
    let mut seq: u64 = store
        .get_meta(SEQ)?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut writes: Vec<(String, String)> = Vec::new();
    let mut deletes: Vec<String> = Vec::new();
    let mut made = Vec::new();
    for (id, item_ops) in &per_item {
        let Some(node) = store.get_node(id)? else {
            continue;
        };
        let after: BTreeMap<String, Value> = node
            .props
            .iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let prior = pending.iter().find(|e| &e.item.id == id);
        let mut before_props = before.get(id).cloned().flatten();
        let created = item_ops.iter().any(|o| o.kind == kind::NODE_CREATE);
        let mut fields: BTreeSet<String> = BTreeSet::new();
        let mut op_ids: Vec<String> = Vec::new();
        let mut kind_of = if created { "created" } else { "edited" }.to_string();
        if let Some(p) = prior {
            // Replace the pending entry: keep what the agent last saw as "before".
            before_props = p.before.clone();
            fields.extend(p.fields.iter().cloned());
            op_ids.extend(p.op_ids.iter().cloned());
            if p.kind == "created" {
                kind_of = "created".into();
            }
            deletes.push(key_of(p.seq));
        }
        for op in item_ops {
            op_ids.push(op.op_id.clone());
            match op.kind.as_str() {
                kind::NODE_CREATE => {}
                kind::NODE_RETIRE => {
                    fields.insert("status".into());
                }
                _ => {
                    if let Some(k) = op.payload["key"].as_str() {
                        fields.insert(k.to_string());
                    }
                }
            }
        }
        let status_now = after
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let status_before = before_props
            .as_ref()
            .and_then(|b| b.get("status"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if kind_of != "created" && status_now != status_before {
            match status_now {
                "retired" => kind_of = "retired".into(),
                "superseded" => kind_of = "superseded".into(),
                _ => {}
            }
        }
        if !created && prior.is_none() && before_props.as_ref() == Some(&after) {
            continue; // Nothing visible changed.
        }
        let key = node
            .str_prop("key")
            .map(String::from)
            .unwrap_or_else(|| id.clone());
        let title_src = node
            .str_prop("title")
            .or(node.str_prop("text"))
            .or(node.str_prop("name"))
            .unwrap_or_default();
        let title = clip(title_src, 60);
        let actor = item_ops
            .last()
            .map(|o| o.actor.to_string())
            .unwrap_or_default();
        let fields: Vec<String> = fields.into_iter().collect();
        let summary = match kind_of.as_str() {
            "edited" => clip(
                &format!("{key} edited ({}): {title}", fields.join(", ")),
                100,
            ),
            k => clip(&format!("{key} {k}: {title}"), 100),
        };
        seq += 1;
        let entry = Entry {
            seq,
            at: crate::history::now(),
            actor,
            item: ItemRef {
                id: id.clone(),
                key,
                ntype: node.ntype.to_string(),
                title,
            },
            kind: kind_of,
            fields,
            summary,
            origin: origin.to_string(),
            op_ids,
            before: before_props,
            after: Some(after),
        };
        writes.push((key_of(seq), serde_json::to_string(&entry)?));
        made.push(seq);
    }
    let seq_text = seq.to_string();
    store.write(GraphScope::Primary, &mut |w| {
        for k in &deletes {
            w.delete_meta(k)?;
        }
        for (k, v) in &writes {
            w.set_meta(k, v)?;
        }
        w.set_meta(SEQ, &seq_text)
    })?;
    Ok(made)
}
