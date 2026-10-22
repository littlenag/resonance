//! Divergence, recovery and counters (§8.18, MVP-031, MVP-032, MVP-039).
//!
//! Every disagreement between git, the working tree and the graph is classified from git
//! evidence into the classes C1–C9 of §8.18 and recorded with its level and resolution
//! (FR-190, FR-198). Resolutions act automatically only when nothing is lost: whatever they
//! displace is kept under `.resonance/local/recovery/<id>/`, and `resonance recover undo <id>`
//! puts it back (FR-192). Counters behind `resonance_value` are kept the same way, locally.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Error, Result};
use crate::model::{content_hash, new_id};
use crate::ops::{self, Op, hlc, kind};
use crate::repo::{Repo, abs_path};
use crate::store::{GraphRead, GraphScope, GraphStore, GraphWrite};

/// Metadata key prefixes.
pub mod keys {
    pub const DIVERGENCE: &str = "divergence:";
    pub const COUNTER: &str = "counter:";
}

/// The classes of §8.18.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Class {
    /// A git operation brought new content.
    C1,
    /// A commit made without Resonance.
    C2,
    /// An edit made while the daemon was down.
    C3,
    /// Git discarded uncommitted changes.
    C4,
    /// Git's merge differs from the graph's (a merge touched anchored code).
    C5,
    /// `.resonance/` data changed unexpectedly.
    C6,
    /// A long absence or rewritten history.
    C7,
    /// A commit bypassed the consistency checks.
    C8,
    /// Unexplained.
    C9,
}

impl Class {
    pub fn describe(self) -> &'static str {
        match self {
            Class::C1 => "a git operation brought new content",
            Class::C2 => "commits made without Resonance",
            Class::C3 => "edits made while Resonance was not running",
            Class::C4 => "git discarded an uncommitted change",
            Class::C5 => "a merge changed annotated code",
            Class::C6 => ".resonance/ changed unexpectedly",
            Class::C7 => "a long absence or rewritten history",
            Class::C8 => "a commit bypassed the consistency checks",
            Class::C9 => "an unexplained change",
        }
    }
}

/// How much a divergence needs a person (FR-201's levels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    Info,
    Attention,
    ActionRequired,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Attention => "attention",
            Level::ActionRequired => "action required",
        }
    }
}

/// One classified divergence (MVP-031).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Divergence {
    pub id: String,
    pub class: Class,
    pub level: Level,
    pub at: String,
    /// What happened, in one line.
    pub summary: String,
    /// Git evidence, for example the reflog entry and the commits involved.
    pub evidence: Vec<String>,
    /// What Resonance did about it.
    pub resolution: String,
    /// Paths involved (shortened lists for bulk operations).
    pub paths: Vec<String>,
    /// The recovery record that can undo the resolution, if any.
    pub recovery: Option<String>,
    /// Whether it needs nothing more from anyone. Info is resolved when recorded.
    pub resolved: bool,
}

impl Divergence {
    pub fn new(class: Class, level: Level, summary: impl Into<String>) -> Divergence {
        Divergence {
            id: new_id(),
            class,
            level,
            at: hlc::rfc3339(hlc::now_ms()),
            summary: summary.into(),
            evidence: Vec::new(),
            resolution: String::new(),
            paths: Vec::new(),
            recovery: None,
            resolved: level == Level::Info,
        }
    }
}

/// How many divergence records are kept (the counters keep the totals).
const KEEP: usize = 200;

/// Record divergences, count them per class (FR-198), and trim old resolved records.
pub fn record(store: &mut dyn GraphStore, items: &[Divergence]) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let mut texts = Vec::new();
    for d in items {
        texts.push((
            format!("{}{}", keys::DIVERGENCE, d.id),
            serde_json::to_string(d)?,
        ));
    }
    let existing = store.meta_prefix(keys::DIVERGENCE)?;
    store.write(GraphScope::Primary, &mut |w| {
        for (k, v) in &texts {
            w.set_meta(k, v)?;
        }
        for d in items {
            bump(w, &format!("divergence.{:?}", d.class), 1)?;
        }
        // Keys are ULIDs: oldest first. Trim resolved ones beyond the limit.
        let overflow = (existing.len() + texts.len()).saturating_sub(KEEP);
        let mut removed = 0;
        for (k, v) in &existing {
            if removed >= overflow {
                break;
            }
            if serde_json::from_str::<Divergence>(v).is_ok_and(|d| d.resolved) {
                w.delete_meta(k)?;
                removed += 1;
            }
        }
        Ok(())
    })
}

