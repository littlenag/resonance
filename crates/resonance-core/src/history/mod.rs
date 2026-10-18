//! History: deltas, snapshots and the fold (§8.6.1, §9.6, MVP-025–MVP-030).
//!
//! Every authored change is an operation in the live store. The operations since the last
//! sealed delta form the **open delta**, which is written to a file after every change set
//! (MVP-033), minus file-content operations, which become its file manifest (FR-064). Deltas
//! are sealed when a commit takes them: in a shared repository when the commit contains the
//! delta file, in offline mode at every commit, which the delta is then associated with
//! (MVP-030). The graph for a checkout is the deterministic fold of the snapshot and the
//! deltas in effect at `HEAD` (FR-150, FR-151); when that set changes, the live store is
//! re-folded and then settled against the files on disk.

pub mod canonical;
pub mod compact;
pub mod delta;
pub mod fold;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{content_hash, new_id};
use crate::ops::{Op, hlc, kind};
use crate::repo::Repo;
use crate::repo::config::Config;
use crate::store::{GraphRead, GraphScope, GraphStore, SCHEMA_VERSION};

pub use delta::{Delta, DeltaHeader};

/// Metadata keys of the history (local state, never committed).
pub mod keys {
    /// The open delta's record.
    pub const OPEN: &str = "history.open";
    /// Prefix of sealed delta records: `delta:<id>`.
    pub const DELTA_PREFIX: &str = "delta:";
    /// The last operation sequence number appended to the journal.
    pub const JOURNAL_SEQ: &str = "history.journalSeq";
    /// `HEAD` when the history was last caught up.
    pub const HEAD: &str = "git.head";
    /// The signature of the history inputs at the last catch-up.
    pub const SIGNATURE: &str = "history.signature";
    /// Operations a fold could not apply, for `doctor`.
    pub const SKIPPED: &str = "history.skipped";
    /// A refused re-fold, waiting for `resonance rebuild` (C6).
    pub const HELD: &str = "history.held";
}

/// Where a delta lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Location {
    /// `.resonance/deltas/`: committed from stage 2, excluded in offline mode.
    Deltas,
    /// `.resonance/local/private/`: an offline layer over a shared graph (FR-187).
    Private,
}

impl Location {
    pub fn dir(self, repo: &Repo) -> PathBuf {
        match self {
            Location::Deltas => repo.deltas_dir(),
            Location::Private => repo.private_dir(),
        }
    }
}

/// The open delta (MVP-033).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDelta {
    pub id: String,
    pub location: Location,
    /// Operations with a greater sequence number belong to this delta.
    pub after_seq: u64,
    /// File-content operations with a greater sequence number go into its manifest.
    pub manifest_after: u64,
    pub epoch: String,
    pub branch: String,
    pub writer: String,
    /// Manifest entries recorded before a re-fold cleared the file operations they came from.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub carried: BTreeMap<String, delta::ManifestEntry>,
}

/// A sealed delta's local record: in offline mode, the commit it belongs to (MVP-030).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaRecord {
    pub location: Location,
    pub commit: Option<String>,
    pub sealed_at: String,
    /// The commit was rewritten and could not be re-associated: in the detached pool.
    #[serde(default)]
    pub detached: bool,
}

/// How this working tree takes part (§8.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Participation {
    /// Opted in to a shared graph (stage 2).
    User,
    /// Offline: a private graph (stage 1), or a private layer over a shared graph (FR-187).
    Offline,
}

impl Participation {
    pub fn as_str(self) -> &'static str {
        match self {
            Participation::User => "user",
            Participation::Offline => "offline",
        }
    }
}

/// The repository's stage and this working tree's participation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mode {
    pub stage: u8,
    pub participation: Participation,
}

impl Mode {
    /// Whether deltas in `.resonance/deltas/` are committed (stage 2 and up).
    pub fn shared(self) -> bool {
        self.stage >= 2
    }

    /// Whether this developer has opted in (FR-203).
    pub fn opted_in(self) -> bool {
        self.shared() && self.participation == Participation::User
    }

    /// Where this working tree's open delta goes.
    pub fn open_location(self) -> Location {
        if self.shared() && self.participation == Participation::Offline {
            Location::Private
        } else {
            Location::Deltas
        }
    }

