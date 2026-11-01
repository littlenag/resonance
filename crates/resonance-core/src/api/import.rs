//! The agent's import of spec documents (FR-122, FR-124, MVP-042).
//!
//! Resonance reads no documents (NG6). An agent reads them, then hands over what it found: the
//! items, each with the region of the document it came from. With `dry_run` this returns the
//! plan for the agent to preview with the developer; otherwise, when every item validates, it
//! creates or updates the items, declares each region as a code unit of kind `spec`, and links
//! the item to it with `derivedFrom`, so that a later change on either side makes the link
//! stale. Afterwards the graph is the source of truth.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Engine;
use crate::anchors::{Anchor, AnchorState};
use crate::error::{Error, Result};
use crate::ingest::reconcile::{put_anchor, recompute_containment};
use crate::links::{self, anchor_of, new_link, new_link_props};
use crate::model::{Edge, EdgeType, Layer, LinkStatus, Node, NodeType, Props, ids, new_id};
use crate::ops::{Tx, hlc, kind};
use crate::store::{EdgeQuery, GraphRead, NodeQuery};

/// The kind of the code units that hold a spec item's source text.
pub const SPEC_UNIT_KIND: &str = "spec";

/// Where in the document an item came from: a line range, or an exact snippet.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRegion {
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    #[serde(rename = "match")]
    pub match_text: Option<String>,
}

/// One item the agent found in a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    #[serde(rename = "type")]
    pub ntype: String,
    pub key: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    pub region: ImportRegion,
}

/// What happens, or happened, to one item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStep {
    pub key: String,
    #[serde(rename = "type")]
    pub ntype: String,
    /// `create`, `update`, `unchanged` or `error`.
    pub action: String,
    /// For `update`: the fields that change. For `create`: the fields set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    /// The region, as lines of the document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<(usize, usize)>,
    /// The region moved, or is new.
    #[serde(default)]
    pub region_changed: bool,
    /// Status of the item's document link before this call, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_status: Option<String>,
    /// Apply confirms that link, taking the current text and region as its baseline.
    #[serde(default)]
    pub refresh_link: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// After applying: the item's ID and its source unit's address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// The plan, or what was applied.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    pub document: String,
    pub dry_run: bool,
    pub applied: bool,
    pub items: Vec<ImportStep>,
    pub errors: usize,
}

/// Fields an item needs when it is created.
fn required(ntype: NodeType) -> &'static [&'static str] {
    match ntype {
        NodeType::Requirement => &["text"],
        NodeType::Decision => &["title", "decision"],
        NodeType::Design => &["title"],
        NodeType::Constraint => &["text"],
        _ => &[],
    }
}

fn prop_text(n: &Node, k: &str) -> String {
    match n.props.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(v) => v.to_string(),
    }
}

/// What validation found for one item, kept for applying.
struct Checked {
    ntype: NodeType,
    existing: Option<Node>,
    anchor: Anchor,
    changed: Vec<String>,
}

impl Engine {
    /// The spec item with this key, of any type.
    pub(crate) fn item_by_key(&self, key: &str) -> Result<Option<Node>> {
        for t in NodeType::SPEC_ITEMS {
            if let Some(n) = self
                .reader()
                .find_nodes(&NodeQuery::of_type(t).including_retired())?
                .into_iter()
                .find(|n| n.str_prop("key") == Some(key))
            {
                return Ok(Some(n));
            }
        }
        Ok(None)
    }

    /// The live `spec` code unit an item was imported from, if any (FR-124).
    pub(crate) fn source_unit(&self, item_id: &str) -> Result<Option<Node>> {
        spec_source_unit(self.reader(), item_id)
    }