/// Divergences, newest first; only unresolved ones unless `all`.
pub fn list(r: &dyn GraphRead, all: bool) -> Result<Vec<Divergence>> {
    let mut out: Vec<Divergence> = r
        .meta_prefix(keys::DIVERGENCE)?
        .into_iter()
        .filter_map(|(_, v)| serde_json::from_str(&v).ok())
        .filter(|d: &Divergence| all || !d.resolved)
        .collect();
    out.reverse();
    Ok(out)
}

pub fn get(r: &dyn GraphRead, id: &str) -> Result<Divergence> {
    let matches: Vec<Divergence> = r
        .meta_prefix(keys::DIVERGENCE)?
        .into_iter()
        .filter(|(k, _)| k[keys::DIVERGENCE.len()..].starts_with(id))
        .filter_map(|(_, v)| serde_json::from_str(&v).ok())
        .collect();
    match matches.len() {
        1 => Ok(matches.into_iter().next().expect("one")),
        0 => Err(Error::NotFound(format!("divergence {id}"))),
        _ => Err(Error::Invalid(format!(
            "{id} matches several divergences; give more of the ID"
        ))),
    }
}

/// Mark a divergence resolved, with a note of how.
pub fn resolve(store: &mut dyn GraphStore, id: &str, how: &str) -> Result<Divergence> {
    let mut d = get(&*store, id)?;
    d.resolved = true;
    if !how.is_empty() {
        d.resolution = if d.resolution.is_empty() {
            how.to_string()
        } else {
            format!("{}; {how}", d.resolution)
        };
    }
    let text = serde_json::to_string(&d)?;
    let key = format!("{}{}", keys::DIVERGENCE, d.id);
    store.write(GraphScope::Primary, &mut |w| w.set_meta(&key, &text))?;
    Ok(d)
}

// ----- Counters (FR-198, FR-240, MVP-039) --------------------------------------------------

fn bump(w: &mut dyn GraphWrite, name: &str, by: u64) -> Result<()> {
    let key = format!("{}{name}", keys::COUNTER);
    let n: u64 = w.get_meta(&key)?.and_then(|v| v.parse().ok()).unwrap_or(0);
    w.set_meta(&key, &(n + by).to_string())
}

/// Add to local counters in one transaction.
pub fn count(store: &mut dyn GraphStore, items: &[(&str, u64)]) -> Result<()> {
    if items.iter().all(|(_, n)| *n == 0) {
        return Ok(());
    }
    store.write(GraphScope::Primary, &mut |w| {
        for (name, by) in items {
            if *by > 0 {
                bump(w, name, *by)?;
            }
        }
        Ok(())
    })
}

/// Every counter, by name.
pub fn counters(r: &dyn GraphRead) -> Result<std::collections::BTreeMap<String, u64>> {
    Ok(r.meta_prefix(keys::COUNTER)?
        .into_iter()
        .map(|(k, v)| (k[keys::COUNTER.len()..].to_string(), v.parse().unwrap_or(0)))
        .collect())
}

// ----- Recovery records and the recovery writer (FR-192, MVP-032) --------------------------

/// What a resolution displaced, and how to put it back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryRecord {
    pub id: String,
    pub path: String,
    pub class: Class,
    pub at: String,
    /// What was kept: `disk` (the graph followed the file) or `graph` (the file was restored).
    pub kept: String,
    /// The content hash of the displaced version, stored next to this record.
    pub displaced_hash: String,
    /// The content hash of the version that was kept.
    pub kept_hash: Option<String>,
    pub explanation: String,
    pub undone_at: Option<String>,
}

fn record_dir(repo: &Repo, id: &str) -> PathBuf {
    repo.recovery_dir().join(id)
}

/// Keep displaced bytes and a record; returns the record.
pub fn keep(
    repo: &Repo,
    path: &str,
    class: Class,
    kept: &str,
    displaced: &[u8],
    kept_hash: Option<String>,
    explanation: &str,
) -> Result<RecoveryRecord> {
    let rec = RecoveryRecord {
        id: new_id(),
        path: path.to_string(),
        class,
        at: hlc::rfc3339(hlc::now_ms()),
        kept: kept.to_string(),
        displaced_hash: content_hash(displaced),
        kept_hash,
        explanation: explanation.to_string(),
        undone_at: None,
    };
    let dir = record_dir(repo, &rec.id);
    fs::create_dir_all(&dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
    fs::write(dir.join("displaced"), displaced)
        .map_err(|e| Error::io("writing a recovery copy", e))?;
    write_record(repo, &rec)?;
    Ok(rec)
}

fn write_record(repo: &Repo, rec: &RecoveryRecord) -> Result<()> {
    let dir = record_dir(repo, &rec.id);
    fs::write(dir.join("record.json"), serde_json::to_string_pretty(rec)?)
        .map_err(|e| Error::io("writing a recovery record", e))
}

/// Every recovery record, newest first.
pub fn records(repo: &Repo) -> Vec<RecoveryRecord> {
    let mut out: Vec<RecoveryRecord> = fs::read_dir(repo.recovery_dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| fs::read_to_string(e.path().join("record.json")).ok())
                .filter_map(|t| serde_json::from_str(&t).ok())
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.id.cmp(&a.id));
    out
}

