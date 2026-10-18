//! The fold (FR-150, FR-151, MVP-027): which deltas are in effect at `HEAD`, the order their
//! operations apply in, and re-folding the live store when that changes (FR-068).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::delta::{self, Delta};
use super::{
    HeadMove, Location, Mode, OpenDelta, keys, on_head_moved, read_open, records, signature, sync,
};
use crate::error::Result;
use crate::ops::{self, Hlc, Op};
use crate::repo::Repo;
use crate::store::{GraphRead, GraphScope, GraphStore, GraphWrite, Snapshot, meta};

/// The inputs of a fold.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub snapshot: Option<Snapshot>,
    /// Delta files other than the open one that are in effect, in fold order.
    pub deltas: Vec<Delta>,
    /// The open delta's file, if it exists.
    pub open: Option<Delta>,
}

impl Inputs {
    /// The folded operations (snapshot, then deltas) and the open delta's operations, each
    /// deduplicated by ID (FR-193).
    pub fn ops(&self) -> (Vec<Op>, Vec<Op>) {
        let mut seen = BTreeSet::new();
        let mut base = Vec::new();
        if let Some(s) = &self.snapshot {
            for op in &s.ops {
                if seen.insert(op.op_id.clone()) {
                    base.push(op.clone());
                }
            }
        }
        for d in &self.deltas {
            for op in &d.ops {
                if seen.insert(op.op_id.clone()) {
                    base.push(op.clone());
                }
            }
        }
        let open = self
            .open
            .iter()
            .flat_map(|d| d.ops.iter())
            .filter(|op| seen.insert(op.op_id.clone()))
            .cloned()
            .collect();
        (base, open)
    }
}

/// The commit that introduced each committed delta file, as a position in topological order.
fn introduced(repo: &Repo) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    let Ok(log) = repo.git(&[
        "log",
        "--topo-order",
        "--reverse",
        "--format=%x01%H",
        "--name-only",
        "--diff-filter=A",
        "--",
        ".resonance/deltas",
    ]) else {
        return out;
    };
    let mut index = 0usize;
    for line in log.lines() {
        if let Some(_commit) = line.strip_prefix('\u{1}') {
            index += 1;
            continue;
        }
        let path = std::path::Path::new(line.trim());
        if let Some(id) = delta::id_of(path) {
            out.entry(id).or_insert(index);
        }
    }
    out
}

fn list(dir: &std::path::Path) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter_map(|p| delta::id_of(&p).map(|id| (id, p)))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Collect the snapshot and the deltas in effect at `HEAD` (FR-073, MVP-030).
pub fn collect(
    repo: &Repo,
    store: &dyn GraphStore,
    mode: Mode,
    open: Option<&OpenDelta>,
) -> Result<Inputs> {
    let snapshot = if repo.snapshot_path().is_file() {
        Some(store.read_snapshot(&repo.snapshot_path())?)
    } else {
        None
    };
    let folded: BTreeSet<String> = snapshot
        .as_ref()
        .map(|s| s.meta.folded_deltas.iter().cloned().collect())
        .unwrap_or_default();
    let recs = records(store)?;
    let ancestors = super::ancestry(repo);
    let committed = if mode.shared() {
        introduced(repo)
    } else {
        HashMap::new()
    };
    // (bucket, position, id): bucket 0 is committed history, 1 is uncommitted.
    let mut keyed: Vec<((u8, usize, String), PathBuf)> = Vec::new();
    for loc in [Location::Deltas, Location::Private] {
        for (id, path) in list(&loc.dir(repo)) {
            if open.is_some_and(|o| o.id == id) || folded.contains(&id) {
                continue;
            }
            let key = if mode.associates(loc) {
                match recs.get(&id).and_then(|r| r.commit.clone()) {
                    Some(c) => match ancestors.get(&c) {
                        Some(pos) => (0, *pos, id.clone()),
                        None => continue, // Another branch, or the detached pool.
                    },
                    None => (1, 0, id.clone()),
                }
            } else {
                match committed.get(&id) {
                    Some(pos) => (0, *pos, id.clone()),
                    None => (1, 0, id.clone()),
                }
            };
            keyed.push((key, path));
        }
    }
    keyed.sort();
    // Deltas of the same commit are merged and ordered by clock, then ID (FR-151).
    let mut groups: BTreeMap<(u8, usize), Vec<Delta>> = BTreeMap::new();
    for ((bucket, pos, _), path) in keyed {
        let pos = if bucket == 0 { pos } else { 0 };
        groups
            .entry((bucket, pos))
            .or_default()
            .push(Delta::read(&path)?);
    }
    let mut deltas = Vec::new();
    for (_, mut group) in groups {
        if group.len() == 1 {
            deltas.push(group.remove(0));
            continue;
        }
        let mut ops: Vec<Op> = group.iter().flat_map(|d| d.ops.iter().cloned()).collect();
        ops.sort_by(|a, b| a.hlc.cmp(&b.hlc).then(a.op_id.cmp(&b.op_id)));
        let mut merged = group.remove(0);
        merged.ops = ops;
        deltas.push(merged);
    }
    let open_delta = match open {
        Some(o) => {
            let p = super::delta_path(repo, o.location, &o.id);
            if p.is_file() {
                Some(Delta::read(&p)?)
            } else {
                None
            }
        }
        None => None,
    };
    Ok(Inputs {
        snapshot,
        deltas,
        open: open_delta,
    })
}