    fn check_import_item(&self, document: &str, item: &ImportItem) -> Result<Checked> {
        let ntype: NodeType = item
            .ntype
            .parse()
            .map_err(|_| Error::Usage(format!("`{}` is not a spec item type", item.ntype)))?;
        if !NodeType::SPEC_ITEMS.contains(&ntype) {
            return Err(Error::Usage(format!(
                "a {ntype} is not a spec item (Requirement, Decision, Design or Constraint)"
            )));
        }
        let key = item.key.trim();
        if key.is_empty()
            || key.contains(char::is_whitespace)
            || key.contains("::")
            || key.contains('#')
        {
            return Err(Error::Usage(format!(
                "`{}` cannot be used as a key",
                item.key
            )));
        }
        if ntype == NodeType::Requirement && !ids::is_requirement_key(key) {
            return Err(Error::Usage(format!(
                "`{key}` is not a requirement key; keys look like FR-042 or NFR-7 (capital letters, a dash, digits)"
            )));
        }
        let allowed = Self::spec_fields(ntype);
        for k in item.fields.keys() {
            if !allowed.contains(&k.as_str()) {
                return Err(Error::Usage(format!(
                    "a {ntype} has no field `{k}` (fields: {})",
                    allowed.join(", ")
                )));
            }
        }
        if let Some(st) = item.fields.get("status") {
            Self::check_status(ntype, st)?;
        }
        let existing = self.item_by_key(key)?;
        let mut changed = Vec::new();
        match &existing {
            Some(n) if n.ntype != ntype => {
                return Err(Error::Invalid(format!(
                    "{key} is already a {}, not a {ntype}",
                    n.ntype
                )));
            }
            Some(n) => {
                for k in required(ntype) {
                    if item.fields.get(*k).is_some_and(|v| v.trim().is_empty()) {
                        return Err(Error::Usage(format!("the {ntype} {k} must not be empty")));
                    }
                }
                for (k, v) in &item.fields {
                    if prop_text(n, k).trim() != v.trim() {
                        changed.push(k.clone());
                    }
                }
            }
            None => {
                for k in required(ntype) {
                    if item.fields.get(*k).is_none_or(|v| v.trim().is_empty()) {
                        return Err(Error::Usage(format!("a new {ntype} needs `{k}`")));
                    }
                }
                changed = item
                    .fields
                    .iter()
                    .filter(|(_, v)| !v.trim().is_empty())
                    .map(|(k, _)| k.clone())
                    .collect();
            }
        }
        let r = &item.region;
        let (region, match_text) = match (r.start_line, &r.match_text) {
            (Some(a), None) => (format!("{document}:{a}-{}", r.end_line.unwrap_or(a)), None),
            (None, Some(m)) => (document.to_string(), Some(m.clone())),
            _ => {
                return Err(Error::Usage(
                    "give the item's region as startLine (and endLine), or as an exact match"
                        .into(),
                ));
            }
        };
        let anchor = self.anchor_for(&region, match_text.as_deref(), None)?;
        Ok(Checked {
            ntype,
            existing,
            anchor,
            changed,
        })
    }

    /// `resonance_import_items` (FR-122, MVP-042): validate, and unless `dry_run`, apply.
    pub fn import_items(
        &mut self,
        document: &str,
        items: &[ImportItem],
        dry_run: bool,
    ) -> Result<ImportPlan> {
        if items.is_empty() {
            return Err(Error::Usage("give at least one item".into()));
        }
        // Regions are given relative to the repository root.
        let saved_cwd = std::mem::replace(&mut self.cwd, self.repo.root.clone());
        let result = self.import_inner(document, items, dry_run);
        self.cwd = saved_cwd;
        result
    }