pub fn find_record(repo: &Repo, id: &str) -> Result<RecoveryRecord> {
    let matches: Vec<RecoveryRecord> = records(repo)
        .into_iter()
        .filter(|r| r.id.starts_with(id))
        .collect();
    match matches.len() {
        1 => Ok(matches.into_iter().next().expect("one")),
        0 => Err(Error::NotFound(format!("recovery record {id}"))),
        _ => Err(Error::Invalid(format!(
            "{id} matches several recovery records; give more of the ID"
        ))),
    }
}

/// Put the displaced version back in place (`recover undo`). The version being replaced is
/// itself kept, so an undo can be undone.
pub fn undo(repo: &Repo, id: &str) -> Result<(RecoveryRecord, RecoveryRecord)> {
    let mut rec = find_record(repo, id)?;
    if rec.undone_at.is_some() {
        return Err(Error::Invalid(format!(
            "recovery {} was already undone at {}",
            rec.id,
            rec.undone_at.as_deref().unwrap_or("?")
        )));
    }
    let displaced = fs::read(record_dir(repo, &rec.id).join("displaced"))
        .map_err(|e| Error::io("reading a recovery copy", e))?;
    let target = abs_path(&repo.root, &rec.path);
    let current = fs::read(&target).unwrap_or_default();
    let back = keep(
        repo,
        &rec.path,
        rec.class,
        "disk",
        &current,
        Some(content_hash(&displaced)),
        &format!("replaced when recovery {} was undone", rec.id),
    )?;
    write_file(&target, &displaced)?;
    rec.undone_at = Some(hlc::rfc3339(hlc::now_ms()));
    write_record(repo, &rec)?;
    Ok((rec, back))
}

/// The recovery writer (FR-050, MVP-032): an atomic replace in the same directory that keeps
/// the file's permissions. Line endings are the content's own.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::Invalid(format!("{} has no directory", path.display())))?;
    fs::create_dir_all(dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
    let mode = fs::metadata(path).ok().map(|m| m.permissions().mode());
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.resonance-tmp"));
    fs::write(&tmp, bytes).map_err(|e| Error::io(format!("writing {}", tmp.display()), e))?;
    if let Some(m) = mode {
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(m));
    }
    fs::rename(&tmp, path).map_err(|e| Error::io(format!("writing {}", path.display()), e))
}

// ----- Classification (FR-190, FR-195, MVP-031) --------------------------------------------

/// What a reconcile observed, for classification.
pub struct Observed<'a> {
    pub catch_up: &'a crate::history::fold::CatchUp,
    pub changed: &'a [crate::ingest::reconcile::ChangedFile],
    /// The first reconcile of a daemon that just started (C3 applies only then).
    pub startup: bool,
    pub mode: crate::history::Mode,
    /// This developer's email, to tell their commits from others' (C2).
    pub me: &'a str,
    pub config: &'a crate::repo::config::Config,
}

fn shorten(paths: Vec<String>) -> Vec<String> {
    let n = paths.len();
    if n <= 20 {
        return paths;
    }
    let mut out: Vec<String> = paths.into_iter().take(20).collect();
    out.push(format!("… and {} more", n - 20));
    out
}

/// Files that hold code units or tests.
fn anchored_paths(r: &dyn GraphRead) -> Result<std::collections::BTreeSet<String>> {
    use crate::model::NodeType;
    use crate::store::NodeQuery;
    let mut out = std::collections::BTreeSet::new();
    for t in [NodeType::CodeUnit, NodeType::Test] {
        for n in r.find_nodes(&NodeQuery::of_type(t))? {
            if let Some(p) = n.str_prop("path") {
                out.insert(p.to_string());
            }
        }
    }
    Ok(out)
}