    /// Whether deltas in `location` are sealed per commit and associated with it (MVP-030).
    pub fn associates(self, location: Location) -> bool {
        !self.shared() || location == Location::Private
    }
}

/// The local git configuration key that records participation (MVP-033).
pub const PARTICIPATION_KEY: &str = "resonance.participation";

/// This working tree's mode.
pub fn mode(repo: &Repo, config: &Config) -> Mode {
    let stage = config.repository.stage.max(1);
    let participation = match repo
        .git(&["config", "--local", "--get", PARTICIPATION_KEY])
        .ok()
        .as_deref()
    {
        Some("user") if stage >= 2 => Participation::User,
        _ => Participation::Offline,
    };
    Mode {
        stage,
        participation,
    }
}

pub(crate) fn now() -> String {
    hlc::rfc3339(hlc::now_ms())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| Error::io(format!("writing {}", tmp.display()), e))?;
    fs::rename(&tmp, path).map_err(|e| Error::io(format!("writing {}", path.display()), e))
}

pub fn read_open(r: &dyn GraphRead) -> Result<Option<OpenDelta>> {
    Ok(match r.get_meta(keys::OPEN)? {
        Some(s) => Some(serde_json::from_str(&s)?),
        None => None,
    })
}

pub fn records(r: &dyn GraphRead) -> Result<BTreeMap<String, DeltaRecord>> {
    let mut out = BTreeMap::new();
    for (k, v) in r.meta_prefix(keys::DELTA_PREFIX)? {
        if let Ok(rec) = serde_json::from_str(&v) {
            out.insert(k[keys::DELTA_PREFIX.len()..].to_string(), rec);
        }
    }
    Ok(out)
}

/// The path of a delta file.
pub fn delta_path(repo: &Repo, location: Location, id: &str) -> PathBuf {
    location
        .dir(repo)
        .join(format!("{id}.{}", delta::EXTENSION))
}

/// The current branch name, for delta headers.
fn branch(repo: &Repo) -> String {
    repo.git(&["symbolic-ref", "--quiet", "--short", "HEAD"])
        .unwrap_or_else(|_| "HEAD".into())
}

/// The snapshot epoch of a store's history, or empty when there is no snapshot.
fn epoch(repo: &Repo, store: &dyn GraphStore) -> String {
    if repo.snapshot_path().is_file() {
        store
            .read_snapshot(&repo.snapshot_path())
            .map(|s| s.meta.epoch)
            .unwrap_or_default()
    } else {
        String::new()
    }
}

/// A new open delta starting after `seq`.
fn new_open(repo: &Repo, store: &dyn GraphStore, mode: Mode, writer: &str, seq: u64) -> OpenDelta {
    OpenDelta {
        id: new_id(),
        location: mode.open_location(),
        after_seq: seq,
        manifest_after: seq,
        epoch: epoch(repo, store),
        branch: branch(repo),
        writer: writer.to_string(),
        carried: BTreeMap::new(),
    }
}