    fn import_inner(
        &mut self,
        document: &str,
        items: &[ImportItem],
        dry_run: bool,
    ) -> Result<ImportPlan> {
        let document = self.repo.relative_path(&self.repo.root, document)?;
        let mut plan = ImportPlan {
            document: document.clone(),
            dry_run,
            ..Default::default()
        };
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for it in items {
            *seen.entry(it.key.trim().to_string()).or_default() += 1;
        }
        let mut checked: Vec<Option<Checked>> = Vec::new();
        for it in items {
            let mut step = ImportStep {
                key: it.key.trim().to_string(),
                ntype: it.ntype.clone(),
                ..Default::default()
            };
            let result = if seen.get(step.key.as_str()).copied().unwrap_or(0) > 1 {
                Err(Error::Invalid(format!(
                    "{} appears more than once in this import",
                    step.key
                )))
            } else {
                self.check_import_item(&document, it)
            };
            match result {
                Ok(c) => {
                    step.lines = Some((c.anchor.start_line, c.anchor.end_line));
                    let unit = match &c.existing {
                        Some(n) => self.source_unit(&n.id)?,
                        None => None,
                    };
                    step.region_changed = unit.as_ref().and_then(anchor_of).is_none_or(|a| {
                        a.path != c.anchor.path
                            || a.start_byte != c.anchor.start_byte
                            || a.end_byte != c.anchor.end_byte
                            || a.quote_hash != c.anchor.quote_hash
                    });
                    step.ntype = c.ntype.to_string();
                    step.fields = c.changed.clone();
                    step.action = match &c.existing {
                        None => "create",
                        Some(_) if c.changed.is_empty() && !step.region_changed => "unchanged",
                        Some(_) => "update",
                    }
                    .into();
                    if let Some(n) = &c.existing {
                        step.id = Some(n.id.clone());
                        if let Some(e) = document_edge(self.reader(), &n.id)? {
                            step.link_status = Some(e.status.as_str().to_string());
                        }
                        // Confirming an unchanged item, or one whose text still disagrees
                        // with the region, would erase a real drift (FR-124).
                        let body = imported_body(c.ntype, &it.fields, Some(n));
                        step.refresh_link =
                            step.action != "unchanged" && quote_agrees(&c.anchor.quote, &body);
                    }
                    checked.push(Some(c));
                }
                Err(e) => {
                    step.action = "error".into();
                    step.error = Some(e.to_string());
                    plan.errors += 1;
                    checked.push(None);
                }
            }
            plan.items.push(step);
        }
        if dry_run || plan.errors > 0 {
            return Ok(plan);
        }
        // One change set, so a later failure does not leave the earlier items committed.
        let write = plan.items.iter().any(|s| s.action != "unchanged");
        let mut unit_ids = vec![None; items.len()];
        if write {
            let author = self.actor.id.clone();
            let provenance = self.provenance().to_string();
            let flags: Vec<(String, bool, bool)> = plan
                .items
                .iter()
                .map(|s| (s.key.clone(), s.region_changed, s.refresh_link))
                .collect();
            let written = self.mutate(|tx| {
                let mut out = Vec::with_capacity(checked.len());
                for (i, c) in checked.iter().enumerate() {
                    let c = c.as_ref().expect("validated");
                    let (key, region_changed, refresh_link) = &flags[i];
                    let id = write_spec(tx, c, &items[i].fields, key, &author)?;
                    let unit = write_unit(tx, c, &id, key, &provenance, &author, *region_changed)?;
                    ensure_derived(tx, &id, &unit, &provenance, &author)?;
                    if *refresh_link {
                        confirm_if_stale(tx, &id, &unit)?;
                    }
                    out.push((id, unit));
                }
                Ok(out)
            })?;
            for (i, (id, unit)) in written.into_iter().enumerate() {
                plan.items[i].id = Some(id);
                unit_ids[i] = Some(unit);
            }
        } else {
            for (i, c) in checked.iter().enumerate() {
                let Some(n) = c.as_ref().and_then(|c| c.existing.as_ref()) else {
                    continue;
                };
                plan.items[i].id = Some(n.id.clone());
                if let Some(u) = self.source_unit(&n.id)? {
                    unit_ids[i] = Some(u.id);
                }
            }
        }
        for (i, unit) in unit_ids.into_iter().enumerate() {
            let Some(unit) = unit else { continue };
            plan.items[i].unit = self
                .reader()
                .get_node(&unit)?
                .map(|n| crate::query::label_of(self.reader(), &n).unwrap_or(unit.clone()));
            if let Some(id) = &plan.items[i].id
                && let Some(e) = document_edge(self.reader(), id)?
            {
                plan.items[i].link_status = Some(e.status.as_str().to_string());
            }
        }
        plan.applied = true;
        Ok(plan)
    }

    /// Where a spec item came from, for views: the document, its lines and the unit (FR-124).
    pub(crate) fn source_of(&self, item_id: &str) -> Result<Option<Value>> {
        let Some(u) = self.source_unit(item_id)? else {
            return Ok(None);
        };
        let a = anchor_of(&u);
        Ok(Some(serde_json::json!({
            "path": u.str_prop("path"),
            "line": a.as_ref().map(|a| a.start_line),
            "endLine": a.as_ref().map(|a| a.end_line),
            "unit": u.id,
        })))
    }
}

/// Spec items that still carry the removed importer's `source` property and have no
/// document link. Their keys, for `doctor`.
pub fn legacy_unlinked_sources(r: &dyn GraphRead) -> Result<Vec<String>> {
    let mut keys = Vec::new();
    for t in NodeType::SPEC_ITEMS {
        for n in r.find_nodes(&NodeQuery::of_type(t).including_retired())? {
            if n.props.get("source").is_none_or(Value::is_null) {
                continue;
            }
            if spec_source_unit(r, &n.id)?.is_none() {
                keys.push(n.str_prop("key").unwrap_or(n.id.as_str()).to_string());
            }
        }
    }
    keys.sort();
    Ok(keys)
}