/// Classify what a reconcile observed into divergences, keeping whatever a resolution
/// displaced (C4). Ordinary edits while Resonance runs are not divergences.
pub fn classify(repo: &Repo, r: &dyn GraphRead, o: &Observed<'_>) -> Result<Vec<Divergence>> {
    let mut out = Vec::new();
    let changed_paths: Vec<String> = o.changed.iter().map(|c| c.path.clone()).collect();
    if let Some(mv) = &o.catch_up.head_move {
        let op = mv.operation.clone();
        let evidence = vec![format!(
            "reflog: {op} ({} → {})",
            mv.from
                .as_deref()
                .map(|c| &c[..c.len().min(8)])
                .unwrap_or("?"),
            mv.to
                .as_deref()
                .map(|c| &c[..c.len().min(8)])
                .unwrap_or("?")
        )];
        let own_commit = op.starts_with("commit");
        // What the operation changed, from git itself: the files may already have been
        // ingested by an earlier reconcile that ran before HEAD moved (daemon timing).
        let changed_paths: Vec<String> = match (&mv.from, &mv.to) {
            (Some(from), Some(to)) => repo
                .git(&["diff", "--name-only", from, to])
                .map(|s| {
                    s.lines()
                        .filter(|l| !l.starts_with(".resonance/"))
                        .map(String::from)
                        .collect()
                })
                .unwrap_or(changed_paths),
            _ => changed_paths,
        };
        if !own_commit && !changed_paths.is_empty() {
            let mut d = Divergence::new(
                Class::C1,
                Level::Info,
                format!("`{op}` changed {} file(s)", changed_paths.len()),
            );
            d.evidence = evidence.clone();
            d.resolution = "applied to the graph as git-originated changes; links to changed code were re-checked".into();
            d.paths = shorten(changed_paths.clone());
            out.push(d);
        }
        // C7: the last known commit is gone, or many commits arrived at once.
        if let (Some(from), Some(to)) = (&mv.from, &mv.to) {
            let reachable = repo
                .git(&["cat-file", "-e", &format!("{from}^{{commit}}")])
                .is_ok()
                && repo.git(&["merge-base", "--is-ancestor", from, to]).is_ok();
            let count: usize = repo
                .git(&["rev-list", "--count", &format!("{from}..{to}")])
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if !reachable && !own_commit && !op.starts_with("checkout") {
                let mut d = Divergence::new(
                    Class::C7,
                    Level::Attention,
                    "history was rewritten since Resonance last looked",
                );
                d.evidence = evidence.clone();
                d.resolution = "caught up in bulk at the new HEAD".into();
                out.push(d);
            } else if count > 50 {
                let mut d = Divergence::new(
                    Class::C7,
                    Level::Attention,
                    format!("{count} commits arrived at once"),
                );
                d.evidence = evidence.clone();
                d.resolution = "caught up in bulk at the new HEAD; see the repair list".into();
                out.push(d);
            }
            // C2: commits by others that carried no Resonance deltas.
            if reachable && count > 0 && count <= 500 {
                let log = repo
                    .git(&[
                        "log",
                        "--format=%x01%H%x01%ae%x01%an%x01%s",
                        "--name-only",
                        &format!("{from}..{to}"),
                    ])
                    .unwrap_or_default();
                let mut outside: Vec<(String, String, String)> = Vec::new();
                let mut current: Option<(String, String, String, bool)> = None;
                let mut flush = |c: Option<(String, String, String, bool)>| {
                    if let Some((h, email, name, has_delta)) = c
                        && !has_delta
                        && email != o.me
                    {
                        outside.push((h, name, email));
                    }
                };
                for line in log.lines() {
                    if let Some(rest) = line.strip_prefix('\u{1}') {
                        flush(current.take());
                        let parts: Vec<&str> = rest.split('\u{1}').collect();
                        current = Some((
                            parts.first().unwrap_or(&"").to_string(),
                            parts.get(1).unwrap_or(&"").to_string(),
                            parts.get(2).unwrap_or(&"").to_string(),
                            false,
                        ));
                    } else if line.starts_with(".resonance/deltas/")
                        && let Some(c) = current.as_mut()
                    {
                        c.3 = true;
                    }
                }
                flush(current.take());
                if !outside.is_empty() {
                    let mut names: Vec<String> = outside.iter().map(|o| o.1.clone()).collect();
                    names.sort();
                    names.dedup();
                    let mut d = Divergence::new(
                        Class::C2,
                        Level::Info,
                        format!(
                            "{} commit(s) by {} arrived without Resonance data",
                            outside.len(),
                            names.join(", ")
                        ),
                    );
                    d.evidence = outside
                        .iter()
                        .take(10)
                        .map(|(h, n, e)| format!("{} by {n} <{e}>", &h[..h.len().min(8)]))
                        .collect();
                    d.resolution = "their changes were ingested; links to code they changed are stale or on the repair list".into();
                    out.push(d);
                }
            }
            // C8: this developer's own commits changed governed files and no delta in the
            // range covers them (a hook was skipped). Offline graphs are not users.
            if let Some(d) = own_uncovered(repo, from, to, o)? {
                out.push(d);
            }
        }
        // C5: a merge touched annotated or linked code.
        if op.starts_with("merge") || op.starts_with("pull") {
            let anchored = anchored_paths(r)?;
            let touched: Vec<String> = changed_paths
                .iter()
                .filter(|p| anchored.contains(*p))
                .cloned()
                .collect();
            if !touched.is_empty() {
                let mut d = Divergence::new(
                    Class::C5,
                    Level::Attention,
                    format!("`{op}` changed {} file(s) with code units", touched.len()),
                );
                d.evidence = evidence.clone();
                d.resolution = "git's merged text was taken; anchors were re-located and anything uncertain is on the repair list".into();
                d.paths = shorten(touched);
                out.push(d);
            }
        }
        if mv.detached > 0 {
            let mut d = Divergence::new(
                Class::C7,
                Level::Attention,
                format!(
                    "{} offline delta(s) belonged to rewritten commits that could not be matched",
                    mv.detached
                ),
            );
            d.evidence = evidence;
            d.resolution = "kept in the detached pool; attach them with `resonance recover attach <delta>` or drop them with `resonance recover drop <delta>`".into();
            d.resolved = false;
            out.push(d);
        }
    } else if !o.changed.is_empty() && repo.head_commit().is_some() {
        // C4: a file went back to its committed version, discarding an uncommitted change.
        let candidates: Vec<&crate::ingest::reconcile::ChangedFile> = o
            .changed
            .iter()
            .filter(|c| c.old_blob.is_some() && c.new_blob.is_some())
            .collect();
        if !candidates.is_empty() {
            let mut args = vec!["diff", "--name-only", "HEAD", "--"];
            args.extend(candidates.iter().map(|c| c.path.as_str()));
            let differs: std::collections::BTreeSet<String> = repo
                .git(&args)
                .map(|s| s.lines().map(String::from).collect())
                .unwrap_or_default();
            let stash_evidence = repo.git(&["stash", "list", "-n", "1"]).unwrap_or_default();
            let reflog = crate::history::reflog_subject(repo);
            for c in candidates
                .into_iter()
                .filter(|c| !differs.contains(&c.path))
            {
                let Ok(head) = repo.git_bytes(&["show", &format!("HEAD:{}", c.path)]) else {
                    continue;
                };
                let head_hash = content_hash(&head);
                if c.new_blob.as_deref() != Some(head_hash.as_str())
                    || c.old_blob.as_deref() == Some(head_hash.as_str())
                {
                    continue;
                }
                let Some(old) = r.get_blob(c.old_blob.as_deref().unwrap_or_default())? else {
                    continue;
                };
                let rec = keep(
                    repo,
                    &c.path,
                    Class::C4,
                    "disk",
                    &old,
                    c.new_blob.clone(),
                    "an uncommitted change that went back to the committed version",
                )?;
                let mut d = Divergence::new(
                    Class::C4,
                    Level::Attention,
                    format!("{}: an uncommitted change was discarded", c.path),
                );
                d.evidence.push(format!(
                    "the file now matches HEAD; the last reflog entry is `{reflog}`"
                ));
                if !stash_evidence.is_empty() {
                    d.evidence.push(format!(
                        "newest stash: {stash_evidence} (`git stash pop` brings it back)"
                    ));
                }
                d.resolution = format!(
                    "the graph followed the file; your previous version is kept: `resonance recover undo {}`",
                    rec.id
                );
                d.paths = vec![c.path.clone()];
                d.recovery = Some(rec.id);
                out.push(d);
            }
        }
        // C3: changes found when a daemon starts were made while Resonance was not running.
        // For a developer who has opted in, the graph is the source of truth (FR-012b): the
        // file is restored from the graph and the disk version kept for `recover undo`.
        if o.startup && o.mode.opted_in() {
            let explained: std::collections::BTreeSet<String> =
                out.iter().flat_map(|d| d.paths.clone()).collect();
            for c in o.changed.iter().filter(|c| !explained.contains(&c.path)) {
                if !crate::api::governs(o.config, &c.path) {
                    continue;
                }
                let abs = crate::repo::abs_path(&repo.root, &c.path);
                let disk = std::fs::read(&abs).unwrap_or_default();
                let graph = match &c.old_blob {
                    Some(b) => r.get_blob(b)?,
                    None => None,
                };
                let mut d = Divergence::new(
                    Class::C3,
                    Level::ActionRequired,
                    format!("{}: changed while Resonance was not running", c.path),
                );
                d.evidence
                    .push("found by the start-up reconcile; no git operation explains it".into());
                d.paths = vec![c.path.clone()];
                match graph {
                    Some(bytes) if c.new_blob.is_some() => {
                        let rec = keep(
                            repo,
                            &c.path,
                            Class::C3,
                            "graph",
                            &disk,
                            c.old_blob.clone(),
                            "a change made while Resonance was not running (FR-012b)",
                        )?;
                        write_file(&abs, &bytes)?;
                        d.resolution = format!(
                            "restored the graph's version; your disk version is kept: `resonance recover undo {}` brings it back, then decide: merge, keep, or discard",
                            rec.id
                        );
                        d.recovery = Some(rec.id);
                    }
                    _ => {
                        d.resolution = "the graph has no stored version to restore (a new, large or binary file); the disk version stays: decide whether to keep it".into();
                    }
                }
                out.push(d);
            }
        } else if o.startup {
            let explained: std::collections::BTreeSet<String> =
                out.iter().flat_map(|d| d.paths.clone()).collect();
            let rest: Vec<String> = changed_paths
                .iter()
                .filter(|p| !explained.contains(*p))
                .cloned()
                .collect();
            if !rest.is_empty() {
                let mut d = Divergence::new(
                    Class::C3,
                    Level::Info,
                    format!(
                        "{} file(s) changed while Resonance was not running",
                        rest.len()
                    ),
                );
                d.evidence
                    .push("found by the start-up reconcile; HEAD did not move".into());
                d.resolution = if o.mode.opted_in() {
                    "ingested".into()
                } else {
                    "ingested: offline, git and the files are authoritative (FR-188)".into()
                };
                d.paths = shorten(rest);
                out.push(d);
            }
        }
    }
    if o.catch_up.held {
        let unresolved_c6 = list(r, false)?.iter().any(|d| d.class == Class::C6);
        if !unresolved_c6 {
            let mut d = Divergence::new(
                Class::C6,
                Level::ActionRequired,
                "delta files disappeared from .resonance/ without a git operation",
            );
            d.evidence.push(
                "HEAD did not move; the delta history no longer holds operations the graph has"
                    .into(),
            );
            d.resolution = "nothing was changed; run `resonance rebuild` to accept the change, or restore the files (`resonance backup --restore`, or git)".into();
            out.push(d);
        }
    }
    Ok(out)
}