/// Build the open delta's content from the live store.
pub fn open_delta_content(r: &dyn GraphRead, open: &OpenDelta) -> Result<Delta> {
    let mut ops = Vec::new();
    let mut manifest: BTreeMap<String, delta::ManifestEntry> = open.carried.clone();
    for rec in r.ops_after(open.after_seq.min(open.manifest_after), usize::MAX)? {
        if delta::is_file_content_op(&rec.op.kind) {
            if rec.seq <= open.manifest_after {
                continue;
            }
            let path = rec.op.payload["path"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let prev = rec.op.payload["prev"].as_str().map(String::from);
            let new = if rec.op.kind == kind::FILE_PUT {
                rec.op.payload["blob"].as_str().map(String::from)
            } else {
                None
            };
            manifest
                .entry(path)
                .and_modify(|e| e.1 = new.clone())
                .or_insert((prev, new));
        } else if rec.seq > open.after_seq {
            ops.push(rec.op);
        }
    }
    // A file that ends where it started was not changed.
    manifest.retain(|_, (a, b)| a != b);
    Ok(Delta {
        header: DeltaHeader {
            resonance: "delta".into(),
            format: delta::DELTA_FORMAT,
            id: open.id.clone(),
            schema: SCHEMA_VERSION,
            writer: open.writer.clone(),
            version: crate::api::VERSION.into(),
            epoch: open.epoch.clone(),
            branch: open.branch.clone(),
        },
        ops,
        manifest,
    })
}

/// What [`sync`] wrote.
#[derive(Debug, Clone, Default)]
pub struct Synced {
    /// The open delta's file, when it has content.
    pub open_path: Option<PathBuf>,
    /// The content hash of what was written, for self-write suppression.
    pub hash: Option<String>,
}

/// Write the open delta and append to the journal. Call after every change set (MVP-033,
/// FR-194). `writer` names this working tree's actor in new delta headers.
pub fn sync(repo: &Repo, store: &mut dyn GraphStore, mode: Mode, writer: &str) -> Result<Synced> {
    let mut open = match read_open(&*store)? {
        Some(o) => o,
        None => {
            let o = new_open(repo, &*store, mode, writer, 0);
            let text = serde_json::to_string(&o)?;
            store.write(GraphScope::Primary, &mut |w| w.set_meta(keys::OPEN, &text))?;
            o
        }
    };
    if open.location != mode.open_location() {
        // The mode changed (share, join, leave): continue in the new place.
        let old = delta_path(repo, open.location, &open.id);
        let new = delta_path(repo, mode.open_location(), &open.id);
        if old.is_file() && !new.exists() {
            if let Some(d) = new.parent() {
                fs::create_dir_all(d).map_err(|e| Error::io("creating the delta directory", e))?;
            }
            fs::rename(&old, &new).map_err(|e| Error::io("moving the open delta", e))?;
        }
        open.location = mode.open_location();
        let text = serde_json::to_string(&open)?;
        store.write(GraphScope::Primary, &mut |w| w.set_meta(keys::OPEN, &text))?;
    }
    let content = open_delta_content(&*store, &open)?;
    let path = delta_path(repo, open.location, &open.id);
    let mut synced = Synced::default();
    if !content.ops.is_empty() || !content.manifest.is_empty() {
        let bytes = content.to_bytes()?;
        let hash = content_hash(&bytes);
        let current = fs::read(&path).ok().map(|b| content_hash(&b));
        if current.as_deref() != Some(hash.as_str()) {
            write_atomic(&path, &bytes)?;
        }
        synced.open_path = Some(path);
        synced.hash = Some(hash);
    } else if path.is_file() {
        let _ = fs::remove_file(&path);
    }
    journal(repo, store)?;
    Ok(synced)
}

/// Append operations the journal has not seen (FR-194). The journal is independent of the
/// working tree, so graph changes that disappear from it can be recovered.
fn journal(repo: &Repo, store: &mut dyn GraphStore) -> Result<()> {
    let after: u64 = store
        .get_meta(keys::JOURNAL_SEQ)?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let recs = store.ops_after(after, usize::MAX)?;
    let Some(last) = recs.last().map(|r| r.seq) else {
        return Ok(());
    };
    let dir = repo.journal_dir();
    fs::create_dir_all(&dir).map_err(|e| Error::io("creating the journal", e))?;
    let day = &now()[..10];
    let path = dir.join(format!("{day}.jsonl"));
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| Error::io(format!("opening {}", path.display()), e))?;
    let mut out = String::new();
    for r in &recs {
        out.push_str(&canonical::to_string(&r.op)?);
        out.push('\n');
    }
    f.write_all(out.as_bytes())
        .map_err(|e| Error::io(format!("writing {}", path.display()), e))?;
    store.write(GraphScope::Primary, &mut |w| {
        w.set_meta(keys::JOURNAL_SEQ, &last.to_string())
    })
}

/// Every operation in the journal, oldest first, deduplicated by ID.
pub fn journal_ops(repo: &Repo) -> Result<Vec<Op>> {
    let mut files: Vec<PathBuf> = match fs::read_dir(repo.journal_dir()) {
        Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.path())).collect(),
        Err(_) => return Ok(Vec::new()),
    };
    files.sort();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for f in files {
        let text = fs::read_to_string(&f).unwrap_or_default();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if let Ok(op) = serde_json::from_str::<Op>(line)
                && seen.insert(op.op_id.clone())
            {
                out.push(op);
            }
        }
    }
    Ok(out)
}

