//! The shared graph and opting in (§8.6.1, §8.16, §8.17, MVP-033–MVP-037): `share`, joining,
//! `leave`, `unshare`, compaction, hooks and drivers, `check`, `review` and `show-delta`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Engine, VERSION};
use crate::error::{Error, Result};
use crate::history::{
    self, DeltaRecord, Location, PARTICIPATION_KEY, compact, delta, keys, read_open,
};
use crate::hooks::{self, HookChange};
use crate::ingest::ReconcileOptions;
use crate::model::{NodeType, new_id};
use crate::ops::{Op, kind};
use crate::recovery::{self, Class, Level};
use crate::repo::config::{self, Config};
use crate::repo::exclude;
use crate::store::{GraphScope, NodeQuery};

pub const README: &str = include_str!("../../assets/resonance-readme.md");
pub const GITIGNORE: &str = include_str!("../../assets/gitignore");
pub const GITATTRIBUTES: &str = include_str!("../../assets/gitattributes");

/// One finding of `check` (FR-221, MVP-036).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub code: String,
    /// `error`, `attention` or `info`.
    pub severity: String,
    pub message: String,
    pub fix: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

impl Finding {
    pub(crate) fn new(
        code: &str,
        severity: &str,
        message: impl Into<String>,
        fix: impl Into<String>,
    ) -> Finding {
        Finding {
            code: code.into(),
            severity: severity.into(),
            message: message.into(),
            fix: fix.into(),
            paths: Vec::new(),
        }
    }
    pub(crate) fn with_paths(mut self, paths: Vec<String>) -> Finding {
        self.paths = paths;
        self
    }
}

/// What `check` found.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub scope: String,
    pub findings: Vec<Finding>,
    pub errors: usize,
    /// Files `check --staged` staged itself (the open delta).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub staged: Vec<String>,
}

impl CheckReport {
    fn finish(mut self) -> CheckReport {
        self.errors = self
            .findings
            .iter()
            .filter(|f| f.severity == "error")
            .count();
        self
    }
}

/// What `share` would do or did (FR-185, MVP-037).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareView {
    pub applied: bool,
    /// Items that will be committed, by type.
    pub counts: BTreeMap<String, usize>,
    /// Every annotation's text: the privacy review (NFR-012).
    pub annotations: Vec<Value>,
    /// Annotations kept local (`--keep-local`).
    pub kept_local: Vec<String>,
    pub operations: usize,
    /// New files to commit.
    pub files: Vec<String>,
    pub hooks: Vec<HookChange>,
}

/// What joining did (FR-172, FR-186).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinView {
    /// Offline operations merged into the shared graph as a new delta.
    pub merged_operations: usize,
    pub merged_deltas: Vec<String>,
    pub hooks: Vec<HookChange>,
}

/// What `leave` or `unshare` did.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaveView {
    pub export: Option<String>,
    pub hooks: Vec<HookChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub staged_removals: Vec<String>,
    /// The agent kit's working-tree parts, removed (MVP-037).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kit: Vec<crate::agent::KitStep>,
}

/// A delta rendered for people (`show-delta`, `review`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewView {
    pub range: String,
    pub deltas: Vec<String>,
    pub intent_added: Vec<String>,
    pub intent_changed: Vec<String>,
    pub intent_retired: Vec<String>,
    pub links_added: Vec<String>,
    pub links_removed: Vec<String>,
    pub link_status_changes: usize,
    pub annotations: Vec<String>,
    pub identity_operations: usize,
    pub conflicts: Vec<String>,
    pub compacted: bool,
    pub files_outside_resonance: Vec<String>,
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(|e| Error::io(format!("creating {}", d.display()), e))?;
    }
    fs::write(path, text).map_err(|e| Error::io(format!("writing {}", path.display()), e))
}

impl Engine {
    /// Register the merge and diff drivers in the local git configuration (MVP-036).
    pub fn register_drivers(&self, exe: &Path) -> Result<()> {
        let e = exe.display().to_string();
        let repo = &self.repo;
        repo.git(&[
            "config",
            "--local",
            "merge.resonance-snapshot.name",
            "Resonance snapshot merge",
        ])?;
        repo.git(&[
            "config",
            "--local",
            "merge.resonance-snapshot.driver",
            &format!("\"{e}\" git-driver merge %O %A %B"),
        ])?;
        repo.git(&[
            "config",
            "--local",
            "diff.resonance-delta.textconv",
            &format!("\"{e}\" show-delta"),
        ])?;
        Ok(())
    }

    /// Install the hooks for this working tree's mode (FR-069, MVP-035).
    pub fn install_hooks(&self, exe: &Path, dry_run: bool) -> Result<Vec<HookChange>> {
        if self.mode.opted_in() {
            hooks::install(&self.repo, exe, hooks::OPTED_IN, true, dry_run)
        } else {
            hooks::install(&self.repo, exe, hooks::OFFLINE, false, dry_run)
        }
    }

    /// Every operation in effect: the fold of snapshot, deltas and the open delta.
    fn all_ops(&mut self) -> Result<Vec<Op>> {
        self.after_write()?;
        let mode = self.mode;
        history::fold::all_ops(&self.repo, &*self.store, mode)
    }