/// Own commits in `from..to` that changed governed files no delta manifest covers (C8).
fn own_uncovered(
    repo: &Repo,
    from: &str,
    to: &str,
    o: &Observed<'_>,
) -> Result<Option<Divergence>> {
    if !o.mode.opted_in() && o.mode.stage < 3 {
        return Ok(None);
    }
    let changed: Vec<String> = repo
        .git(&["diff", "--name-only", from, to])
        .unwrap_or_default()
        .lines()
        .filter(|p| !p.is_empty() && !p.starts_with(".resonance/"))
        .filter(|p| crate::api::governs(o.config, p))
        .map(String::from)
        .collect();
    if changed.is_empty() {
        return Ok(None);
    }
    let mut covered = std::collections::BTreeSet::new();
    let commits = repo
        .git(&[
            "log",
            "--format=%H",
            &format!("{from}..{to}"),
            "--",
            ".resonance/deltas",
        ])
        .unwrap_or_default();
    for c in commits.lines() {
        let added = repo
            .git(&[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                "-m",
                "--diff-filter=AM",
                c,
                "--",
                ".resonance/deltas",
            ])
            .unwrap_or_default();
        for path in added.lines() {
            if let Ok(bytes) = repo.git_bytes(&["show", &format!("{c}:{path}")])
                && let Ok(d) = crate::history::Delta::parse(&bytes)
            {
                covered.extend(d.manifest.keys().cloned());
            }
        }
    }
    let mut mine = Vec::new();
    for f in changed.into_iter().filter(|f| !covered.contains(f)) {
        let authors = repo
            .git(&["log", "--format=%ae", &format!("{from}..{to}"), "--", &f])
            .unwrap_or_default();
        if authors.lines().any(|e| e == o.me) {
            mine.push(f);
        }
    }
    if mine.is_empty() {
        return Ok(None);
    }
    let mut d = Divergence::new(
        Class::C8,
        Level::ActionRequired,
        format!(
            "{} file(s) changed in your commits with no Resonance manifest",
            mine.len()
        ),
    );
    d.evidence.push(format!(
        "range {}..{}",
        &from[..from.len().min(8)],
        &to[..to.len().min(8)]
    ));
    d.resolution = "commit the open delta that covers these files (`git add .resonance/deltas && git commit`); that resolves this".into();
    d.paths = shorten(mine);
    Ok(Some(d))
}

