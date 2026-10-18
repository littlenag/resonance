//! Divergence, recovery and counters (§8.18, MVP-031, MVP-032, MVP-039).
//!
//! Every disagreement between git, the working tree and the graph is classified from git
//! evidence into the classes C1–C9 of §8.18 and recorded with its level and resolution
//! (FR-190, FR-198). Resolutions act automatically only when nothing is lost: whatever they
//! displace is kept under `.resonance/local/recovery/<id>/`, and `resonance recover undo <id>`
//! puts it back (FR-192). Counters behind `resonance value` are kept the same way, locally.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{content_hash, new_id};
use crate::ops::hlc;
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
        if o.startup {
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