    /// `resonance share` (FR-185): stage 1 to stage 2. Without `apply`, only the preview.
    pub fn share(&mut self, exe: &Path, keep_local: &[String], apply: bool) -> Result<ShareView> {
        if self.mode.shared() {
            return Err(Error::Invalid(
                "this repository already shares its graph (stage 2)".into(),
            ));
        }
        if self.repo.has_committed_data() {
            return Err(Error::Invalid(
                "a teammate already shared a graph here; join it with `resonance init` instead"
                    .into(),
            ));
        }
        if self.repo.head_commit().is_none() {
            return Err(Error::Invalid("share needs at least one commit".into()));
        }
        // Resolve the annotations kept local.
        let mut local: BTreeSet<String> = BTreeSet::new();
        for k in keep_local {
            let n = self.resolve_node(k)?;
            if n.ntype != NodeType::Annotation {
                return Err(Error::Usage(format!(
                    "--keep-local takes annotations; {k} is a {}",
                    n.ntype
                )));
            }
            local.insert(n.id);
        }
        let mut view = ShareView::default();
        let r = self.reader();
        for t in NodeType::ALL {
            if matches!(t, NodeType::File | NodeType::Evidence) {
                continue;
            }
            let n = r
                .find_nodes(&NodeQuery::of_type(t).including_retired())?
                .len();
            if n > 0 {
                view.counts.insert(t.to_string(), n);
            }
        }
        for a in r.find_nodes(&NodeQuery::of_type(NodeType::Annotation))? {
            view.annotations.push(json!({
                "id": a.id, "kind": a.str_prop("kind"), "text": a.str_prop("text"),
                "local": local.contains(&a.id),
            }));
        }
        view.kept_local = local.iter().cloned().collect();
        view.files = vec![
            ".resonance/graph.db".into(),
            ".resonance/config.toml".into(),
            ".resonance/README.md".into(),
            ".resonance/.gitignore".into(),
            ".resonance/.gitattributes".into(),
        ];
        let ops = self.all_ops()?;
        // Split off what stays local: the annotations, their edges and edits to them.
        let mut local_edges: BTreeSet<String> = BTreeSet::new();
        for op in &ops {
            if op.kind == kind::EDGE_ADD
                && op.payload["src"]
                    .as_str()
                    .is_some_and(|s| local.contains(s))
                && let Some(id) = op.payload["id"].as_str()
            {
                local_edges.insert(id.to_string());
            }
        }
        let is_local = |op: &Op| {
            let id = op.payload["id"].as_str().unwrap_or_default();
            local.contains(id) || local_edges.contains(id)
        };
        let (private_ops, shared_ops): (Vec<Op>, Vec<Op>) = ops.into_iter().partition(is_local);
        view.operations = shared_ops.len();
        if !apply {
            view.hooks = hooks::install(&self.repo, exe, hooks::OPTED_IN, true, true)?;
            return Ok(view);
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        crate::lifecycle::backup(&self.repo, &home, "pre-share")?;
        // 1. The snapshot of everything shared; the old offline deltas are folded into it.
        let mut folded: Vec<String> = Vec::new();
        for loc in [Location::Deltas] {
            if let Ok(rd) = fs::read_dir(loc.dir(&self.repo)) {
                for e in rd.flatten() {
                    if let Some(id) = delta::id_of(&e.path()) {
                        folded.push(id);
                    }
                }
            }
        }
        let snap = compact::snapshot(shared_ops, folded.clone());
        self.store
            .write_snapshot(&self.repo.snapshot_path(), &snap)?;
        for id in &folded {
            let _ = fs::remove_file(history::delta_path(&self.repo, Location::Deltas, id));
        }
        // 2. What stays local goes to a private delta (FR-187, NFR-012).
        let head = self.repo.head_commit();
        let mut recs: Vec<(String, String)> = Vec::new();
        if !private_ops.is_empty() {
            let id = new_id();
            let d = delta::Delta {
                header: delta::DeltaHeader {
                    resonance: "delta".into(),
                    format: delta::DELTA_FORMAT,
                    id: id.clone(),
                    schema: crate::store::SCHEMA_VERSION,
                    writer: self.writer.clone(),
                    version: VERSION.into(),
                    epoch: snap.meta.epoch.clone(),
                    branch: compact::current_branch(&self.repo).unwrap_or_default(),
                },
                ops: private_ops,
                manifest: BTreeMap::new(),
            };
            let p = history::delta_path(&self.repo, Location::Private, &id);
            write_text(&p, &String::from_utf8_lossy(&d.to_bytes()?))?;
            recs.push((
                format!("{}{id}", keys::DELTA_PREFIX),
                serde_json::to_string(&DeltaRecord {
                    location: Location::Private,
                    commit: head.clone(),
                    sealed_at: crate::history::now(),
                    detached: false,
                })?,
            ));
        }
        // 3. A fresh open delta after everything folded; old delta records go.
        let old_records: Vec<String> = history::records(&*self.store)?
            .into_keys()
            .map(|id| format!("{}{id}", keys::DELTA_PREFIX))
            .collect();
        let last = self.store.last_op_seq()?;
        let mut open =
            read_open(&*self.store)?.ok_or_else(|| Error::Store("no open delta".into()))?;
        open.id = new_id();
        open.location = Location::Deltas;
        open.after_seq = last;
        open.manifest_after = last;
        open.carried.clear();
        open.epoch = snap.meta.epoch.clone();
        let open_text = serde_json::to_string(&open)?;
        self.store.write(GraphScope::Primary, &mut |w| {
            for k in &old_records {
                w.delete_meta(k)?;
            }
            for (k, v) in &recs {
                w.set_meta(k, v)?;
            }
            w.set_meta(keys::OPEN, &open_text)
        })?;
        // 4. The committed files, the configuration, and opting in.
        let res = self.repo.resonance_dir();
        write_text(&res.join("README.md"), README)?;
        write_text(&res.join(".gitignore"), GITIGNORE)?;
        write_text(&res.join(".gitattributes"), GITATTRIBUTES)?;
        let cfg = self.repo.config_path();
        config::set_value(&cfg, "repository", "stage", "2")?;
        config::set_value(&cfg, "repository", "minVersion", &format!("\"{VERSION}\""))?;
        exclude::remove(&self.repo)?;
        self.repo
            .git(&["config", "--local", PARTICIPATION_KEY, "user"])?;
        self.register_drivers(exe)?;
        self.config = Config::load(&cfg)?;
        self.mode = history::mode(&self.repo, &self.config);
        view.hooks = self.install_hooks(exe, false)?;
        self.reconcile(ReconcileOptions::default())?;
        view.applied = true;
        Ok(view)
    }

    /// Join a shared graph (FR-172, FR-186): opt in, and bring this working tree's offline
    /// work, if any, into the shared graph as uncommitted deltas.
    pub fn join(&mut self, exe: &Path) -> Result<JoinView> {
        let mut view = JoinView::default();
        self.after_write()?;
        // Offline deltas (sealed in .resonance/deltas or private/) become shared, uncommitted.
        let recs = history::records(&*self.store)?;
        let committed: BTreeSet<String> = self
            .repo
            .git(&["ls-files", "--", ".resonance/deltas"])
            .map(|s| {
                s.lines()
                    .filter_map(|l| delta::id_of(Path::new(l)))
                    .collect()
            })
            .unwrap_or_default();
        let mut moved = Vec::new();
        for (id, rec) in &recs {
            if committed.contains(id) {
                continue;
            }
            if let Some(p) = history::find_delta(&self.repo, id) {
                let d = delta::Delta::read(&p)?;
                view.merged_operations += d.ops.len();
                let to = history::delta_path(&self.repo, Location::Deltas, id);
                if p != to {
                    fs::create_dir_all(self.repo.deltas_dir())
                        .map_err(|e| Error::io("creating .resonance/deltas", e))?;
                    fs::rename(&p, &to).map_err(|e| Error::io("moving a delta", e))?;
                }
                moved.push(id.clone());
            }
            let _ = rec;
        }
        let gone: Vec<String> = recs
            .keys()
            .map(|id| format!("{}{id}", keys::DELTA_PREFIX))
            .collect();
        self.store.write(GraphScope::Primary, &mut |w| {
            for k in &gone {
                w.delete_meta(k)?;
            }
            Ok(())
        })?;
        view.merged_deltas = moved;
        exclude::remove(&self.repo)?;
        self.repo
            .git(&["config", "--local", PARTICIPATION_KEY, "user"])?;
        self.register_drivers(exe)?;
        self.config = Config::load(&self.repo.config_path())?;
        self.mode = history::mode(&self.repo, &self.config);
        view.hooks = self.install_hooks(exe, false)?;
        self.reconcile(ReconcileOptions::default())?;
        // A clean tree whose open delta holds only Resonance's own bookkeeping (a fresh
        // clone rebuilding its graph) has nothing to share: joining changes no tracked file.
        if self.only_bookkeeping_open()? && self.tree_clean() {
            history::fold::rebase_open(&mut *self.store)?;
            self.after_write()?;
        }
        Ok(view)
    }

    fn only_bookkeeping_open(&self) -> Result<bool> {
        let Some(open) = read_open(&*self.store)? else {
            return Ok(true);
        };
        let content = history::open_delta_content(&*self.store, &open)?;
        Ok(content
            .ops
            .iter()
            .all(|o| o.actor.kind == crate::ops::ActorKind::Tool))
    }

    fn tree_clean(&self) -> bool {
        self.repo
            .git(&[
                "status",
                "--porcelain",
                "--untracked-files=normal",
                "--",
                ".",
                ":(exclude).resonance",
            ])
            .is_ok_and(|s| s.trim().is_empty())
    }

    /// `resonance leave` (FR-172, MVP-037): stop being a Resonance user, without loss.
    pub fn leave(&mut self, export: Option<&Path>) -> Result<LeaveView> {
        if !self.mode.opted_in() {
            return Err(Error::Invalid(
                "you have not opted in here; there is nothing to leave".into(),
            ));
        }
        self.after_write()?;
        let mut view = LeaveView::default();
        if let Some(open) = read_open(&*self.store)? {
            let content = history::open_delta_content(&*self.store, &open)?;
            if !content.ops.is_empty() {
                let Some(to) = export else {
                    return Err(Error::Invalid(format!(
                        "the open delta has {} uncommitted graph change(s); commit .resonance/deltas/{}.delta first, or pass --export",
                        content.ops.len(),
                        open.id
                    )));
                };
                write_text(to, &String::from_utf8_lossy(&content.to_bytes()?))?;
                view.export = Some(to.display().to_string());
            }
        }
        view.hooks = hooks::uninstall(&self.repo)?;
        view.kit = crate::agent::uninstall_kit(&self.repo)?;
        let _ = self
            .repo
            .git(&["config", "--local", "--unset", PARTICIPATION_KEY]);
        self.mode = history::mode(&self.repo, &self.config);
        Ok(view)
    }

    /// `resonance unshare` (FR-172, MVP-037): stage 2 to stage 1 for the repository. Exports
    /// first; stages the removal of the committed files and keeps them as local offline data.
    pub fn unshare(&mut self, export: &Path, exe: &Path) -> Result<LeaveView> {
        if !self.mode.opted_in() {
            return Err(Error::Invalid(
                "unshare is for a developer who has opted in to a shared graph".into(),
            ));
        }
        let mut buf = Vec::new();
        self.export(&mut buf)?;
        write_text(export, &String::from_utf8_lossy(&buf))?;
        let mut view = LeaveView {
            export: Some(export.display().to_string()),
            ..Default::default()
        };
        let tracked = self.repo.git(&["ls-files", "--", ".resonance"])?;
        view.staged_removals = tracked.lines().map(String::from).collect();
        if !view.staged_removals.is_empty() {
            self.repo
                .git(&["rm", "-r", "-q", "--cached", "--", ".resonance"])?;
        }
        exclude::add(&self.repo)?;
        config::set_value(&self.repo.config_path(), "repository", "stage", "1")?;
        let _ = self
            .repo
            .git(&["config", "--local", "--unset", PARTICIPATION_KEY]);
        self.config = Config::load(&self.repo.config_path())?;
        self.mode = history::mode(&self.repo, &self.config);
        view.hooks = self.install_hooks(exe, false)?;
        Ok(view)
    }

    /// `resonance compact` (FR-154–FR-156).
    pub fn compact(&mut self, anywhere: bool, commit: bool) -> Result<compact::Compaction> {
        self.after_write()?;
        let mode = self.mode;
        let out = compact::compact(
            &self.repo,
            &mut *self.store,
            &self.config,
            mode,
            anywhere,
            commit,
        )?;
        self.reconcile(ReconcileOptions::default())?;
        Ok(out)
    }

    /// Compact automatically on the trunk when enough sealed deltas have accumulated
    /// (MVP-034); never commits.
    pub(super) fn maybe_compact(&mut self) -> Result<Option<compact::Compaction>> {
        if !self.mode.shared() || self.config.compaction.max_deltas == 0 {
            return Ok(None);
        }
        let trunk = compact::trunk(&self.repo, &self.config);
        if compact::current_branch(&self.repo).as_deref() != Some(trunk.as_str()) {
            return Ok(None);
        }
        if compact::sealed_count(&self.repo, &*self.store, self.mode)?
            < self.config.compaction.max_deltas
        {
            return Ok(None);
        }
        let mode = self.mode;
        let out = compact::compact(
            &self.repo,
            &mut *self.store,
            &self.config,
            mode,
            false,
            false,
        )?;
        let mut d = recovery::Divergence::new(
            Class::C1,
            Level::Attention,
            format!(
                "compacted {} delta(s) into a new snapshot",
                out.folded.len()
            ),
        );
        d.resolution = "commit .resonance/graph.db and the removed delta files (`resonance compact --commit` does it in its own commit)".into();
        recovery::record(&mut *self.store, &[d])?;
        Ok(Some(out))
    }

    /// The open delta's repository-relative path, when it has content.
    pub fn open_delta_path(&self) -> Result<Option<String>> {
        let Some(open) = read_open(&*self.store)? else {
            return Ok(None);
        };
        let p = history::delta_path(&self.repo, open.location, &open.id);
        Ok(p.is_file().then(|| {
            p.strip_prefix(&self.repo.root)
                .map(|r| r.display().to_string())
                .unwrap_or_default()
        }))
    }

    /// Findings common to every check: conflicts and pending decisions.
    fn common_findings(&self, out: &mut Vec<Finding>, strict: bool) -> Result<()> {
        let open: Vec<String> = self
            .reader()
            .find_nodes(&NodeQuery::of_type(NodeType::Conflict))?
            .into_iter()
            .filter(|c| c.str_prop("status") == Some("open"))
            .map(|c| c.id)
            .collect();
        if !open.is_empty() {
            out.push(Finding::new(
                "R006",
                if strict { "error" } else { "attention" },
                format!("{} unresolved identity conflict(s)", open.len()),
                "decide which change holds, then `resonance recover dismiss <conflict>` (see `resonance status`)",
            ).with_paths(open));
        }
        let pending: Vec<String> = recovery::list(self.reader(), false)?
            .into_iter()
            .filter(|d| d.level == Level::ActionRequired)
            .map(|d| d.id)
            .collect();
        if !pending.is_empty() {
            out.push(
                Finding::new(
                    "R008",
                    "error",
                    format!("{} divergence(s) need a decision", pending.len()),
                    "see `resonance recover`",
                )
                .with_paths(pending),
            );
        }
        if let Some(n) = self.pending_changes()? {
            out.push(Finding::new(
                "R009",
                "attention",
                format!("{n} change-log entr(ies) are pending: people changed spec items that no agent has handled yet"),
                "handle them and remove them with `resonance changes done <seq>` (FR-277)",
            ));
        }
        let kit = crate::agent::kit_status(&self.repo, self.mode);
        if !kit.installed {
            out.push(Finding::new(
                "R010",
                "info",
                "the agent kit is not installed for this working tree",
                "`resonance agent install` (FR-210)",
            ));
        } else if kit.stale {
            out.push(Finding::new(
                "R010",
                "info",
                format!(
                    "the agent kit is stale (installed by {} for another version, stage or participation)",
                    kit.version.as_deref().unwrap_or("?")
                ),
                "`resonance agent update` (FR-214)",
            ));
        }
        Ok(())
    }

    /// `check --staged` (FR-215, MVP-036). With `stage`, first stages the open delta when the
    /// staged code depends on it (what the pre-commit hook does).
    pub fn check_staged(&mut self, stage: bool) -> Result<CheckReport> {
        self.reconcile(ReconcileOptions::default())?;
        let mut report = CheckReport {
            scope: "staged".into(),
            ..Default::default()
        };
        if !self.mode.opted_in() {
            self.common_findings(&mut report.findings, false)?;
            return Ok(report.finish());
        }
        let repo = self.repo.clone();
        let staged_status = repo.git(&["diff", "--cached", "--name-status", "--no-renames"])?;
        let mut staged: Vec<(String, String)> = staged_status
            .lines()
            .filter_map(|l| {
                l.split_once('\t')
                    .map(|(s, p)| (s.to_string(), p.to_string()))
            })
            .collect();
        let open = read_open(&*self.store)?;
        let content = match &open {
            Some(o) => Some(history::open_delta_content(&*self.store, o)?),
            None => None,
        };
        let open_rel = self.open_delta_path()?;
        let code: Vec<&(String, String)> = staged
            .iter()
            .filter(|(_, p)| !p.starts_with(".resonance/"))
            .collect();
        // Stage the open delta (and uncommitted deltas brought in by joining) with the code.
        if stage && (!code.is_empty() || content.as_ref().is_some_and(|c| !c.ops.is_empty())) {
            let untracked = repo.git(&[
                "ls-files",
                "--others",
                "--exclude-standard",
                "--",
                ".resonance/deltas",
            ])?;
            let mut add: Vec<String> = untracked.lines().map(String::from).collect();
            if let Some(p) = &open_rel
                && !add.contains(p)
            {
                add.push(p.clone());
            }
            if !add.is_empty() {
                let mut args = vec!["add", "--"];
                args.extend(add.iter().map(String::as_str));
                repo.git(&args)?;
                report.staged = add;
                let again = repo.git(&["diff", "--cached", "--name-status", "--no-renames"])?;
                staged = again
                    .lines()
                    .filter_map(|l| {
                        l.split_once('\t')
                            .map(|(s, p)| (s.to_string(), p.to_string()))
                    })
                    .collect();
            }
        }
        let staged_paths: BTreeSet<&str> = staged.iter().map(|(_, p)| p.as_str()).collect();
        let code: Vec<String> = staged
            .iter()
            .filter(|(_, p)| !p.starts_with(".resonance/"))
            .map(|(_, p)| p.clone())
            .collect();
        // R001: the open delta must travel with the code it describes.
        if let (Some(p), Some(c)) = (&open_rel, &content)
            && !staged_paths.contains(p.as_str())
            && (!c.ops.is_empty() || code.iter().any(|f| c.manifest.contains_key(f)))
        {
            report.findings.push(
                Finding::new(
                    "R001",
                    "error",
                    "the open delta is not staged with the code it describes",
                    format!("git add {p}"),
                )
                .with_paths(vec![p.clone()]),
            );
        }
        // R002: staged files whose changes Resonance never saw.
        if let Some(c) = &content {
            let outside: Vec<String> = code
                .iter()
                .filter(|f| !c.manifest.contains_key(*f))
                .filter(|f| self.governs(f))
                .cloned()
                .collect();
            if !outside.is_empty() {
                report.findings.push(
                    Finding::new(
                        "R002",
                        "error",
                        format!("{} staged file(s) changed outside Resonance", outside.len()),
                        "run `resonance reconcile` so the graph sees them, then stage again",
                    )
                    .with_paths(outside),
                );
            }
        }
        // R003: never commit local state.
        let local: Vec<String> = staged_paths
            .iter()
            .filter(|p| p.starts_with(".resonance/local/"))
            .map(|p| p.to_string())
            .collect();
        if !local.is_empty() {
            report.findings.push(Finding::new(
                "R003",
                "error",
                "files under .resonance/local/ are staged; they are private to this working tree",
                "git restore --staged .resonance/local",
            ).with_paths(local));
        }
        // R004 / R005: compactions on the trunk only; sealed deltas never change.
        let trunk = compact::trunk(&repo, &self.config);
        let on_trunk = compact::current_branch(&repo).as_deref() == Some(trunk.as_str());
        let snapshot_staged = staged.iter().any(|(_, p)| p == ".resonance/graph.db");
        let deleted_deltas: Vec<String> = staged
            .iter()
            .filter(|(s, p)| s == "D" && p.starts_with(".resonance/deltas/"))
            .map(|(_, p)| p.clone())
            .collect();
        if (snapshot_staged || !deleted_deltas.is_empty()) && !on_trunk {
            report.findings.push(Finding::new(
                "R004",
                "error",
                format!("a compaction is staged off the trunk ({trunk})"),
                "compact on the trunk only (FR-155); `git restore --staged .resonance/graph.db .resonance/deltas`",
            ));
        }
        let modified: Vec<String> = staged
            .iter()
            .filter(|(s, p)| s == "M" && p.starts_with(".resonance/deltas/"))
            .map(|(_, p)| p.clone())
            .collect();
        if !modified.is_empty() || (!deleted_deltas.is_empty() && !snapshot_staged) {
            let mut paths = modified;
            if !snapshot_staged {
                paths.extend(deleted_deltas);
            }
            report.findings.push(Finding::new(
                "R005",
                "error",
                "sealed deltas were modified or removed outside a compaction",
                "sealed deltas never change (FR-076); restore them with `git restore --staged --worktree <file>`",
            ).with_paths(paths));
        }
        self.common_findings(&mut report.findings, false)?;
        Ok(report.finish())
    }

    /// Whether Resonance governs `path` (FR-178).
    pub fn governs(&self, path: &str) -> bool {
        governs(&self.config, path)
    }

    /// `check --range a..b` (FR-176, FR-216, MVP-036): every commit's file changes covered by
    /// the manifests of the deltas committed in the range, no conflicts, and a branch that
    /// merges into the trunk without conflicts in `.resonance/`.
    pub fn check_range(&mut self, range: &str) -> Result<CheckReport> {
        self.reconcile(ReconcileOptions::default())?;
        let repo = self.repo.clone();
        let mut report = CheckReport {
            scope: range.to_string(),
            ..Default::default()
        };
        let (a, b) = range
            .split_once("..")
            .map(|(a, b)| {
                (
                    a.to_string(),
                    if b.is_empty() {
                        "HEAD".to_string()
                    } else {
                        b.to_string()
                    },
                )
            })
            .ok_or_else(|| Error::Usage(format!("`{range}` is not a range such as main..HEAD")))?;
        let coverage = range_coverage(&repo, &a, &b, &self.config)?;
        if !coverage.uncovered_mine.is_empty() {
            report.findings.push(Finding::new(
                "R002",
                "error",
                format!(
                    "{} file(s) changed in your commits without Resonance seeing them (C8: hooks bypassed, or the daemon was down)",
                    coverage.uncovered_mine.len()
                ),
                "run `resonance reconcile`, commit the open delta (`git add .resonance/deltas && git commit`), then push",
            ).with_paths(coverage.uncovered_mine));
        }
        if !coverage.uncovered_others.is_empty() {
            report.findings.push(
                Finding::new(
                    "R002",
                    "info",
                    format!(
                        "{} file(s) changed by teammates without Resonance",
                        coverage.uncovered_others.len()
                    ),
                    "nothing to do: their changes are ingested as outside changes (C2)",
                )
                .with_paths(coverage.uncovered_others),
            );
        }
        // R007: would merging into the trunk conflict on the snapshot?
        let trunk = compact::trunk(&repo, &self.config);
        if let Ok(base) = repo.git(&["merge-base", &b, &trunk]) {
            let ours = !repo
                .git(&[
                    "diff",
                    "--name-only",
                    &base,
                    &b,
                    "--",
                    ".resonance/graph.db",
                ])?
                .trim()
                .is_empty();
            let theirs = !repo
                .git(&[
                    "diff",
                    "--name-only",
                    &base,
                    &trunk,
                    "--",
                    ".resonance/graph.db",
                ])?
                .trim()
                .is_empty();
            if ours && theirs {
                let driver = repo
                    .git(&["config", "--get", "merge.resonance-snapshot.driver"])
                    .is_ok();
                report.findings.push(Finding::new(
                    "R007",
                    if driver { "attention" } else { "error" },
                    "the snapshot changed on both this branch and the trunk; merging will need the snapshot merge driver",
                    if driver {
                        "the merge driver repairs it mechanically"
                    } else {
                        "compact only on the trunk; or run `resonance recover repair-snapshot` after the merge"
                    },
                ));
            }
        }
        self.common_findings(&mut report.findings, true)?;
        Ok(report.finish())
    }

    /// `resonance review <range>` (FR-066, MVP-036).
    pub fn review(&mut self, range: &str) -> Result<ReviewView> {
        let repo = self.repo.clone();
        let (a, b) = range
            .split_once("..")
            .map(|(a, b)| {
                (
                    a.to_string(),
                    if b.is_empty() {
                        "HEAD".to_string()
                    } else {
                        b.to_string()
                    },
                )
            })
            .ok_or_else(|| Error::Usage(format!("`{range}` is not a range such as main..HEAD")))?;
        let mut v = ReviewView {
            range: range.to_string(),
            ..Default::default()
        };
        let added = repo.git(&[
            "diff",
            "--name-only",
            "--diff-filter=A",
            &a,
            &b,
            "--",
            ".resonance/deltas",
        ])?;
        let mut ops: Vec<Op> = Vec::new();
        for path in added.lines() {
            if let Ok(bytes) = repo.git_bytes(&["show", &format!("{b}:{path}")])
                && let Ok(d) = delta::Delta::parse(&bytes)
            {
                v.deltas.push(d.header.id.clone());
                ops.extend(d.ops);
            }
        }
        // Uncommitted: the open delta, when reviewing up to the working tree.
        v.compacted = !repo
            .git(&["diff", "--name-only", &a, &b, "--", ".resonance/graph.db"])?
            .trim()
            .is_empty();
        let r = self.reader();
        let label = |id: &str| -> String {
            r.get_node(id)
                .ok()
                .flatten()
                .map(|n| crate::query::label_of(r, &n).unwrap_or_else(|_| id.to_string()))
                .unwrap_or_else(|| id.to_string())
        };
        let created: BTreeSet<String> = ops
            .iter()
            .filter(|o| o.kind == kind::NODE_CREATE)
            .filter_map(|o| o.payload["id"].as_str().map(String::from))
            .collect();
        for op in &ops {
            let p = &op.payload;
            let id = p["id"].as_str().unwrap_or_default();
            match op.kind.as_str() {
                kind::NODE_CREATE => {
                    let t = p["type"].as_str().unwrap_or_default();
                    if matches!(t, "Requirement" | "Decision" | "Design" | "Constraint") {
                        v.intent_added.push(format!("{t} {}", label(id)));
                    } else if t == "Annotation" {
                        v.annotations.push(format!(
                            "{}: {}",
                            p["props"]["kind"].as_str().unwrap_or("note"),
                            p["props"]["text"].as_str().unwrap_or_default()
                        ));
                    } else if t == "Conflict" {
                        v.conflicts.push(id.to_string());
                    }
                }
                kind::NODE_SET if !created.contains(id) => {
                    let key = p["key"].as_str().unwrap_or_default();
                    let is_intent = r
                        .get_node(id)
                        .ok()
                        .flatten()
                        .is_some_and(|n| NodeType::SPEC_ITEMS.contains(&n.ntype));
                    if is_intent {
                        if key == "status" && p["value"] == "retired" {
                            v.intent_retired.push(label(id));
                        } else if !matches!(key, "source" | "alsoDefinedIn" | "specStatus") {
                            v.intent_changed.push(format!("{} ({key})", label(id)));
                        }
                    }
                }
                kind::EDGE_ADD => v.links_added.push(format!(
                    "{} {} {}",
                    label(p["src"].as_str().unwrap_or_default()),
                    p["props"]["extType"]
                        .as_str()
                        .or(p["type"].as_str())
                        .unwrap_or_default(),
                    label(p["dst"].as_str().unwrap_or_default())
                )),
                kind::EDGE_REMOVE => v.links_removed.push(id.to_string()),
                kind::EDGE_SET if p["key"] == "status" => v.link_status_changes += 1,
                k if k.starts_with("identity.") || k == kind::FILE_MOVE => {
                    v.identity_operations += 1
                }
                _ => {}
            }
        }
        v.intent_changed.sort();
        v.intent_changed.dedup();
        let coverage = range_coverage(&repo, &a, &b, &self.config)?;
        v.files_outside_resonance = coverage
            .uncovered_mine
            .into_iter()
            .chain(coverage.uncovered_others)
            .collect();
        Ok(v)
    }

    /// Run a git hook (MVP-035): `pre-commit` and `pre-push` check and may block; `post-*`
    /// only bring the graph up to date.
    pub fn run_hook(&mut self, name: &str, args: &[String], stdin: &str) -> Result<CheckReport> {
        match name {
            "pre-commit" => self.check_staged(true),
            "pre-push" => {
                let mut report = CheckReport {
                    scope: "push".into(),
                    ..Default::default()
                };
                if !self.mode.opted_in() {
                    return Ok(report.finish());
                }
                for line in stdin.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    let [_, local_sha, _, remote_sha] = parts.as_slice() else {
                        continue;
                    };
                    if local_sha.chars().all(|c| c == '0') {
                        continue; // A deletion.
                    }
                    let base = if remote_sha.chars().all(|c| c == '0') {
                        // A new branch: what is not on the trunk yet.
                        let trunk = compact::trunk(&self.repo, &self.config);
                        let target = if self
                            .repo
                            .git(&[
                                "rev-parse",
                                "--verify",
                                "--quiet",
                                &format!("refs/remotes/origin/{trunk}"),
                            ])
                            .is_ok()
                        {
                            format!("origin/{trunk}")
                        } else {
                            trunk
                        };
                        match self.repo.git(&["merge-base", local_sha, &target]) {
                            Ok(b) if !b.is_empty() => b,
                            _ => continue,
                        }
                    } else {
                        remote_sha.to_string()
                    };
                    let r = self.check_range(&format!("{base}..{local_sha}"))?;
                    report.findings.extend(r.findings);
                }
                Ok(report.finish())
            }
            _ => {
                let _ = args;
                self.reconcile(ReconcileOptions::default())?;
                Ok(CheckReport {
                    scope: name.to_string(),
                    ..Default::default()
                })
            }
        }
    }