/// Manifest paths of the deltas committed at `HEAD`.
pub fn committed_manifest_paths(repo: &Repo) -> std::collections::BTreeSet<String> {
    let listed = repo
        .git(&[
            "ls-tree",
            "-r",
            "--name-only",
            "HEAD",
            "--",
            ".resonance/deltas",
        ])
        .unwrap_or_default();
    paths_in_manifests(repo, listed.lines(), |path| format!("HEAD:{path}"))
}

/// Manifest paths of deltas staged in the index. A follow-up commit of the open delta
/// is the repair for C8 and must not be blocked by the C8 it repairs (FR-203).
pub fn staged_manifest_paths(repo: &Repo) -> std::collections::BTreeSet<String> {
    let listed = repo
        .git(&[
            "diff",
            "--cached",
            "--name-only",
            "--diff-filter=AM",
            "--",
            ".resonance/deltas",
        ])
        .unwrap_or_default();
    paths_in_manifests(repo, listed.lines(), |path| format!(":{path}"))
}

fn paths_in_manifests<'a>(
    repo: &Repo,
    paths: impl Iterator<Item = &'a str>,
    spec: impl Fn(&str) -> String,
) -> std::collections::BTreeSet<String> {
    let mut covered = std::collections::BTreeSet::new();
    for path in paths {
        let Ok(bytes) = repo.git_bytes(&["show", &spec(path)]) else {
            continue;
        };
        let Ok(delta) = crate::history::Delta::parse(&bytes) else {
            continue;
        };
        covered.extend(delta.manifest.keys().cloned());
    }
    covered
}