fn spec_source_unit(r: &dyn GraphRead, item_id: &str) -> Result<Option<Node>> {
    for e in r.find_edges(&EdgeQuery::from(item_id).of_type(EdgeType::DerivedFrom))? {
        if e.removed {
            continue;
        }
        if let Some(n) = r.get_node(&e.dst)?
            && n.ntype == NodeType::CodeUnit
            && !n.retired
            && n.str_prop("kind") == Some(SPEC_UNIT_KIND)
        {
            return Ok(Some(n));
        }
    }
    Ok(None)
}

fn document_edge(r: &dyn GraphRead, item_id: &str) -> Result<Option<Edge>> {
    let Some(u) = spec_source_unit(r, item_id)? else {
        return Ok(None);
    };
    Ok(
        r.find_edges(&EdgeQuery::from(item_id).of_type(EdgeType::DerivedFrom))?
            .into_iter()
            .find(|e| !e.removed && e.dst == u.id),
    )
}

/// The item text this import would leave in the graph, to compare with the region quote.
fn imported_body(
    ntype: NodeType,
    fields: &BTreeMap<String, String>,
    existing: Option<&Node>,
) -> String {
    let key = match ntype {
        NodeType::Decision => "decision",
        NodeType::Design
            if fields.get("text").is_some_and(|v| !v.trim().is_empty())
                || (fields.get("text").is_none()
                    && existing.is_some_and(|n| !prop_text(n, "text").trim().is_empty())) =>
        {
            "text"
        }
        NodeType::Design => "title",
        _ => "text",
    };
    if let Some(v) = fields.get(key) {
        return v.trim().to_string();
    }
    existing
        .map(|n| prop_text(n, key).trim().to_string())
        .unwrap_or_default()
}

/// The region quote and the item text describe the same passage.
fn quote_agrees(quote: &str, text: &str) -> bool {
    let q = quote.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    !t.is_empty() && !q.is_empty() && (q.contains(&t) || t.contains(&q))
}

fn spec_node_id(ntype: NodeType, key: &str) -> String {
    match ntype {
        NodeType::Requirement => ids::requirement_id(key),
        NodeType::Decision => format!("dec:{key}"),
        NodeType::Design => format!("design:{key}"),
        NodeType::Constraint => format!("constraint:{key}"),
        _ => new_id(),
    }
}