/// A cheap signature of the history inputs, so that catch-up is skipped when nothing that
/// could change the fold has changed: `HEAD`, the mode, the snapshot and the delta files
/// other than the open one.
pub fn signature(repo: &Repo, mode: Mode, open_id: Option<&str>) -> String {
    let mut s = format!(
        "head={};stage={};p={};",
        repo.head_commit().unwrap_or_default(),
        mode.stage,
        mode.participation.as_str()
    );
    let stat = |p: &Path| -> String {
        fs::metadata(p)
            .map(|m| {
                let t = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                format!("{}:{t}", m.len())
            })
            .unwrap_or_else(|_| "-".into())
    };
    s.push_str(&format!("snapshot={};", stat(&repo.snapshot_path())));
    for loc in [Location::Deltas, Location::Private] {
        let mut names: Vec<PathBuf> = fs::read_dir(loc.dir(repo))
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        names.sort();
        for p in names {
            if delta::id_of(&p).is_some_and(|id| Some(id.as_str()) != open_id) {
                s.push_str(&format!("{}={};", p.display(), stat(&p)));
            }
        }
    }
    content_hash(s.as_bytes())
}

/// What happened when `HEAD` moved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadMove {
    pub from: Option<String>,
    pub to: Option<String>,
    /// The reflog subject of the move, for example `commit: …` or `checkout: …`.
    pub operation: String,
    /// The delta sealed by this move, if any.
    pub sealed: Option<String>,
    /// Offline deltas moved to rewritten commits.
    pub reassociated: usize,
    /// Offline deltas that could not be re-associated (the detached pool).
    pub detached: usize,
}

/// The oldest commit created (by `commit`, `commit (amend)`, `commit (merge)` and so on)
/// since `HEAD` was `from`, from the reflog. `None` when no commit was made, or when `from`
/// is not in the recent reflog (then the newest commit, if the newest move made one).
pub fn first_commit_since(repo: &Repo, from: Option<&str>) -> Option<String> {
    let log = repo
        .git(&["reflog", "-n", "500", "--format=%H%x01%gs"])
        .ok()?;
    let mut newest_commit_entry: Option<String> = None;
    let mut oldest: Option<String> = None;
    for (i, line) in log.lines().enumerate() {
        let (hash, subject) = line.split_once('\u{1}')?;
        if from == Some(hash) && i > 0 {
            return oldest;
        }
        if from == Some(hash) && i == 0 {
            return None;
        }
        if subject.starts_with("commit") {
            if i == 0 {
                newest_commit_entry = Some(hash.to_string());
            }
            oldest = Some(hash.to_string());
        }
    }
    newest_commit_entry
}

/// The newest reflog subject of `HEAD`.
pub fn reflog_subject(repo: &Repo) -> String {
    repo.git(&["reflog", "-1", "--format=%gs"])
        .unwrap_or_default()
}

/// Commits reachable from `HEAD`, oldest first in topological order, with their positions.
pub fn ancestry(repo: &Repo) -> HashMap<String, usize> {
    match repo.git(&["rev-list", "--topo-order", "--reverse", "HEAD"]) {
        Ok(out) => out
            .lines()
            .enumerate()
            .map(|(i, c)| (c.to_string(), i))
            .collect(),
        Err(_) => HashMap::new(),
    }
}

/// React to `HEAD` moving from `from`: seal the open delta when a commit took it, and keep
/// offline deltas attached to rewritten commits (MVP-030, MVP-033).
pub fn on_head_moved(
    repo: &Repo,
    store: &mut dyn GraphStore,
    mode: Mode,
    writer: &str,
    from: Option<&str>,
) -> Result<HeadMove> {
    let to = repo.head_commit();
    let operation = reflog_subject(repo);
    let mut mv = HeadMove {
        from: from.map(String::from),
        to: to.clone(),
        operation: operation.clone(),
        ..Default::default()
    };
    let Some(head) = to else {
        return Ok(mv);
    };
    let open = read_open(&*store)?;
    // The first commit made since `from`: uncommitted graph changes belong to it, as
    // working-tree changes do, even when later commits or checkouts followed it.
    let first_commit = first_commit_since(repo, from);
    let is_commit = first_commit.is_some();
    if let Some(open) = &open {
        if mode.associates(open.location) {
            if let Some(c) = &first_commit {
                mv.sealed = seal_offline(repo, store, mode, writer, open, c)?;
            }
        } else {
            mv.sealed = seal_shared(repo, store, mode, writer, open, &head)?;
        }
    }
    // Rewrites of offline history (MVP-030).
    if operation.starts_with("commit (amend)")
        && let Ok(previous) = repo.git(&["rev-parse", "HEAD@{1}"])
    {
        let recs = records(&*store)?;
        let mut moved = Vec::new();
        for (id, rec) in recs {
            if rec.commit.as_deref() == Some(previous.as_str()) && mode.associates(rec.location) {
                moved.push((
                    id,
                    DeltaRecord {
                        commit: Some(head.clone()),
                        detached: false,
                        ..rec
                    },
                ));
            }
        }
        mv.reassociated += moved.len();
        put_records(store, &moved)?;
    }
    if operation.starts_with("rebase") || operation.starts_with("reset") || is_commit {
        let (re, det) = reassociate(repo, store, mode)?;
        mv.reassociated += re;
        mv.detached += det;
    }
    Ok(mv)
}

