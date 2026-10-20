//! Compaction and snapshot merges (FR-152, FR-154–FR-157, MVP-034).
//!
//! Compaction folds the snapshot and the sealed deltas in effect into a new snapshot (a new
//! epoch) and deletes the folded delta files, as working-tree changes; Resonance never
//! commits on its own unless asked (FR-156). It runs on the trunk only by default, so feature
//! branches only ever add deltas and merge without conflicts (FR-155). Snapshots keep the
//! operations they folded, so two colliding compactions merge mechanically (FR-157).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::fold::collect;
use super::{Location, Mode, delta_path, keys, read_open, records};
use crate::error::{Error, Result};
use crate::model::content_hash;
use crate::ops::Op;
use crate::repo::Repo;
use crate::repo::config::Config;
use crate::store::{GraphScope, GraphStore, SCHEMA_VERSION, Snapshot, SnapshotMeta};

/// The epoch of a snapshot: the SHA-256 of the sorted IDs of the deltas it folded.
pub fn epoch_of(folded: &[String]) -> String {
    let mut ids: Vec<&String> = folded.iter().collect();
    ids.sort();
    let joined: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    content_hash(joined.join("\n").as_bytes())
}

/// A snapshot of `ops`, folding `folded` (sorted, deduplicated).
pub fn snapshot(ops: Vec<Op>, folded: Vec<String>) -> Snapshot {
    let mut folded: Vec<String> = folded
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    folded.sort();
    Snapshot {
        meta: SnapshotMeta {
            format: String::new(),
            format_version: 0,
            schema_version: SCHEMA_VERSION,
            epoch: epoch_of(&folded),
            folded_deltas: folded,
        },
        ops,
    }
}

/// The trunk branch (MVP-034): `compaction.trunk`, else the remote's default branch, else
/// `main`, else `master`.
pub fn trunk(repo: &Repo, config: &Config) -> String {
    if !config.compaction.trunk.is_empty() {
        return config.compaction.trunk.clone();
    }
    if let Ok(r) = repo.git(&[
        "symbolic-ref",
        "--quiet",
        "--short",
        "refs/remotes/origin/HEAD",
    ]) && let Some(b) = r.strip_prefix("origin/")
    {
        return b.to_string();
    }
    for b in ["main", "master"] {
        if repo
            .git(&[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{b}"),
            ])
            .is_ok()
        {
            return b.to_string();
        }
    }
    "main".into()
}

