//! Detects changed files and applies them incrementally (FR-047, FR-049, FR-012a).
//!
//! There is no daemon in the MVP: every command and MCP tool call reconciles first, then
//! answers (MVP-002). Reconcile has two phases. The first, outside any transaction, walks the
//! tree and hashes the files whose size or modification time changed. The second applies the
//! changes in one transaction: file content (as `file.put` / `file.delete`), anchors
//! (MVP-013), intent sources (MVP-011), link statuses (MVP-012) and containment.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::anchors::locate::{Step, relocate_with, restore};
use crate::anchors::{Anchor, AnchorState};
use crate::error::{Error, Result};
use crate::ingest::blobs::should_store;
use crate::ingest::lineindex::LineIndex;
use crate::ingest::walk::{
    compile_ignores, git_ignored, in_nested_repository, is_ignored_path, mtime_ns, walk,
};
use crate::intent::{self, IntentReport};
use crate::links::{self, StatusChanges, anchor_of};
use crate::model::{Edge, EdgeType, Layer, LinkStatus, Node, NodeType, content_hash, ids};
use crate::ops::{Actor, Tx, hlc, kind};
use crate::repo::abs_path;
use crate::repo::config::Config;
use crate::store::{EdgeQuery, FileRecord, GraphRead, GraphScope, GraphStore, NodeQuery, meta};

/// A file modified this close to when it was last recorded is re-hashed even if its size
/// and modification time look unchanged (the "racy timestamp" problem).
const RACY_NS: i64 = 2_000_000_000;

#[derive(Debug, Clone, Copy, Default)]
pub struct ReconcileOptions {
    /// Re-hash every file, ignoring size and modification time (`reconcile --full`).
    pub full: bool,
    /// Re-import intent sources even if they look unchanged.
    pub force_intent: bool,
    /// Recompute containment for every file (`rebuild-derived`).
    pub rebuild_containment: bool,
}

/// What anchors did during a reconcile.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorReport {
    /// Followed a change in their own file (MVP-013 steps 1 and 2).
    pub relocated: usize,
    /// Moved to another file (step 3, `identity.move`).
    pub moved: usize,
    /// Re-anchored approximately (step 4, `identity.retarget`).
    pub approximate: usize,
    /// Could not be matched (step 5).
    pub orphaned: usize,
    /// Orphans whose text reappeared (MVP-005).
    pub restored: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileReport {
    pub files_scanned: usize,
    pub files_hashed: usize,
    pub files_added: usize,
    pub files_changed: usize,
    pub files_removed: usize,
    /// Files detected as moved by content similarity (FR-036).
    #[serde(default)]
    pub files_moved: usize,
    pub anchors: AnchorReport,
    pub links: StatusChanges,
    pub intent: IntentReport,
    pub operations: usize,
    pub duration_ms: u64,
    /// What catching up with git and the delta history did first (FR-068, FR-195).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<crate::history::fold::CatchUp>,
}

impl ReconcileReport {
    /// Whether anything in the graph changed.
    pub fn changed_anything(&self) -> bool {
        self.operations > 0
    }
}

struct Hashed {
    path: String,
    size: u64,
    mtime_ns: i64,
    hash: String,
    bytes: Vec<u8>,
}

/// Bring the graph in step with the disk.
pub fn reconcile(
    store: &mut dyn GraphStore,
    root: &Path,
    config: &Config,
    opts: ReconcileOptions,
    index: &mut LineIndex,
) -> Result<ReconcileReport> {
    let started = Instant::now();
    let (hashed, removed, scanned) = scan_all(&*store, root, config, opts.full)?;
    commit(
        store, root, config, opts, index, hashed, removed, scanned, started,
    )
}

