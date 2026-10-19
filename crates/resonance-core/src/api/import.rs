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
use serde_json::Value;

use super::{Engine, UnitEdit, UnitInput};
use crate::anchors::Anchor;
use crate::error::{Error, Result};
use crate::links::anchor_of;
use crate::model::{EdgeType, LinkStatus, Node, NodeType, ids};
use crate::store::{EdgeQuery, NodeQuery};

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
    region: String,
    match_text: Option<String>,
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
        let r = self.reader();
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
            region,
            match_text,
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
        for (i, c) in checked.into_iter().enumerate() {
            let c = c.expect("validated");
            let it = &items[i];
            let key = plan.items[i].key.clone();
            let id = match &c.existing {
                None => {
                    let fields: BTreeMap<String, String> = it
                        .fields
                        .iter()
                        .filter(|(_, v)| !v.trim().is_empty())
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    self.spec_add(c.ntype, Some(&key), &fields)?.id
                }
                Some(n) => {
                    if !c.changed.is_empty() {
                        let fields: BTreeMap<String, String> = c
                            .changed
                            .iter()
                            .map(|k| (k.clone(), it.fields[k].clone()))
                            .collect();
                        self.spec_edit(&n.id, &fields)?;
                    }
                    n.id.clone()
                }
            };
            // The source region (FR-124).
            let unit = match self.source_unit(&id)? {
                Some(u) => {
                    if plan.items[i].region_changed {
                        self.unit_edit(
                            &u.id,
                            UnitEdit {
                                region: Some(c.region.clone()),
                                match_text: c.match_text.clone(),
                                ..Default::default()
                            },
                        )?;
                    }
                    u.id
                }
                None => {
                    self.unit_add(UnitInput {
                        region: c.region.clone(),
                        match_text: c.match_text.clone(),
                        name: key.clone(),
                        kind: Some(SPEC_UNIT_KIND.into()),
                        run: None,
                        test: false,
                    })?
                    .id
                }
            };
            let link = self.link(&id, "derivedFrom", &unit, Some(1.0))?;
            if !matches!(
                link.link.status,
                LinkStatus::Asserted | LinkStatus::Verified
            ) {
                self.confirm(&link.link.id)?;
            }
            plan.items[i].id = Some(id);
            plan.items[i].unit = self
                .reader()
                .get_node(&unit)?
                .map(|n| crate::query::label_of(self.reader(), &n).unwrap_or(unit.clone()));
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