    /// Change-log entries pending, if any (filled in by the change log).
    fn pending_changes(&self) -> Result<Option<usize>> {
        let n = crate::changes::pending_count(self.reader())?;
        Ok((n > 0).then_some(n))
    }

    /// Record that `history` refused something for the C6 repair of a conflicted snapshot.
    pub fn repair_snapshot(&mut self) -> Result<bool> {
        let done = compact::repair_conflicted_snapshot(&self.repo, &*self.store)?;
        if done {
            self.reconcile(ReconcileOptions::default())?;
        }
        Ok(done)
    }

    /// Whether this working tree has opted in (for the CLI).
    pub fn opted_in(&self) -> bool {
        self.mode.opted_in()
    }
}

/// Whether the configuration governs `path` (FR-178): every path when no globs are set.
pub fn governs(config: &Config, path: &str) -> bool {
    if config.governance.paths.is_empty() {
        return true;
    }
    let mut b = globset::GlobSetBuilder::new();
    for g in &config.governance.paths {
        if let Ok(glob) = globset::Glob::new(g) {
            b.add(glob);
        }
    }
    b.build().map(|s| s.is_match(path)).unwrap_or(true)
}

/// Files changed in `a..b` that no delta committed in the range covers.
struct Coverage {
    uncovered_mine: Vec<String>,
    uncovered_others: Vec<String>,
}