/// The branch checked out, if any.
pub fn current_branch(repo: &Repo) -> Option<String> {
    repo.git(&["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
}

/// What a compaction did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Compaction {
    pub folded: Vec<String>,
    pub operations: usize,
    pub epoch: String,
    /// The commit made with `--commit`.
    pub committed: Option<String>,
}

/// The number of sealed deltas in effect that a compaction would fold.
pub fn sealed_count(repo: &Repo, store: &dyn GraphStore, mode: Mode) -> Result<usize> {
    let open = read_open(store)?;
    let inputs = collect(repo, store, mode, open.as_ref())?;
    Ok(inputs
        .groups
        .iter()
        .filter(|(_, committed)| *committed)
        .map(|(ids, _)| ids.len())
        .sum())
}

/// Fold the snapshot and the sealed deltas in effect into a new snapshot. With `anywhere`,
/// runs off the trunk too; with `commit`, commits the result (shared repositories only).
pub fn compact(
    repo: &Repo,
    store: &mut dyn GraphStore,
    config: &Config,
    mode: Mode,
    anywhere: bool,
    commit: bool,
) -> Result<Compaction> {
    let trunk = trunk(repo, config);
    let branch = current_branch(repo);
    if !anywhere && branch.as_deref() != Some(trunk.as_str()) {
        return Err(Error::Invalid(format!(
            "compaction runs on the trunk ({trunk}) only, so that branches merge without conflicts (FR-155); you are on {}. Pass anywhere to resonance_compact to override.",
            branch.as_deref().unwrap_or("a detached HEAD")
        )));
    }
    let open = read_open(&*store)?;
    let inputs = collect(repo, &*store, mode, open.as_ref())?;
    let mut ops: Vec<Op> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut folded: Vec<String> = inputs
        .snapshot
        .as_ref()
        .map(|s| s.meta.folded_deltas.clone())
        .unwrap_or_default();
    if let Some(s) = &inputs.snapshot {
        for op in &s.ops {
            if seen.insert(op.op_id.clone()) {
                ops.push(op.clone());
            }
        }
    }
    let mut newly = Vec::new();
    for (d, (ids, committed)) in inputs.deltas.iter().zip(&inputs.groups) {
        if !committed {
            continue;
        }
        for op in &d.ops {
            if seen.insert(op.op_id.clone()) {
                ops.push(op.clone());
            }
        }
        newly.extend(ids.iter().cloned());
    }
    if newly.is_empty() {
        return Ok(Compaction {
            folded: Vec::new(),
            operations: ops.len(),
            epoch: inputs.snapshot.map(|s| s.meta.epoch).unwrap_or_default(),
            committed: None,
        });
    }
    folded.extend(newly.iter().cloned());
    let snap = snapshot(ops, folded);
    store.write_snapshot(&repo.snapshot_path(), &snap)?;
    let recs = records(&*store)?;
    // Private deltas are a local layer (FR-187). They are not in `newly` — `collect` keeps
    // them out of committed history — and a compaction must not delete them even if an id
    // collides with one.
    for id in &newly {
        if recs
            .get(id)
            .is_some_and(|r| r.location == Location::Private)
        {
            continue;
        }
        let p = delta_path(repo, Location::Deltas, id);
        if p.is_file() {
            fs::remove_file(&p).map_err(|e| Error::io(format!("removing {}", p.display()), e))?;
        }
    }
    let gone: Vec<String> = newly
        .iter()
        .filter(|id| {
            recs.get(*id)
                .is_some_and(|r| r.location != Location::Private)
        })
        .map(|id| format!("{}{id}", keys::DELTA_PREFIX))
        .collect();
    store.write(GraphScope::Primary, &mut |w| {
        for k in &gone {
            w.delete_meta(k)?;
        }
        Ok(())
    })?;
    let mut out = Compaction {
        folded: newly.clone(),
        operations: snap.ops.len(),
        epoch: snap.meta.epoch.clone(),
        committed: None,
    };
    if commit {
        if !mode.shared() {
            return Err(Error::Invalid(
                "offline compaction is local; there is nothing to commit".into(),
            ));
        }
        repo.git(&[
            "add",
            "-A",
            "--",
            ".resonance/graph.db",
            ".resonance/deltas",
        ])?;
        repo.git(&[
            "commit",
            "-q",
            "--no-verify",
            "-m",
            &format!(
                "resonance: compact {} delta(s) into a new snapshot",
                newly.len()
            ),
            "--",
            ".resonance/graph.db",
            ".resonance/deltas",
        ])?;
        out.committed = repo.head_commit();
    }
    Ok(out)
}

/// Merge two snapshots: the union of their operations, ours first, then theirs that ours
/// lacks; the union of their folded deltas (FR-157). Always succeeds.
pub fn merge(ours: &Snapshot, theirs: &Snapshot) -> Snapshot {
    let mut seen: BTreeSet<&str> = ours.ops.iter().map(|o| o.op_id.as_str()).collect();
    let mut ops = ours.ops.clone();
    for op in &theirs.ops {
        if seen.insert(op.op_id.as_str()) {
            ops.push(op.clone());
        }
    }
    let folded: Vec<String> = ours
        .meta
        .folded_deltas
        .iter()
        .chain(theirs.meta.folded_deltas.iter())
        .cloned()
        .collect();
    snapshot(ops, folded)
}

/// The merge driver (`resonance git-driver merge %O %A %B`): writes the merged snapshot to
/// `ours`, as git expects.
pub fn merge_files(store: &dyn GraphStore, ours: &Path, theirs: &Path) -> Result<Snapshot> {
    let a = store.read_snapshot(ours)?;
    let b = store.read_snapshot(theirs)?;
    let merged = merge(&a, &b);
    store.write_snapshot(ours, &merged)?;
    Ok(merged)
}

/// Repair a conflicted `graph.db` without the merge driver (C6): merge the two sides git
/// recorded, write the result and stage it.
pub fn repair_conflicted_snapshot(repo: &Repo, store: &dyn GraphStore) -> Result<bool> {
    let unmerged = repo.git(&["ls-files", "-u", "--", ".resonance/graph.db"])?;
    if unmerged.trim().is_empty() {
        return Ok(false);
    }
    let dir = repo.local_dir().join("merge");
    fs::create_dir_all(&dir).map_err(|e| Error::io("creating a merge directory", e))?;
    let ours = dir.join("ours.db");
    let theirs = dir.join("theirs.db");
    fs::write(&ours, repo.git_bytes(&["show", ":2:.resonance/graph.db"])?)
        .map_err(|e| Error::io("writing a merge side", e))?;
    fs::write(
        &theirs,
        repo.git_bytes(&["show", ":3:.resonance/graph.db"])?,
    )
    .map_err(|e| Error::io("writing a merge side", e))?;
    let merged = merge_files(store, &ours, &theirs)?;
    store.write_snapshot(&repo.snapshot_path(), &merged)?;
    repo.git(&["add", "--", ".resonance/graph.db"])?;
    let _ = fs::remove_dir_all(&dir);
    Ok(true)
}