/// Apply an operation during a fold. A fold never fails on one operation: an operation that
/// cannot apply (for example, it refers to a node that a concurrent branch never created) is
/// skipped and reported (FR-150).
fn apply_lenient(w: &mut dyn GraphWrite, op: &Op) -> bool {
    if is_identity_conflict(w, op) {
        return record_conflict(w, op).is_ok();
    }
    ops::apply(w, op).is_ok()
}

/// An identity operation on a unit that another change set already split or merged away:
/// the two cannot both hold (§9.4).
fn is_identity_conflict(w: &dyn GraphWrite, op: &Op) -> bool {
    use crate::ops::kind;
    if !matches!(
        op.kind.as_str(),
        kind::IDENTITY_MOVE | kind::IDENTITY_RETARGET | kind::IDENTITY_SPLIT | kind::IDENTITY_MERGE
    ) || op.payload.get("edgeId").is_some()
    {
        return false;
    }
    let ids: Vec<String> = match op.kind.as_str() {
        kind::IDENTITY_MERGE => op.payload["ids"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        _ => op.payload["id"]
            .as_str()
            .map(String::from)
            .into_iter()
            .collect(),
    };
    ids.iter().any(|id| {
        w.get_node(id).ok().flatten().is_some_and(|n| {
            n.retired
                && (n.props.contains_key("splitInto") || n.props.contains_key("mergedInto"))
                && n.str_prop("identityChangeSet") != Some(op.change_set.as_str())
        })
    })
}

/// A `Conflict` node for an identity operation that could not apply. Its ID comes from the
/// operation, so every clone records the same conflict.
fn record_conflict(w: &mut dyn GraphWrite, op: &Op) -> crate::error::Result<()> {
    use crate::model::{Layer, Node, NodeType, Props};
    let target = op.payload["id"]
        .as_str()
        .map(String::from)
        .or_else(|| op.payload["ids"].as_array().map(|a| format!("{a:?}")))
        .unwrap_or_default();
    let mut p = Props::new();
    p.insert(
        "description".into(),
        serde_json::json!(format!(
            "{} of {target} conflicts with a split or merge made on another branch",
            op.kind
        )),
    );
    p.insert("ops".into(), serde_json::json!([op.op_id]));
    p.insert("target".into(), serde_json::json!(target));
    p.insert("operation".into(), serde_json::to_value(op)?);
    p.insert("status".into(), serde_json::json!("open"));
    w.put_node(&Node::new(
        format!("conflict:{}", op.op_id),
        NodeType::Conflict,
        Layer::Authored,
        p,
    ))
}

/// What a catch-up did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatchUp {
    pub head_move: Option<HeadMove>,
    /// The live store was re-folded; derived data must be settled against the disk.
    pub refolded: bool,
    /// A re-fold that would drop operations without `HEAD` moving was refused (C6).
    pub held: bool,
    /// Operations in effect after the catch-up.
    pub operations: usize,
    /// Operations a re-fold could not apply.
    pub skipped: usize,
}

/// A re-fold that was refused, kept for `status` and `resonance rebuild`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Held {
    pub at: String,
    /// Operations in the live store that no delta or snapshot holds any more.
    pub missing: usize,
}