fn range_coverage(repo: &crate::repo::Repo, a: &str, b: &str, config: &Config) -> Result<Coverage> {
    let changed: Vec<String> = repo
        .git(&["diff", "--name-only", a, b])?
        .lines()
        .filter(|p| !p.starts_with(".resonance/"))
        .filter(|p| governs(config, p))
        .map(String::from)
        .collect();
    // Every delta written by a commit in the range counts, including those a compaction
    // later in the range folded away.
    let mut covered: BTreeSet<String> = BTreeSet::new();
    let commits = repo.git(&[
        "log",
        "--format=%H",
        &format!("{a}..{b}"),
        "--",
        ".resonance/deltas",
    ])?;
    for c in commits.lines() {
        let added = repo.git(&[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "-m",
            "--diff-filter=AM",
            c,
            "--",
            ".resonance/deltas",
        ])?;
        for path in added.lines() {
            if let Ok(bytes) = repo.git_bytes(&["show", &format!("{c}:{path}")])
                && let Ok(d) = delta::Delta::parse(&bytes)
            {
                covered.extend(d.manifest.keys().cloned());
            }
        }
    }
    let me = repo.git(&["config", "user.email"]).unwrap_or_default();
    let mut mine = Vec::new();
    let mut others = Vec::new();
    for f in changed.into_iter().filter(|f| !covered.contains(f)) {
        let authors = repo
            .git(&["log", "--format=%ae", &format!("{a}..{b}"), "--", &f])
            .unwrap_or_default();
        if authors.lines().any(|e| e == me) {
            mine.push(f);
        } else {
            others.push(f);
        }
    }
    Ok(Coverage {
        uncovered_mine: mine,
        uncovered_others: others,
    })
}