/// Reconcile only the given repository-relative paths, as reported by the watcher (FR-049).
/// Event contents are hints: each path is re-examined by size, time and hash (FR-042). When
/// one of them is a directory, this falls back to a full reconcile.
pub fn reconcile_paths(
    store: &mut dyn GraphStore,
    root: &Path,
    config: &Config,
    paths: &[String],
    index: &mut LineIndex,
) -> Result<ReconcileReport> {
    let started = Instant::now();
    match scan_paths(&*store, root, config, paths)? {
        Some((hashed, removed, scanned)) => commit(
            store,
            root,
            config,
            ReconcileOptions::default(),
            index,
            hashed,
            removed,
            scanned,
            started,
        ),
        None => reconcile(store, root, config, ReconcileOptions::default(), index),
    }
}

/// Read and hash a file unless its size and modification time show it unchanged.
fn hash_if_changed(
    root: &Path,
    path: &str,
    size: u64,
    mtime_ns: i64,
    record: Option<&FileRecord>,
    full: bool,
) -> Result<Option<Hashed>> {
    if !full
        && let Some(r) = record
        && r.size == size
        && r.mtime_ns == mtime_ns
        && mtime_ns.saturating_add(RACY_NS) <= r.recorded_ns
    {
        return Ok(None);
    }
    let bytes = match fs::read(abs_path(root, path)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Error::io(format!("reading {path}"), e)),
    };
    Ok(Some(Hashed {
        path: path.to_string(),
        size: bytes.len() as u64,
        mtime_ns,
        hash: content_hash(&bytes),
        bytes,
    }))
}

type Scan = (Vec<Hashed>, Vec<String>, usize);

/// Phase 1 for the whole tree: find and hash what changed, without holding the write lock.
fn scan_all(store: &dyn GraphStore, root: &Path, config: &Config, full: bool) -> Result<Scan> {
    let walked = walk(root, &config.ingest.ignore)?;
    let records: HashMap<String, FileRecord> = store
        .list_files()?
        .into_iter()
        .map(|r| (r.path.clone(), r))
        .collect();
    let mut hashed = Vec::new();
    for f in &walked {
        if let Some(h) = hash_if_changed(
            root,
            &f.path,
            f.size,
            f.mtime_ns,
            records.get(&f.path),
            full,
        )? {
            hashed.push(h);
        }
    }
    let on_disk: BTreeSet<&str> = walked.iter().map(|f| f.path.as_str()).collect();
    let removed: Vec<String> = records
        .keys()
        .filter(|p| !on_disk.contains(p.as_str()))
        .cloned()
        .collect();
    Ok((hashed, removed, walked.len()))
}

/// Phase 1 for some paths. `None` means a directory changed and the whole tree must be scanned.
fn scan_paths(
    store: &dyn GraphStore,
    root: &Path,
    config: &Config,
    paths: &[String],
) -> Result<Option<Scan>> {
    let extra = compile_ignores(&config.ingest.ignore)?;
    let mut candidates: Vec<(String, u64, i64)> = Vec::new();
    let mut removed: BTreeSet<String> = BTreeSet::new();
    let mut all_records: Option<Vec<FileRecord>> = None;
    for path in paths {
        let abs = abs_path(root, path);
        match fs::symlink_metadata(&abs) {
            Ok(md) if md.is_dir() => return Ok(None),
            Ok(md) if md.is_file() => candidates.push((path.clone(), md.len(), mtime_ns(&md))),
            Ok(_) => {
                // A symbolic link or something else that is not ingested.
                if store.get_file(path)?.is_some() {
                    removed.insert(path.clone());
                }
            }
            Err(_) => {
                if store.get_file(path)?.is_some() {
                    removed.insert(path.clone());
                } else {
                    // Perhaps a directory that was removed: everything under it is gone.
                    let records = match &all_records {
                        Some(r) => r,
                        None => all_records.insert(store.list_files()?),
                    };
                    let prefix = format!("{path}/");
                    for r in records.iter().filter(|r| r.path.starts_with(&prefix)) {
                        if !abs_path(root, &r.path).exists() {
                            removed.insert(r.path.clone());
                        }
                    }
                }
            }
        }
    }
    // Apply the same ignore rules as the full walk.
    let maybe: Vec<String> = candidates
        .iter()
        .map(|c| c.0.clone())
        .filter(|p| !is_ignored_path(p, &extra) && !in_nested_repository(root, p))
        .collect();
    let git_ignored = git_ignored(root, &maybe)?;
    let mut hashed = Vec::new();
    let mut scanned = 0;
    for (path, size, mtime) in candidates {
        let ignored = is_ignored_path(&path, &extra)
            || in_nested_repository(root, &path)
            || git_ignored.contains(&path);
        let record = store.get_file(&path)?;
        if ignored {
            if record.is_some() {
                removed.insert(path);
            }
            continue;
        }
        scanned += 1;
        if let Some(h) = hash_if_changed(root, &path, size, mtime, record.as_ref(), false)? {
            hashed.push(h);
        }
    }
    Ok(Some((hashed, removed.into_iter().collect(), scanned)))
}