fn field_value(fields: &BTreeMap<String, String>, key: &str) -> Option<String> {
    fields
        .get(key)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn write_spec(
    tx: &mut Tx<'_>,
    c: &Checked,
    fields: &BTreeMap<String, String>,
    key: &str,
    author: &str,
) -> Result<String> {
    let Some(existing) = &c.existing else {
        let id = spec_node_id(c.ntype, key);
        let mut p = Props::new();
        p.insert("key".into(), json!(key));
        let status = field_value(fields, "status").unwrap_or_else(|| match c.ntype {
            NodeType::Decision => "accepted".into(),
            _ => "active".into(),
        });
        match c.ntype {
            NodeType::Requirement => {
                let text = field_value(fields, "text")
                    .ok_or_else(|| Error::Usage("a requirement needs text".into()))?;
                let kind = field_value(fields, "kind")
                    .unwrap_or_else(|| ids::requirement_kind(key).to_string());
                p.insert("text".into(), json!(text));
                p.insert("kind".into(), json!(kind));
            }
            NodeType::Decision => {
                let title = field_value(fields, "title")
                    .ok_or_else(|| Error::Usage("a decision needs a title".into()))?;
                let decision = field_value(fields, "decision")
                    .ok_or_else(|| Error::Usage("a decision needs its decision".into()))?;
                p.insert("title".into(), json!(title));
                p.insert("decision".into(), json!(decision));
                p.insert("context".into(), json!(field_value(fields, "context")));
                p.insert(
                    "consequences".into(),
                    json!(field_value(fields, "consequences")),
                );
            }
            NodeType::Design | NodeType::Constraint => {
                p.insert("title".into(), json!(field_value(fields, "title")));
                p.insert("text".into(), json!(field_value(fields, "text")));
                if c.ntype == NodeType::Constraint {
                    p.insert("scope".into(), json!(field_value(fields, "scope")));
                }
            }
            _ => {}
        }
        p.insert("status".into(), json!(status));
        p.insert("author".into(), json!(author));
        p.insert("createdAt".into(), json!(hlc::rfc3339(hlc::now_ms())));
        tx.create_node(&id, c.ntype, Layer::Authored, p)?;
        return Ok(id);
    };
    let mut changed = false;
    for k in &c.changed {
        let v = fields[k.as_str()].trim();
        if existing.props.get(k).and_then(Value::as_str) != Some(v) {
            tx.set_prop(&existing.id, k, json!(v))?;
            changed = true;
        }
    }
    if changed {
        links::refresh_statuses(tx, &existing.id)?;
        links::stale_document_links(tx, &existing.id)?;
    }
    Ok(existing.id.clone())
}

fn write_unit(
    tx: &mut Tx<'_>,
    c: &Checked,
    item_id: &str,
    key: &str,
    provenance: &str,
    author: &str,
    region_changed: bool,
) -> Result<String> {
    if let Some(u) = spec_source_unit(tx.r(), item_id)? {
        if region_changed {
            let old =
                anchor_of(&u).ok_or_else(|| Error::Store(format!("{} has no anchor", u.id)))?;
            let new = &c.anchor;
            if new.path != old.path {
                put_anchor(tx, &u.id, &old, new)?;
            } else {
                tx.apply(
                    kind::IDENTITY_RETARGET,
                    json!({
                        "id": u.id,
                        "from": old.location(),
                        "to": new.location(),
                        "confidence": 1.0,
                        "anchor": new,
                    }),
                )?;
            }
            if old.state == AnchorState::Orphaned {
                links::restore_links(tx, &u.id)?;
            }
            links::refresh_statuses(tx, &u.id)?;
            recompute_containment(tx, &old.path)?;
            if new.path != old.path {
                recompute_containment(tx, &new.path)?;
            }
        }
        return Ok(u.id);
    }
    let id = new_id();
    let mut p = Props::new();
    p.insert("name".into(), json!(key));
    p.insert("kind".into(), json!(SPEC_UNIT_KIND));
    p.insert("path".into(), json!(c.anchor.path));
    p.insert("anchor".into(), serde_json::to_value(&c.anchor)?);
    p.insert("author".into(), json!(author));
    p.insert("provenance".into(), json!(provenance));
    p.insert("createdAt".into(), json!(hlc::rfc3339(hlc::now_ms())));
    tx.create_node(&id, NodeType::CodeUnit, Layer::Authored, p)?;
    recompute_containment(tx, &c.anchor.path)?;
    Ok(id)
}

fn ensure_derived(
    tx: &mut Tx<'_>,
    src: &str,
    dst: &str,
    provenance: &str,
    author: &str,
) -> Result<()> {
    let exists = tx
        .r()
        .find_edges(&EdgeQuery::from(src).of_type(EdgeType::DerivedFrom))?
        .into_iter()
        .any(|e| !e.removed && e.dst == dst);
    if exists {
        return Ok(());
    }
    let s = tx
        .r()
        .get_node(src)?
        .ok_or_else(|| Error::NotFound(src.to_string()))?;
    let d = tx
        .r()
        .get_node(dst)?
        .ok_or_else(|| Error::NotFound(dst.to_string()))?;
    super::check_link_types(EdgeType::DerivedFrom, &s, &d)?;
    super::require_live(&s, src)?;
    super::require_live(&d, dst)?;
    let props = new_link_props(tx.r(), src, dst, provenance, author, Some(1.0))?;
    let edge = new_link(EdgeType::DerivedFrom, src, dst, props);
    tx.add_edge(&edge)
}

fn confirm_if_stale(tx: &mut Tx<'_>, src: &str, dst: &str) -> Result<()> {
    let Some(e) = tx
        .r()
        .find_edges(&EdgeQuery::from(src).of_type(EdgeType::DerivedFrom))?
        .into_iter()
        .find(|e| !e.removed && e.dst == dst)
    else {
        return Ok(());
    };
    if e.status != LinkStatus::Stale {
        return Ok(());
    }
    tx.set_edge(&e.id, "status", json!(LinkStatus::Asserted))?;
    links::refresh_hashes(tx, &e)
}