/// A C8 whose files a committed delta now covers is the repaired bypass. Resolve it.
pub fn resolve_repaired_c8(repo: &Repo, store: &mut dyn GraphStore) -> Result<()> {
    let covered = committed_manifest_paths(repo);
    if covered.is_empty() {
        return Ok(());
    }
    let open = list(&*store, false)?;
    for d in open.into_iter().filter(|d| d.class == Class::C8) {
        let paths: Vec<&str> = d
            .paths
            .iter()
            .map(String::as_str)
            .filter(|p| !p.starts_with('…'))
            .collect();
        if !paths.is_empty() && paths.iter().all(|p| covered.contains(*p)) {
            resolve(store, &d.id, "a later delta manifest covers these files")?;
        }
    }
    Ok(())
}

/// Inverse operations for a delta that a revert deleted after the snapshot had folded it
/// (FR-159). Stored beside recovery records and applied only by `recover inverse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InverseOffer {
    delta_id: String,
    divergence_id: String,
    ops: Vec<Op>,
}

fn inverse_path(repo: &Repo, delta_id: &str) -> PathBuf {
    repo.recovery_dir().join(format!("inverse-{delta_id}.json"))
}

fn inverse_ops(ops: &[Op]) -> Vec<Op> {
    let mut out = Vec::new();
    let change_set = new_id();
    let actor = ops::Actor {
        kind: ops::ActorKind::Tool,
        id: "resonance recover".into(),
    };
    // Reverse order: an edge added after its nodes is removed before those nodes retire.
    for op in ops.iter().rev() {
        let (kind_s, payload) = match op.kind.as_str() {
            kind::NODE_CREATE => (kind::NODE_RETIRE, json!({"id": op.payload["id"]})),
            kind::EDGE_ADD => (kind::EDGE_REMOVE, json!({"id": op.payload["id"]})),
            kind::NODE_SET => {
                let Some(prev) = op
                    .payload
                    .get("prev")
                    .or_else(|| op.payload.get("previous"))
                else {
                    continue;
                };
                (
                    kind::NODE_SET,
                    json!({"id": op.payload["id"], "key": op.payload["key"], "value": prev}),
                )
            }
            _ => continue,
        };
        if payload["id"].as_str().unwrap_or("").is_empty() && payload.get("key").is_none() {
            continue;
        }
        out.push(Op {
            op_id: new_id(),
            kind: kind_s.to_string(),
            actor: actor.clone(),
            hlc: hlc::rfc3339(hlc::now_ms()),
            change_set: change_set.clone(),
            schema: op.schema,
            payload,
        });
    }
    out
}