/// Phase 2: apply everything in one transaction.
#[allow(clippy::too_many_arguments)]
fn commit(
    store: &mut dyn GraphStore,
    root: &Path,
    config: &Config,
    opts: ReconcileOptions,
    index: &mut LineIndex,
    hashed: Vec<Hashed>,
    removed: Vec<String>,
    scanned: usize,
    started: Instant,
) -> Result<ReconcileReport> {
    let mut report = ReconcileReport {
        files_scanned: scanned,
        files_hashed: hashed.len(),
        ..Default::default()
    };
    store.write(GraphScope::Primary, &mut |w| {
        let mut tx = Tx::new(w, Actor::resonance())?;
        apply(
            &mut tx,
            root,
            config,
            opts,
            index,
            &hashed,
            &removed,
            &mut report,
        )?;
        let now = hlc::now_ms();
        tx.w().set_meta(meta::LAST_RECONCILE, &hlc::rfc3339(now))?;
        report.operations = tx.finish()?;
        Ok(())
    })?;
    report.duration_ms = started.elapsed().as_millis() as u64;
    Ok(report)
}

/// A file whose content changed in this reconcile.
struct ContentChange {
    path: String,
    old_blob: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn apply(
    tx: &mut Tx<'_>,
    root: &Path,
    config: &Config,
    opts: ReconcileOptions,
    index: &mut LineIndex,
    hashed: &[Hashed],
    removed: &[String],
    report: &mut ReconcileReport,
) -> Result<()> {
    let now_ns = hlc::now_ns();
    let mut changes: Vec<ContentChange> = Vec::new();
    // New text of changed files, for anchors (only valid UTF-8 text files can hold anchors).
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    let mut changed_nodes: BTreeSet<String> = BTreeSet::new();

    for h in hashed {
        let id = ids::file_id(&h.path);
        let node = tx.r().get_node(&id)?;
        let old_blob = node
            .as_ref()
            .filter(|n| !n.retired)
            .and_then(|n| n.str_prop("blob"))
            .map(String::from);
        let stored = should_store(&h.bytes, config.ingest.max_blob_bytes);
        tx.w()
            .put_blob(&h.hash, h.size, stored.then_some(h.bytes.as_slice()))?;
        tx.w().put_file(&FileRecord {
            path: h.path.clone(),
            blob: h.hash.clone(),
            size: h.size,
            mtime_ns: h.mtime_ns,
            recorded_ns: now_ns,
        })?;
        if old_blob.as_deref() == Some(h.hash.as_str()) {
            continue; // Touched, not changed.
        }
        tx.apply(
            kind::FILE_PUT,
            json!({"path": h.path, "blob": h.hash, "size": h.size, "stored": stored, "prev": old_blob}),
        )?;
        if old_blob.is_some() {
            report.files_changed += 1;
            changed_nodes.insert(id.clone());
        } else {
            report.files_added += 1;
            if node.is_some() {
                // A file that comes back: its links return (MVP-005).
                report.links += links::restore_links(tx, &id)?;
            }
        }
        if stored && let Ok(text) = std::str::from_utf8(&h.bytes) {
            texts.insert(h.path.clone(), text.to_string());
        }
        changes.push(ContentChange {
            path: h.path.clone(),
            old_blob,
        });
    }

    // Files that disappeared, and whether each one moved to a new path (FR-036).
    let added: Vec<&str> = changes
        .iter()
        .filter(|c| c.old_blob.is_none() && texts.contains_key(&c.path))
        .map(|c| c.path.as_str())
        .collect();
    let mut gone: Vec<(String, Node)> = Vec::new();
    for path in removed {
        tx.w().delete_file(path)?;
        let id = ids::file_id(path);
        if let Some(node) = tx.r().get_node(&id)?
            && !node.retired
        {
            gone.push((path.clone(), node));
        }
    }
    let moves = detect_moves(tx, &gone, &added, &texts)?;
    for (path, node) in &gone {
        let id = ids::file_id(path);
        let old_blob = node.str_prop("blob").map(String::from);
        if let Some(to) = moves.get(path) {
            tx.apply(kind::FILE_MOVE, json!({"from": path, "to": to}))?;
            report.files_moved += 1;
            // Links and annotations on the file follow it.
            let to_id = ids::file_id(to);
            for e in links::links_of(tx.r(), &id)? {
                let end = if e.src == id { "src" } else { "dst" };
                tx.apply(
                    kind::IDENTITY_RETARGET,
                    json!({"edgeId": e.id, "end": end, "from": id, "to": to_id}),
                )?;
            }
        } else {
            tx.apply(
                kind::FILE_DELETE,
                json!({"path": path, "prev": node.str_prop("blob")}),
            )?;
            report.files_removed += 1;
            report.links += links::orphan_links(tx, &id)?;
        }
        changes.push(ContentChange {
            path: path.clone(),
            old_blob,
        });
    }
    if index.is_built() {
        for c in &changes {
            index.set(&c.path, texts.get(&c.path).map(String::as_str));
        }
    }

    // Anchors in changed files (MVP-013).
    let mut containment: BTreeSet<String> = changes.iter().map(|c| c.path.clone()).collect();
    if !changes.is_empty() {
        let by_path = anchored_by_path(tx)?;
        let others: Vec<(&str, &str)> = texts
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        for change in &changes {
            let Some(units) = by_path.get(&change.path) else {
                continue;
            };
            let old_text = match &change.old_blob {
                Some(b) => tx
                    .r()
                    .get_blob(b)?
                    .and_then(|bytes| String::from_utf8(bytes).ok()),
                None => None,
            };
            // A file that moved is followed as the same file at its new path (FR-036).
            let moved_to = moves.get(&change.path);
            let current_path = moved_to.cloned().unwrap_or_else(|| change.path.clone());
            let new_text = texts.get(&current_path).map(String::as_str);
            for unit in units {
                let Some(anchor) = anchor_of(unit) else {
                    continue;
                };
                if anchor.state != AnchorState::Attached {
                    continue;
                }
                let mut seen_at = anchor.clone();
                seen_at.path = current_path.clone();
                let r = relocate_with(&seen_at, old_text.as_deref(), new_text, &others, &mut |a| {
                    unchanged_files(tx.r(), index, a)
                });
                if r.anchor.path != anchor.path {
                    containment.insert(r.anchor.path.clone());
                }
                apply_relocation(
                    tx,
                    unit,
                    &anchor,
                    r.anchor,
                    r.step,
                    report,
                    &mut changed_nodes,
                )?;
            }
        }

        // MVP-005: orphans whose text reappears with matching context are restored.
        if !others.is_empty() {
            for nodes in by_path.values() {
                for unit in nodes {
                    let Some(current) = tx.r().get_node(&unit.id)? else {
                        continue;
                    };
                    let Some(anchor) = anchor_of(&current) else {
                        continue;
                    };
                    if anchor.state != AnchorState::Orphaned || current.retired {
                        continue;
                    }
                    if let Some(found) = restore(&anchor, &others) {
                        containment.insert(found.path.clone());
                        put_anchor(tx, &current.id, &anchor, &found)?;
                        report.anchors.restored += 1;
                        report.links += links::restore_links(tx, &current.id)?;
                    }
                }
            }
        }
    }

    // Intent sources (MVP-011): re-imported when they change.
    report.intent = intent::sync(tx, root, &config.intent.sources, opts.force_intent)?;
    for key in &report.intent.changed {
        changed_nodes.insert(ids::requirement_id(key));
    }

    // Link statuses (MVP-012).
    for id in &changed_nodes {
        report.links += links::refresh_statuses(tx, id)?;
    }

    // Derived containment.
    if opts.rebuild_containment {
        containment.extend(anchored_by_path(tx)?.into_keys());
    }
    for path in &containment {
        recompute_containment(tx, path)?;
    }
    Ok(())
}

/// After a re-fold (MVP-027): recreate File nodes from the stat cache and stored blobs, and
/// re-check every anchor against the stored content of its file, re-locating those that no
/// longer match. Imported intent is re-read and derived containment rebuilt. The disk is not
/// read; an ordinary reconcile afterwards picks up anything that changed there.
pub fn settle(
    store: &mut dyn GraphStore,
    root: &Path,
    config: &Config,
    index: &mut LineIndex,
) -> Result<ReconcileReport> {
    let started = Instant::now();
    *index = LineIndex::default();
    let mut report = ReconcileReport::default();
    store.write(GraphScope::Primary, &mut |w| {
        report = ReconcileReport::default();
        let mut tx = Tx::new(w, Actor::resonance())?;
        let mut texts: BTreeMap<String, String> = BTreeMap::new();
        for rec in tx.r().list_files()? {
            let bytes = tx.r().get_blob(&rec.blob)?;
            tx.apply(
                kind::FILE_PUT,
                json!({"path": rec.path, "blob": rec.blob, "size": rec.size, "stored": bytes.is_some(), "prev": null}),
            )?;
            report.files_added += 1;
            if let Some(text) = bytes.and_then(|b| String::from_utf8(b).ok()) {
                texts.insert(rec.path.clone(), text);
            }
        }
        report.files_scanned = report.files_added;
        let others: Vec<(&str, &str)> = texts
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        let mut changed_nodes: BTreeSet<String> = BTreeSet::new();
        let by_path = anchored_by_path(&tx)?;
        for units in by_path.values() {
            for unit in units {
                let Some(anchor) = anchor_of(unit) else {
                    continue;
                };
                if anchor.state == AnchorState::Orphaned {
                    if let Some(found) = restore(&anchor, &others) {
                        put_anchor(&mut tx, &unit.id, &anchor, &found)?;
                        report.anchors.restored += 1;
                        report.links += links::restore_links(&mut tx, &unit.id)?;
                    }
                    continue;
                }
                let current = texts.get(&anchor.path).map(String::as_str);
                let intact = current
                    .and_then(|t| t.get(anchor.start_byte..anchor.end_byte))
                    .is_some_and(|q| content_hash(q.as_bytes()) == anchor.quote_hash);
                if intact {
                    continue;
                }
                let r = relocate_with(&anchor, None, current, &others, &mut |_| Vec::new());
                apply_relocation(
                    &mut tx,
                    unit,
                    &anchor,
                    r.anchor,
                    r.step,
                    &mut report,
                    &mut changed_nodes,
                )?;
            }
        }
        report.intent = intent::sync(&mut tx, root, &config.intent.sources, true)?;
        for key in &report.intent.changed {
            changed_nodes.insert(ids::requirement_id(key));
        }
        for id in &changed_nodes {
            report.links += links::refresh_statuses(&mut tx, id)?;
        }
        let paths: BTreeSet<String> = anchored_by_path(&tx)?
            .into_keys()
            .chain(texts.keys().cloned())
            .collect();
        for path in &paths {
            recompute_containment(&mut tx, path)?;
        }
        tx.w()
            .set_meta(meta::LAST_RECONCILE, &hlc::rfc3339(hlc::now_ms()))?;
        report.operations = tx.finish()?;
        Ok(())
    })?;
    report.duration_ms = started.elapsed().as_millis() as u64;
    Ok(report)
}

/// Pair files that disappeared with new files whose content is at least half the same, as
/// git does (FR-036). Only files that authored data depends on are followed.
fn detect_moves(
    tx: &Tx<'_>,
    gone: &[(String, Node)],
    added: &[&str],
    texts: &BTreeMap<String, String>,
) -> Result<HashMap<String, String>> {
    let mut moves = HashMap::new();
    if gone.is_empty() || added.is_empty() {
        return Ok(moves);
    }
    let basename = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    let mut taken: BTreeSet<&str> = BTreeSet::new();
    for (path, node) in gone {
        let id = ids::file_id(path);
        let depends = !links::links_of(tx.r(), &id)?.is_empty()
            || [NodeType::CodeUnit, NodeType::Test].iter().any(|t| {
                tx.r()
                    .find_nodes(&NodeQuery::of_type(*t).with_prop("path", path.as_str()))
                    .is_ok_and(|v| !v.is_empty())
            });
        if !depends {
            continue;
        }
        let Some(old_text) = node
            .str_prop("blob")
            .map(|b| tx.r().get_blob(b))
            .transpose()?
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
        else {
            continue;
        };
        // With many candidates, compare only files of the same name.
        let many = added.len() > 50;
        let mut best: Option<(f64, &str)> = None;
        for a in added.iter().copied() {
            if taken.contains(a) || (many && basename(a) != basename(path)) {
                continue;
            }
            let new_text = &texts[a];
            let sim = similar::TextDiff::configure()
                .timeout(std::time::Duration::from_millis(200))
                .diff_lines(old_text.as_str(), new_text.as_str())
                .ratio() as f64;
            if sim >= 0.5 && best.is_none_or(|(b, _)| sim > b) {
                best = Some((sim, a));
            }
        }
        if let Some((_, a)) = best {
            taken.insert(a);
            moves.insert(path.clone(), a.to_string());
        }
    }
    Ok(moves)
}

/// Stored files that may contain `anchor`'s quote, for step 3b (MVP-024).
fn unchanged_files(
    r: &dyn GraphRead,
    index: &mut LineIndex,
    anchor: &Anchor,
) -> Vec<(String, String)> {
    if !index.is_built() && index.build(r).is_err() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for path in index.candidates(&anchor.quote) {
        let text = r
            .get_node(&ids::file_id(&path))
            .ok()
            .flatten()
            .filter(|n| !n.retired)
            .and_then(|n| n.str_prop("blob").map(String::from))
            .and_then(|b| r.get_blob(&b).ok().flatten())
            .and_then(|bytes| String::from_utf8(bytes).ok());
        if let Some(text) = text {
            out.push((path, text));
        }
    }
    out
}

/// Every code unit and test that is not retired, grouped by the file its anchor names.
fn anchored_by_path(tx: &Tx<'_>) -> Result<BTreeMap<String, Vec<Node>>> {
    let mut by_path: BTreeMap<String, Vec<Node>> = BTreeMap::new();
    for t in [NodeType::CodeUnit, NodeType::Test] {
        for n in tx.r().find_nodes(&NodeQuery::of_type(t))? {
            if let Some(p) = n.str_prop("path") {
                by_path.entry(p.to_string()).or_default().push(n);
            }
        }
    }
    for nodes in by_path.values_mut() {
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
    }
    Ok(by_path)
}

/// Record a new anchor for a unit, as `identity.move` when it changed files.
pub fn put_anchor(tx: &mut Tx<'_>, id: &str, old: &Anchor, new: &Anchor) -> Result<()> {
    if old.path != new.path {
        tx.apply(
            kind::IDENTITY_MOVE,
            json!({"id": id, "from": old.location(), "to": new.location(), "anchor": new}),
        )?;
    } else {
        tx.set_prop(id, "anchor", serde_json::to_value(new)?)?;
    }
    if old.path != new.path {
        tx.set_prop(id, "path", json!(new.path))?;
    }
    Ok(())
}

fn apply_relocation(
    tx: &mut Tx<'_>,
    unit: &Node,
    old: &Anchor,
    new: Anchor,
    step: Step,
    report: &mut ReconcileReport,
    changed: &mut BTreeSet<String>,
) -> Result<()> {
    match step {
        Step::Unchanged | Step::Diff | Step::Exact if new.path != old.path => {
            // The file itself moved (FR-036): the same unit, at a new path.
            put_anchor(tx, &unit.id, old, &new)?;
            report.anchors.moved += 1;
        }
        Step::Unchanged => return Ok(()),
        Step::Diff | Step::Exact => {
            if &new == old {
                return Ok(());
            }
            if new.confidence < 1.0 {
                // A judgment with a confidence is an identity decision (FR-033).
                tx.apply(
                    kind::IDENTITY_RETARGET,
                    json!({
                        "id": unit.id, "from": old.location(), "to": new.location(),
                        "confidence": new.confidence, "anchor": new,
                    }),
                )?;
            } else {
                tx.set_prop(&unit.id, "anchor", serde_json::to_value(&new)?)?;
            }
            report.anchors.relocated += 1;
        }
        Step::Move => {
            put_anchor(tx, &unit.id, old, &new)?;
            report.anchors.moved += 1;
        }
        Step::Approximate => {
            // Recorded as an identity decision with its confidence (FR-033).
            tx.apply(
                kind::IDENTITY_RETARGET,
                json!({
                    "id": unit.id, "from": old.location(), "to": new.location(),
                    "confidence": new.confidence, "anchor": new,
                }),
            )?;
            report.anchors.approximate += 1;
        }
        Step::Orphaned => {
            tx.set_prop(&unit.id, "anchor", serde_json::to_value(&new)?)?;
            report.anchors.orphaned += 1;
            report.links += links::orphan_links(tx, &unit.id)?;
            return Ok(());
        }
    }
    if new.quote_hash != old.quote_hash {
        changed.insert(unit.id.clone());
    }
    Ok(())
}

/// Recompute the derived `contains` edges of one file from its anchors (FR-011).
pub fn recompute_containment(tx: &mut Tx<'_>, path: &str) -> Result<()> {
    let file = ids::file_id(path);
    let mut units = Vec::new();
    for t in [NodeType::CodeUnit, NodeType::Test] {
        units.extend(
            tx.r().find_nodes(
                &NodeQuery::of_type(t)
                    .with_prop("path", path)
                    .including_retired(),
            )?,
        );
    }
    let mut stale: Vec<Edge> = tx
        .r()
        .find_edges(&EdgeQuery::from(&file).of_type(EdgeType::Contains))?;
    for u in &units {
        stale.extend(
            tx.r()
                .find_edges(&EdgeQuery::from(&u.id).of_type(EdgeType::Contains))?,
        );
        stale.extend(
            tx.r()
                .find_edges(&EdgeQuery::to(&u.id).of_type(EdgeType::Contains))?,
        );
    }
    for e in stale.iter().filter(|e| e.layer == Layer::Derived) {
        tx.w().delete_edge(&e.id)?;
    }
    let file_live = tx.r().get_node(&file)?.is_some_and(|n| !n.retired);
    if !file_live {
        return Ok(());
    }
    let mut live: Vec<(usize, usize, String)> = units
        .iter()
        .filter(|u| links::is_live(u))
        .filter_map(|u| anchor_of(u).map(|a| (a.start_byte, a.end_byte, u.id.clone())))
        .collect();
    live.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    let mut stack: Vec<(usize, usize, String)> = Vec::new();
    for u in live {
        while let Some(top) = stack.last() {
            let contains = top.0 <= u.0 && u.1 <= top.1 && (top.0, top.1) != (u.0, u.1);
            if contains {
                break;
            }
            stack.pop();
        }
        let parent = stack
            .last()
            .map(|t| t.2.clone())
            .unwrap_or_else(|| file.clone());
        tx.w().put_edge(&Edge {
            id: ids::contains_id(&parent, &u.2),
            etype: EdgeType::Contains,
            src: parent,
            dst: u.2.clone(),
            layer: Layer::Derived,
            status: LinkStatus::Asserted,
            removed: false,
            props: Default::default(),
        })?;
        stack.push(u);
    }
    Ok(())
}