/// A delta for people (`show-delta`, git's textconv driver).
pub fn render_delta(bytes: &[u8]) -> String {
    let d = match delta::Delta::parse(bytes) {
        Ok(d) => d,
        Err(e) => return format!("(not a readable Resonance delta: {e})\n"),
    };
    let mut out = format!(
        "Resonance delta {} by {} (format {}, on {})\n",
        d.header.id, d.header.writer, d.header.format, d.header.branch
    );
    for op in &d.ops {
        let p = &op.payload;
        let what = match op.kind.as_str() {
            kind::NODE_CREATE => format!(
                "create {} {}",
                p["type"].as_str().unwrap_or_default(),
                p["props"]["key"]
                    .as_str()
                    .or(p["props"]["title"].as_str())
                    .or(p["props"]["name"].as_str())
                    .or(p["props"]["text"].as_str())
                    .unwrap_or(p["id"].as_str().unwrap_or_default())
            ),
            kind::NODE_SET => format!(
                "set {}.{} = {}",
                p["id"].as_str().unwrap_or_default(),
                p["key"].as_str().unwrap_or_default(),
                short(&p["value"])
            ),
            kind::EDGE_ADD => format!(
                "link {} {} {}",
                p["src"].as_str().unwrap_or_default(),
                p["props"]["extType"]
                    .as_str()
                    .or(p["type"].as_str())
                    .unwrap_or_default(),
                p["dst"].as_str().unwrap_or_default()
            ),
            kind::EDGE_SET => format!(
                "link {}.{} = {}",
                p["id"].as_str().unwrap_or_default(),
                p["key"].as_str().unwrap_or_default(),
                short(&p["value"])
            ),
            other => format!("{other} {}", p["id"].as_str().unwrap_or_default()),
        };
        out.push_str(&format!("  {} {what}\n", op.actor));
    }
    if !d.manifest.is_empty() {
        out.push_str("  files changed:\n");
        for (path, (a, b)) in &d.manifest {
            let s = |h: &Option<String>| {
                h.as_deref()
                    .map(|h| &h[..h.len().min(8)])
                    .unwrap_or("-")
                    .to_string()
            };
            out.push_str(&format!("    {path} {} -> {}\n", s(a), s(b)));
        }
    }
    out
}

fn short(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let one: String = s
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(60)
        .collect();
    if one.len() < s.len() {
        format!("{one}…")
    } else {
        one
    }
}