/// Bring the live store in line with the history at `HEAD` (FR-068, FR-195 step 1). Cheap
/// when nothing changed. With `force`, re-folds unconditionally (`resonance rebuild`).
pub fn catch_up(
    repo: &Repo,
    store: &mut dyn GraphStore,
    mode: Mode,
    writer: &str,
    force: bool,
) -> Result<CatchUp> {
    let mut out = CatchUp::default();
    // Never lose local operations: the open delta file reflects the store first.
    sync(repo, store, mode, writer)?;
    let head = repo.head_commit();
    let last_head = store.get_meta(keys::HEAD)?;
    if last_head.is_some() && head != last_head {
        out.head_move = Some(on_head_moved(
            repo,
            store,
            mode,
            writer,
            last_head.as_deref(),
        )?);
        sync(repo, store, mode, writer)?;
    }
    let open = read_open(&*store)?;
    let sig = signature(repo, mode, open.as_ref().map(|o| o.id.as_str()));
    let unchanged = store.get_meta(keys::SIGNATURE)?.as_deref() == Some(sig.as_str());
    let record_head = |store: &mut dyn GraphStore, sig: &str| -> Result<()> {
        let head = head.clone().unwrap_or_default();
        store.write(GraphScope::Primary, &mut |w| {
            w.set_meta(keys::HEAD, &head)?;
            w.set_meta(keys::SIGNATURE, sig)
        })
    };
    if unchanged && !force {
        if last_head.is_none() {
            record_head(store, &sig)?;
        }
        return Ok(out);
    }
    let inputs = collect(repo, &*store, mode, open.as_ref())?;
    let (base, open_ops) = inputs.ops();
    let target: BTreeSet<&str> = base
        .iter()
        .chain(open_ops.iter())
        .map(|o| o.op_id.as_str())
        .collect();
    let current: BTreeSet<String> = store
        .ops_after(0, usize::MAX)?
        .into_iter()
        .filter(|r| !delta::is_file_content_op(&r.op.kind))
        .map(|r| r.op.op_id)
        .collect();
    out.operations = target.len();
    let same = current.len() == target.len() && current.iter().all(|c| target.contains(c.as_str()));
    if same && !force {
        record_head(store, &sig)?;
        return Ok(out);
    }
    let missing = current
        .iter()
        .filter(|c| !target.contains(c.as_str()))
        .count();
    if missing > 0 && !force && head == last_head {
        // Operations would disappear although nothing in git moved: `.resonance/` changed
        // unexpectedly (C6). Keep the graph and wait for `resonance rebuild` or recovery.
        let held = serde_json::to_string(&Held {
            at: super::now(),
            missing,
        })?;
        store.write(GraphScope::Primary, &mut |w| w.set_meta(keys::HELD, &held))?;
        record_head(store, &sig)?;
        out.held = true;
        return Ok(out);
    }
    let open = match open {
        Some(o) => o,
        None => return Ok(out),
    };
    let carried = inputs
        .open
        .as_ref()
        .map(|d| d.manifest.clone())
        .unwrap_or_default();
    out.skipped = refold(store, &base, &open_ops, &open, carried)?;
    out.refolded = true;
    store.write(GraphScope::Primary, &mut |w| w.delete_meta(keys::HELD))?;
    record_head(store, &sig)?;
    Ok(out)
}

/// Replace the live store's authored state with the fold of `base` then `open_ops`. File
/// records (the stat cache), blobs and metadata stay; derived data must then be settled.
pub fn refold(
    store: &mut dyn GraphStore,
    base: &[Op],
    open_ops: &[Op],
    open: &OpenDelta,
    carried: BTreeMap<String, delta::ManifestEntry>,
) -> Result<usize> {
    let mut skipped: Vec<String> = Vec::new();
    let mut clock: Option<Hlc> = None;
    store.write(GraphScope::Primary, &mut |w| {
        skipped.clear();
        clock = w.get_meta(meta::HLC)?.as_deref().and_then(Hlc::parse);
        w.clear_graph()?;
        for op in base {
            if !apply_lenient(w, op) {
                skipped.push(op.op_id.clone());
            }
            w.append_op(op)?;
            clock = clock.max(Hlc::parse(&op.hlc));
        }
        let boundary = w.last_op_seq()?;
        for op in open_ops {
            if !apply_lenient(w, op) {
                skipped.push(op.op_id.clone());
            }
            w.append_op(op)?;
            clock = clock.max(Hlc::parse(&op.hlc));
        }
        let mut o = open.clone();
        o.after_seq = boundary;
        o.manifest_after = w.last_op_seq()?;
        o.carried = carried.clone();
        w.set_meta(keys::OPEN, &serde_json::to_string(&o)?)?;
        w.set_meta(keys::JOURNAL_SEQ, &w.last_op_seq()?.to_string())?;
        w.set_meta(keys::SKIPPED, &serde_json::to_string(&skipped)?)?;
        // Imported intent is re-read from its sources during the settle.
        w.delete_meta(crate::intent::meta::FINGERPRINT)?;
        if let Some(c) = clock {
            w.set_meta(meta::HLC, &c.to_string())?;
        }
        Ok(())
    })?;
    Ok(skipped.len())
}

/// After a settle, file operations up to now are not changes of the open delta.
pub fn mark_settled(store: &mut dyn GraphStore) -> Result<()> {
    let Some(mut open) = read_open(&*store)? else {
        return Ok(());
    };
    open.manifest_after = store.last_op_seq()?;
    let text = serde_json::to_string(&open)?;
    store.write(GraphScope::Primary, &mut |w| w.set_meta(keys::OPEN, &text))
}

/// The fold of everything in effect, for `resonance rebuild` and snapshots.
pub fn all_ops(repo: &Repo, store: &dyn GraphStore, mode: Mode) -> Result<Vec<Op>> {
    let open = read_open(store)?;
    let inputs = collect(repo, store, mode, open.as_ref())?;
    let (mut base, open_ops) = inputs.ops();
    base.extend(open_ops);
    Ok(base)
}

/// The read side used by status: held re-folds and skipped operations.
pub fn held(r: &dyn GraphRead) -> Result<Option<Held>> {
    Ok(match r.get_meta(keys::HELD)? {
        Some(s) => serde_json::from_str(&s).ok(),
        None => None,
    })
}