/// Notice a revert that deletes a delta the snapshot already folded. Does not apply anything.
pub fn notice_reverted_deltas(
    repo: &Repo,
    store: &dyn crate::store::GraphStore,
) -> Result<Vec<Divergence>> {
    if !repo.snapshot_path().is_file() {
        return Ok(Vec::new());
    }
    let snap = store.read_snapshot(&repo.snapshot_path())?;
    let folded: std::collections::BTreeSet<&str> =
        snap.meta.folded_deltas.iter().map(String::as_str).collect();
    let deleted = repo
        .git(&[
            "diff",
            "--name-only",
            "--diff-filter=D",
            "HEAD^",
            "HEAD",
            "--",
            ".resonance/deltas",
        ])
        .unwrap_or_default();
    let mut out = Vec::new();
    for path in deleted.lines() {
        let Some(id) = crate::history::delta::id_of(std::path::Path::new(path)) else {
            continue;
        };
        if !folded.contains(id.as_str()) {
            continue;
        }
        let Ok(bytes) = repo.git_bytes(&["show", &format!("HEAD^:{path}")]) else {
            continue;
        };
        let Ok(delta) = crate::history::Delta::parse(&bytes) else {
            continue;
        };
        let ops = inverse_ops(&delta.ops);
        if ops.is_empty() {
            continue;
        }
        let mut d = Divergence::new(
            Class::C7,
            Level::ActionRequired,
            format!("reverted delta {id} is already in the snapshot"),
        );
        d.evidence.push(format!(
            "deleted {path}; {} inverse operation(s) are offered and not applied",
            ops.len()
        ));
        d.resolution = format!("run `resonance recover inverse {id}` to apply the inverse");
        d.resolved = false;
        let offer = InverseOffer {
            delta_id: id.clone(),
            divergence_id: d.id.clone(),
            ops,
        };
        let file = inverse_path(repo, &id);
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir).map_err(|e| Error::io("creating the recovery directory", e))?;
        }
        fs::write(&file, serde_json::to_string(&offer)?)
            .map_err(|e| Error::io("writing inverse operations", e))?;
        out.push(d);
    }
    Ok(out)
}

/// The inverse operations offered for `delta_id` (a unique prefix is enough), and the
/// offer file. The caller deletes the file after the operations apply.
pub fn take_inverse(repo: &Repo, delta_id: &str) -> Result<(String, Vec<Op>, PathBuf)> {
    let dir = repo.recovery_dir();
    let mut matches: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("inverse-") && n.ends_with(".json"))
                })
                .collect()
        })
        .unwrap_or_default();
    matches.retain(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.trim_start_matches("inverse-").starts_with(delta_id))
    });
    if matches.len() != 1 {
        return Err(if matches.is_empty() {
            Error::NotFound(format!("no inverse is offered for delta {delta_id}"))
        } else {
            Error::Invalid(format!(
                "{delta_id} matches several inverse offers; give more of the delta ID"
            ))
        });
    }
    let path = matches.into_iter().next().expect("one");
    let text = fs::read_to_string(&path).map_err(|e| Error::io("reading inverse operations", e))?;
    let offer: InverseOffer = serde_json::from_str(&text)?;
    Ok((offer.divergence_id, offer.ops, path))
}

/// Bytes a recovery record kept aside, when the divergence names one.
pub fn displaced_bytes(repo: &Repo, recovery_id: &str) -> Result<Vec<u8>> {
    let rec = find_record(repo, recovery_id)?;
    fs::read(record_dir(repo, &rec.id).join("displaced"))
        .map_err(|e| Error::io("reading a recovery copy", e))
}

/// Three-way merge of file bytes. Conflict markers are part of the result.
pub fn merge_three(current: &[u8], base: &[u8], other: &[u8]) -> Result<Vec<u8>> {
    let dir = std::env::temp_dir().join(format!("resonance-merge-{}", new_id()));
    fs::create_dir_all(&dir).map_err(|e| Error::io("creating a merge directory", e))?;
    let ours = dir.join("ours");
    let base_path = dir.join("base");
    let theirs = dir.join("theirs");
    fs::write(&ours, current).map_err(|e| Error::io("writing a merge side", e))?;
    fs::write(&base_path, base).map_err(|e| Error::io("writing a merge side", e))?;
    fs::write(&theirs, other).map_err(|e| Error::io("writing a merge side", e))?;
    let out = std::process::Command::new("git")
        .args(["merge-file", "-p"])
        .arg(&ours)
        .arg(&base_path)
        .arg(&theirs)
        .output()
        .map_err(|e| Error::Git(format!("could not run git merge-file: {e}")))?;
    let _ = fs::remove_dir_all(&dir);
    // Exit status is the conflict count. The merged text is still on standard output.
    if out.stdout.is_empty() && !out.status.success() {
        return Err(Error::Git(format!(
            "git merge-file failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(out.stdout)
}