fn put_records(store: &mut dyn GraphStore, recs: &[(String, DeltaRecord)]) -> Result<()> {
    if recs.is_empty() {
        return Ok(());
    }
    let mut texts = Vec::new();
    for (id, rec) in recs {
        texts.push((
            format!("{}{id}", keys::DELTA_PREFIX),
            serde_json::to_string(rec)?,
        ));
    }
    store.write(GraphScope::Primary, &mut |w| {
        for (k, v) in &texts {
            w.set_meta(k, v)?;
        }
        Ok(())
    })
}

/// Offline: the open delta becomes a sealed delta of `head`, and a new one opens.
fn seal_offline(
    repo: &Repo,
    store: &mut dyn GraphStore,
    mode: Mode,
    writer: &str,
    open: &OpenDelta,
    head: &str,
) -> Result<Option<String>> {
    let content = open_delta_content(&*store, open)?;
    let last = store.last_op_seq()?;
    let next = new_open(repo, &*store, mode, writer, last);
    let next_text = serde_json::to_string(&next)?;
    let sealed = if content.ops.is_empty() {
        // Nothing authored: drop the manifest-only delta rather than keep an empty file.
        let _ = fs::remove_file(delta_path(repo, open.location, &open.id));
        None
    } else {
        write_atomic(
            &delta_path(repo, open.location, &open.id),
            &content.to_bytes()?,
        )?;
        Some(open.id.clone())
    };
    let rec = sealed.as_ref().map(|_| {
        serde_json::to_string(&DeltaRecord {
            location: open.location,
            commit: Some(head.to_string()),
            sealed_at: now(),
            detached: false,
        })
    });
    let rec = rec.transpose()?;
    store.write(GraphScope::Primary, &mut |w| {
        if let Some(rec) = &rec {
            w.set_meta(&format!("{}{}", keys::DELTA_PREFIX, open.id), rec)?;
        }
        w.set_meta(keys::OPEN, &next_text)
    })?;
    Ok(sealed)
}

/// Shared: when `HEAD`'s tree contains the open delta, the committed version is sealed. Any
/// operations written after it move to a new open delta (MVP-033).
fn seal_shared(
    repo: &Repo,
    store: &mut dyn GraphStore,
    mode: Mode,
    writer: &str,
    open: &OpenDelta,
    head: &str,
) -> Result<Option<String>> {
    let rel = format!(".resonance/deltas/{}.{}", open.id, delta::EXTENSION);
    let Ok(committed) = repo.git_bytes(&["show", &format!("HEAD:{rel}")]) else {
        return Ok(None);
    };
    let committed_delta = Delta::parse(&committed)?;
    let ids: BTreeSet<&str> = committed_delta
        .ops
        .iter()
        .map(|o| o.op_id.as_str())
        .collect();
    let recs = store.ops_after(open.after_seq.min(open.manifest_after), usize::MAX)?;
    let max_committed = recs
        .iter()
        .filter(|r| ids.contains(r.op.op_id.as_str()))
        .map(|r| r.seq)
        .max();
    let manifest_last = recs
        .iter()
        .filter(|r| delta::is_file_content_op(&r.op.kind))
        .filter(|r| {
            r.op.payload["path"]
                .as_str()
                .is_some_and(|p| committed_delta.manifest.contains_key(p))
        })
        .map(|r| r.seq)
        .max();
    let boundary = max_committed
        .into_iter()
        .chain(manifest_last)
        .max()
        .unwrap_or(open.after_seq);
    // The working file goes back to what was committed; later operations reopen.
    let path = delta_path(repo, Location::Deltas, &open.id);
    write_atomic(&path, &committed)?;
    let mut next = new_open(repo, &*store, mode, writer, boundary);
    next.manifest_after = boundary.max(open.manifest_after);
    // Files in the sealed manifest that the commit did not take stay changed: they remain
    // covered for the commit that does take them.
    let still: std::collections::BTreeSet<String> = repo
        .git(&["diff", "--name-only", "HEAD"])
        .map(|s| s.lines().map(String::from).collect())
        .unwrap_or_default();
    let current = open_delta_content(&*store, open)?;
    for (path, entry) in current.manifest {
        if still.contains(&path) {
            next.carried.insert(path, entry);
        }
    }
    let next_text = serde_json::to_string(&next)?;
    let rec = serde_json::to_string(&DeltaRecord {
        location: Location::Deltas,
        commit: Some(head.to_string()),
        sealed_at: now(),
        detached: false,
    })?;
    store.write(GraphScope::Primary, &mut |w| {
        w.set_meta(&format!("{}{}", keys::DELTA_PREFIX, open.id), &rec)?;
        w.set_meta(keys::OPEN, &next_text)
    })?;
    Ok(Some(open.id.clone()))
}

/// Offline deltas whose commits are no longer ancestors of `HEAD` and no longer reachable from
/// any branch are re-associated by patch ID with a commit of `HEAD`, or else detached.
fn reassociate(repo: &Repo, store: &mut dyn GraphStore, mode: Mode) -> Result<(usize, usize)> {
    let recs = records(&*store)?;
    let ancestors = ancestry(repo);
    let mut lost: Vec<(String, DeltaRecord)> = recs
        .into_iter()
        .filter(|(_, r)| mode.associates(r.location) && !r.detached)
        .filter(|(_, r)| {
            r.commit
                .as_ref()
                .is_some_and(|c| !ancestors.contains_key(c))
        })
        .filter(|(_, r)| {
            // Still on some branch: it belongs to another branch, not to a rewrite.
            let c = r.commit.as_deref().unwrap_or_default();
            repo.git(&["branch", "--all", "--contains", c, "--format=%(refname)"])
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
        })
        .collect();
    if lost.is_empty() {
        return Ok((0, 0));
    }
    let recent: Vec<String> = repo
        .git(&["rev-list", "-n", "500", "HEAD"])
        .map(|s| s.lines().map(String::from).collect())
        .unwrap_or_default();
    let mut by_patch: HashMap<String, String> = HashMap::new();
    for c in &recent {
        if let Some(p) = patch_id(repo, c) {
            by_patch.entry(p).or_insert_with(|| c.clone());
        }
    }
    let (mut re, mut det) = (0, 0);
    for (_, rec) in lost.iter_mut() {
        let old = rec.commit.clone().unwrap_or_default();
        match patch_id(repo, &old).and_then(|p| by_patch.get(&p).cloned()) {
            Some(new) => {
                rec.commit = Some(new);
                re += 1;
            }
            None => {
                rec.detached = true;
                det += 1;
            }
        }
    }
    put_records(store, &lost)?;
    Ok((re, det))
}

/// `git patch-id --stable` of one commit.
fn patch_id(repo: &Repo, commit: &str) -> Option<String> {
    let diff = repo.git_bytes(&["show", "--format=", commit]).ok()?;
    let out = repo.git_stdin(&["patch-id", "--stable"], &diff).ok()?;
    out.split_whitespace().next().map(String::from)
}

/// The deltas in the detached pool (MVP-030).
pub fn detached(store: &dyn GraphRead) -> Result<Vec<(String, DeltaRecord)>> {
    Ok(records(store)?
        .into_iter()
        .filter(|(_, r)| r.detached)
        .collect())
}

/// Find a delta file by ID in either location.
pub fn find_delta(repo: &Repo, id: &str) -> Option<PathBuf> {
    [Location::Deltas, Location::Private]
        .into_iter()
        .map(|l| delta_path(repo, l, id))
        .find(|p| p.is_file())
}
