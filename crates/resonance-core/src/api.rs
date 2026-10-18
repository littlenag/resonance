//! The single library API that serves both the CLI and the MCP server (FR-100, FR-251).
//!
//! Every operation is a method on [`Engine`]. Callers reconcile first ([`Engine::reconcile`],
//! MVP-002), then call one operation, which returns a serializable view. Mutations are
//! applied as one batch of edit-algebra operations (FR-103).

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::anchors::{self, Anchor, AnchorState, Method};
use crate::error::{Error, Result};
use crate::history::{self, Mode, fold};
use crate::ingest::lineindex::LineIndex;
use crate::ingest::reconcile::{put_anchor, recompute_containment};
use crate::ingest::{ReconcileOptions, ReconcileReport, reconcile};
use crate::intent::{self, IntentReport};
use crate::links::{self, LINK_TYPES, anchor_of, is_live, new_link, new_link_props};
use crate::model::{Edge, EdgeType, Layer, LinkStatus, Node, NodeType, Props, ids, new_id};
use crate::ops::{Actor, ActorKind, Tx, hlc, kind};
use crate::query::address::{Resolved, resolve, resolve_edge, resolve_node};
use crate::query::context::ContextPack;
use crate::query::repair::RepairItem;
use crate::query::search::{SearchHit, Unverified};
use crate::query::show::ShowView;
use crate::query::trace::TraceView;
use crate::query::why::WhyView;
use crate::query::{
    AnnotationView, DecisionView, LinkView, NodeRef, RequirementView, UnitView, annotation_view,
    decision_view, link_view, node_ref_by_id, requirement_view, unit_view,
};
use crate::repo::config::{Config, write_intent_sources};
use crate::repo::{Repo, abs_path};
use crate::store::{EdgeQuery, GraphRead, GraphScope, GraphStore, NodeQuery, SCHEMA_VERSION, meta};
use crate::verify::{self, RunOutcome, TestResult};
use crate::{export, query};

/// The version of this build.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Annotation kinds (S4).
pub const ANNOTATION_KINDS: [&str; 4] = ["note", "warning", "call-out", "instruction"];

/// The Resonance API for one repository.
pub struct Engine {
    pub repo: Repo,
    pub config: Config,
    store: Box<dyn GraphStore>,
    actor: Actor,
    /// Relative paths typed by a person are resolved against this directory.
    cwd: PathBuf,
    /// Lines to files, for finding regions moved into unchanged files (MVP-024).
    index: LineIndex,
    /// The repository's stage and this working tree's participation (§8.16).
    mode: Mode,
    /// Who writes this working tree's deltas (the person who opened the engine).
    writer: String,
    /// The open delta as last written, for self-write suppression (FR-044).
    synced: history::Synced,
    /// The next reconcile is a daemon's first: changes it finds are C3 (MVP-031).
    startup: bool,
    /// This developer's email, to tell their commits from others' (C2).
    me: String,
}

/// Counts for `status`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub files: usize,
    pub requirements: usize,
    pub requirements_retired: usize,
    pub decisions: usize,
    pub code_units: usize,
    pub tests: usize,
    pub annotations: usize,
    pub evidence: usize,
    pub links: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    pub root: String,
    /// The repository's adoption stage: 1, offline trial (§8.16).
    pub stage: String,
    pub participation: String,
    pub schema_version: u32,
    pub last_reconcile: Option<String>,
    pub intent_sources: Vec<String>,
    pub counts: Counts,
    pub repair: Vec<RepairItem>,
    pub history: HistoryStatus,
    /// Unresolved divergences (§8.18).
    pub divergences: Vec<crate::recovery::Divergence>,
}

/// `intent project` (FR-121).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub path: String,
    pub keys: Vec<String>,
    pub written: bool,
}

/// `recover` with no action (FR-196).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverView {
    pub divergences: Vec<crate::recovery::Divergence>,
    pub detached: Vec<DetachedDelta>,
    pub held: Option<fold::Held>,
    pub records: Vec<crate::recovery::RecoveryRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetachedDelta {
    pub id: String,
    pub commit: Option<String>,
    pub operations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoView {
    pub record: crate::recovery::RecoveryRecord,
    /// The version that was in place, kept so that the undo can be undone.
    pub saved: crate::recovery::RecoveryRecord,
    pub reconcile: ReconcileReport,
}

/// `resonance value` (FR-240).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValueView {
    pub traces: u64,
    pub whys: u64,
    pub context_packs: u64,
    pub searches: u64,
    pub test_runs: u64,
    /// Anchors that followed edits, moves and renames on their own.
    pub anchors_followed: u64,
    pub repairs: u64,
    pub repair_tokens: u64,
    pub repair_seconds: u64,
    pub divergences: std::collections::BTreeMap<String, u64>,
    pub counters: std::collections::BTreeMap<String, u64>,
    /// What improves on sharing or joining, in offline mode.
    pub upgrade: Option<String>,
}

/// The delta history, for `status` (MVP-026–MVP-030).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStatus {
    /// The open delta's file, relative to the repository root, when it has content.
    pub open_delta: Option<String>,
    /// Operations in the open delta (excluding file manifest entries).
    pub open_operations: usize,
    /// Files in the open delta's manifest.
    pub open_files: usize,
    pub sealed: usize,
    /// Offline deltas whose commits were rewritten and not re-associated (MVP-030).
    pub detached: Vec<String>,
    /// A re-fold refused because operations would disappear without `HEAD` moving (C6).
    pub held: Option<fold::Held>,
    /// Operations the last fold could not apply.
    pub skipped: usize,
    pub snapshot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportView {
    pub sources: Vec<String>,
    pub added_sources: Vec<String>,
    pub intent: IntentReport,
}

/// A link that was created or already existed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkResult {
    /// The node the link starts from; `link.other` is where it goes.
    pub source: NodeRef,
    pub link: LinkView,
    /// False when the link already existed, or when an existing link was changed.
    pub created: bool,
}

/// Fields for a new code unit or test.
#[derive(Debug, Clone, Default)]
pub struct UnitInput {
    /// `path:START-END`, `path:LINE`, or a path used with `match_text`.
    pub region: String,
    pub match_text: Option<String>,
    pub name: String,
    pub kind: Option<String>,
    /// For tests: the exact command that runs this test alone (MVP-017).
    pub run: Option<String>,
    pub test: bool,
}

/// Changes to a code unit or test. All `None` confirms the current anchor.
#[derive(Debug, Clone, Default)]
pub struct UnitEdit {
    pub region: Option<String>,
    pub match_text: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub run: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DecisionInput {
    pub title: Option<String>,
    pub decision: Option<String>,
    pub context: Option<String>,
    pub consequences: Option<String>,
    pub status: Option<String>,
}

/// The result of a split or merge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityChange {
    /// The units that were retired.
    pub retired: Vec<UnitView>,
    /// The units that carry their identity on.
    pub into: Vec<UnitView>,
    /// Links and annotations copied or moved.
    pub links_moved: usize,
}

/// Copy link `e` with endpoint `from` replaced by `to`, unless an equal link exists. Returns 1
/// when a link was created.
fn copy_link(
    tx: &mut Tx<'_>,
    e: &Edge,
    from: &str,
    to: &str,
    provenance: &str,
    author: &str,
) -> Result<usize> {
    let (src, dst) = if e.src == from {
        (to.to_string(), e.dst.clone())
    } else {
        (e.src.clone(), to.to_string())
    };
    let exists = tx
        .r()
        .find_edges(&EdgeQuery::from(&src).of_type(e.etype))?
        .iter()
        .any(|x| x.dst == dst);
    if exists {
        return Ok(0);
    }
    let mut props = new_link_props(tx.r(), &src, &dst, provenance, author, None)?;
    props.insert("derivedFrom".into(), json!(e.id));
    tx.add_edge(&new_link(e.etype, &src, &dst, props))?;
    Ok(1)
}

/// One test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRun {
    pub test: UnitView,
    pub command: Option<String>,
    #[serde(flatten)]
    pub outcome: RunOutcome,
    pub evidence_id: String,
    /// The `verifies` links this run supports, with their status afterwards.
    pub links: Vec<LinkView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestReport {
    pub target: String,
    pub runs: Vec<TestRun>,
    pub passed: usize,
    pub failed: usize,
    pub errors: usize,
}

/// What `restore` did.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub applied: usize,
    /// Operations for things that already existed, or that could not apply.
    pub skipped: usize,
    pub reconcile: Option<ReconcileReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub version: String,
    pub schema_version: u32,
    pub mcp_protocol_version: String,
    pub mcp_protocol_versions: Vec<String>,
}

/// The binary's version information (FR-256).
pub fn version_info() -> VersionInfo {
    VersionInfo {
        version: VERSION.to_string(),
        schema_version: SCHEMA_VERSION,
        mcp_protocol_version: crate::mcp::PROTOCOL_VERSION.to_string(),
        mcp_protocol_versions: crate::mcp::SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }
}

fn nonempty(field: &str, v: &str) -> Result<()> {
    if v.trim().is_empty() {
        Err(Error::Usage(format!("{field} must not be empty")))
    } else {
        Ok(())
    }
}

/// Whether `etype` may go from `src` to `dst` (§6.3).
fn check_link_types(etype: EdgeType, src: &Node, dst: &Node) -> Result<()> {
    use NodeType::*;
    let ok = match etype {
        EdgeType::Satisfies => {
            matches!(src.ntype, CodeUnit | Test | File) && dst.ntype == Requirement
        }
        EdgeType::Verifies => src.ntype == Test && matches!(dst.ntype, Requirement | CodeUnit),
        EdgeType::Constrains => {
            matches!(src.ntype, Decision | Constraint)
                && !matches!(dst.ntype, Evidence | Annotation)
        }
        EdgeType::Annotates => {
            src.ntype == Annotation && !matches!(dst.ntype, Evidence | Annotation)
        }
        EdgeType::DerivedFrom => {
            matches!(src.ntype, Requirement | Design) && dst.ntype == Requirement
        }
        EdgeType::Supersedes => {
            src.ntype == dst.ntype
                && matches!(src.ntype, Requirement | Decision | Design | Constraint)
        }
        EdgeType::Realizes => matches!(src.ntype, CodeUnit | Test | File) && dst.ntype == Design,
        EdgeType::Extension => !matches!(src.ntype, Evidence) && !matches!(dst.ntype, Evidence),
        EdgeType::Contains | EdgeType::Evidences => false,
    };
    if ok {
        return Ok(());
    }
    let expected = match etype {
        EdgeType::Satisfies => "a code unit, test or file satisfies a requirement",
        EdgeType::Verifies => "a test verifies a requirement or a code unit",
        EdgeType::Constrains => {
            "a decision or constraint constrains a requirement, code unit, test, file, design or decision"
        }
        EdgeType::Annotates => "an annotation annotates a node",
        EdgeType::DerivedFrom => "a requirement or design is derived from a requirement",
        EdgeType::Supersedes => "an item supersedes an earlier item of the same type",
        EdgeType::Realizes => "a code unit, test or file realizes a design",
        _ => "this link type is derived and cannot be created by hand",
    };
    Err(Error::Invalid(format!(
        "a {} cannot {} a {}: {expected}",
        src.ntype, etype, dst.ntype
    )))
}

/// A namespaced extension type name: `namespace:Name` (FR-007).
pub fn check_ext_name(name: &str) -> Result<()> {
    let ok = name.split_once(':').is_some_and(|(ns, n)| {
        !ns.is_empty()
            && !n.is_empty()
            && ns
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && n.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    });
    if ok {
        Ok(())
    } else {
        Err(Error::Usage(format!(
            "`{name}` is not a namespaced extension type; use namespace:Name, for example acme:Service (FR-007)"
        )))
    }
}

fn require_live(node: &Node, label: &str) -> Result<()> {
    if is_live(node) {
        return Ok(());
    }
    Err(Error::Invalid(format!(
        "{label} is {}; repair or re-anchor it first (see `resonance status`)",
        query::status_of(node)
    )))
}

impl Engine {
    /// Open the API over a store, migrating it if needed (FR-006, FR-258).
    pub fn open(
        repo: Repo,
        mut store: Box<dyn GraphStore>,
        actor: Actor,
        cwd: PathBuf,
    ) -> Result<Engine> {
        store.migrate()?;
        let config = Config::load(&repo.config_path())?;
        let mode = history::mode(&repo, &config);
        let writer = actor.to_string();
        let me = repo.git(&["config", "user.email"]).unwrap_or_default();
        Ok(Engine {
            repo,
            config,
            store,
            actor,
            cwd,
            index: LineIndex::default(),
            mode,
            writer,
            synced: history::Synced::default(),
            startup: false,
            me,
        })
    }

    /// Mark the next reconcile as a daemon's start-up reconcile (C3, MVP-031).
    pub fn set_startup(&mut self, startup: bool) {
        self.startup = startup;
    }

    /// Classify what a reconcile observed and record it (FR-190, FR-198).
    fn classify(&mut self, cu: &fold::CatchUp, report: &mut ReconcileReport) -> Result<()> {
        let observed = crate::recovery::Observed {
            catch_up: cu,
            changed: &report.changed,
            startup: std::mem::take(&mut self.startup),
            mode: self.mode,
            me: &self.me,
        };
        let divs = crate::recovery::classify(&self.repo, self.reader(), &observed)?;
        crate::recovery::record(&mut *self.store, &divs)?;
        let a = &report.anchors;
        crate::recovery::count(
            &mut *self.store,
            &[(
                "anchors.followed",
                (a.relocated + a.moved + a.approximate + a.restored) as u64,
            )],
        )?;
        report.divergences = divs;
        Ok(())
    }

    /// Add to local counters (MVP-039, FR-240).
    pub fn count(&mut self, items: &[(&str, u64)]) -> Result<()> {
        crate::recovery::count(&mut *self.store, items)
    }

    /// The repository's stage and this working tree's participation.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Whether `path` is the open delta exactly as this engine last wrote it (FR-044).
    pub fn is_own_write(&self, path: &Path) -> bool {
        match (&self.synced.open_path, &self.synced.hash) {
            (Some(p), Some(h)) if p == path => fs::read(path)
                .map(|b| crate::model::content_hash(&b) == *h)
                .unwrap_or(false),
            _ => false,
        }
    }

    /// Write the open delta and the journal after a change set (MVP-033, FR-194).
    fn after_write(&mut self) -> Result<()> {
        self.synced = history::sync(&self.repo, &mut *self.store, self.mode, &self.writer)?;
        Ok(())
    }

    /// Catch up with git and the delta history, settling the store after a re-fold.
    fn catch_up(&mut self, force: bool) -> Result<(fold::CatchUp, Option<ReconcileReport>)> {
        self.mode = history::mode(&self.repo, &self.config);
        let cu = fold::catch_up(&self.repo, &mut *self.store, self.mode, &self.writer, force)?;
        let settled = if cu.refolded {
            let r = crate::ingest::reconcile::settle(
                &mut *self.store,
                &self.repo.root,
                &self.config,
                &mut self.index,
            )?;
            fold::mark_settled(&mut *self.store)?;
            Some(r)
        } else {
            None
        };
        Ok((cu, settled))
    }

    /// Re-fold the graph from its snapshot and deltas, then reconcile (FR-150, `rebuild`).
    pub fn rebuild(&mut self) -> Result<ReconcileReport> {
        self.config = Config::load(&self.repo.config_path())?;
        let (cu, _) = self.catch_up(true)?;
        let mut report = reconcile(
            &mut *self.store,
            &self.repo.root,
            &self.config,
            ReconcileOptions::default(),
            &mut self.index,
        )?;
        self.after_write()?;
        report.history = Some(cu);
        Ok(report)
    }

    pub fn reader(&self) -> &dyn GraphRead {
        &*self.store
    }

    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    pub fn set_actor(&mut self, actor: Actor) {
        self.actor = actor;
    }

    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }

    fn provenance(&self) -> &'static str {
        match self.actor.kind {
            ActorKind::Agent => "agent",
            ActorKind::Tool => "derived",
            ActorKind::Human => "human",
        }
    }

    /// Reconcile the graph with the disk (MVP-002). Re-reads the configuration first.
    /// Catches up with git and the delta history first (FR-068, FR-195).
    pub fn reconcile(&mut self, opts: ReconcileOptions) -> Result<ReconcileReport> {
        self.config = Config::load(&self.repo.config_path())?;
        let (cu, _) = self.catch_up(false)?;
        let mut report = reconcile(
            &mut *self.store,
            &self.repo.root,
            &self.config,
            opts,
            &mut self.index,
        )?;
        self.after_write()?;
        self.classify(&cu, &mut report)?;
        if cu != fold::CatchUp::default() {
            report.history = Some(cu);
        }
        Ok(report)
    }

    /// Reconcile only the given paths, as reported by the watcher (FR-049). After a re-fold
    /// the whole tree is reconciled instead.
    pub fn reconcile_paths(&mut self, paths: &[String]) -> Result<ReconcileReport> {
        let (cu, settled) = self.catch_up(false)?;
        let mut report = if settled.is_some() {
            reconcile(
                &mut *self.store,
                &self.repo.root,
                &self.config,
                ReconcileOptions::default(),
                &mut self.index,
            )?
        } else {
            crate::ingest::reconcile_paths(
                &mut *self.store,
                &self.repo.root,
                &self.config,
                paths,
                &mut self.index,
            )?
        };
        self.after_write()?;
        self.classify(&cu, &mut report)?;
        if cu != fold::CatchUp::default() {
            report.history = Some(cu);
        }
        Ok(report)
    }

    /// Re-read the configuration file (after it changes).
    pub fn reload_config(&mut self) -> Result<()> {
        self.config = Config::load(&self.repo.config_path())?;
        Ok(())
    }

    fn resolve(&self, reference: &str) -> Result<Resolved> {
        resolve(self.reader(), &self.repo, &self.cwd, reference)
    }

    fn resolve_node(&self, reference: &str) -> Result<Node> {
        resolve_node(self.reader(), &self.repo, &self.cwd, reference)
    }

    fn resolve_edge(&self, reference: &str) -> Result<Edge> {
        resolve_edge(self.reader(), &self.repo, &self.cwd, reference)
    }

    /// Run `f` as one change set (FR-022, FR-103).
    fn mutate<T>(&mut self, f: impl FnOnce(&mut Tx<'_>) -> Result<T>) -> Result<T> {
        let actor = self.actor.clone();
        let mut f = Some(f);
        let mut out = None;
        self.store.write(GraphScope::Primary, &mut |w| {
            let mut tx = Tx::new(w, actor.clone())?;
            let f = f.take().expect("a change set runs once");
            out = Some(f(&mut tx)?);
            tx.finish()?;
            Ok(())
        })?;
        self.after_write()?;
        Ok(out.expect("the change set ran"))
    }

    // ----- Status ---------------------------------------------------------------------------

    pub fn status(&self) -> Result<StatusView> {
        let r = self.reader();
        let count =
            |t: NodeType| -> Result<usize> { Ok(r.find_nodes(&NodeQuery::of_type(t))?.len()) };
        let reqs = r.find_nodes(&NodeQuery::of_type(NodeType::Requirement))?;
        let retired = reqs
            .iter()
            .filter(|n| n.str_prop("status") == Some("retired"))
            .count();
        let mut links = std::collections::BTreeMap::new();
        for s in LinkStatus::ALL {
            let n = r
                .find_edges(&EdgeQuery::all().with_status(s))?
                .iter()
                .filter(|e| LINK_TYPES.contains(&e.etype))
                .count();
            links.insert(s.as_str().to_string(), n);
        }
        let open = history::read_open(r)?;
        let open_content = match &open {
            Some(o) => Some(history::open_delta_content(r, o)?),
            None => None,
        };
        let open_path = open
            .as_ref()
            .map(|o| history::delta_path(&self.repo, o.location, &o.id));
        let records = history::records(r)?;
        let skipped: Vec<String> = r
            .get_meta(history::keys::SKIPPED)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let history_status = HistoryStatus {
            open_delta: open_path.filter(|p| p.is_file()).and_then(|p| {
                p.strip_prefix(&self.repo.root)
                    .ok()
                    .map(|p| p.display().to_string())
            }),
            open_operations: open_content.as_ref().map(|c| c.ops.len()).unwrap_or(0),
            open_files: open_content.as_ref().map(|c| c.manifest.len()).unwrap_or(0),
            sealed: records.values().filter(|r| !r.detached).count(),
            detached: records
                .iter()
                .filter(|(_, r)| r.detached)
                .map(|(id, _)| id.clone())
                .collect(),
            held: fold::held(r)?,
            skipped: skipped.len(),
            snapshot: self.repo.snapshot_path().is_file(),
        };
        Ok(StatusView {
            root: self.repo.root.display().to_string(),
            stage: self.mode.stage.to_string(),
            participation: self.mode.participation.as_str().to_string(),
            history: history_status,
            schema_version: self.store.schema_version()?,
            last_reconcile: r.get_meta(meta::LAST_RECONCILE)?,
            intent_sources: self.config.intent.sources.clone(),
            counts: Counts {
                files: r.list_files()?.len(),
                requirements: reqs.len() - retired,
                requirements_retired: retired,
                decisions: count(NodeType::Decision)?,
                code_units: count(NodeType::CodeUnit)?,
                tests: count(NodeType::Test)?,
                annotations: count(NodeType::Annotation)?,
                evidence: count(NodeType::Evidence)?,
                links,
            },
            repair: self.repair_list()?,
            divergences: crate::recovery::list(r, false)?,
        })
    }

    pub fn repair_list(&self) -> Result<Vec<RepairItem>> {
        let mut items = query::repair::repair_list(self.reader())?;
        self.prioritize(&mut items);
        Ok(items)
    }

    // ----- Intent ---------------------------------------------------------------------------

    /// Import requirement documents and keep them in sync (MVP-011, FR-122).
    pub fn import(&mut self, paths: &[String], format: Option<&str>) -> Result<ImportView> {
        if paths.is_empty() {
            return Err(Error::Usage(
                "give at least one file (or ADR directory) to import".into(),
            ));
        }
        if let Some(f) = format
            && intent::formats::Format::parse(f).is_none()
        {
            return Err(Error::Usage(format!(
                "unknown format `{f}` (expected markdown, speckit, kiro, kiro-design, ears or adr)"
            )));
        }
        let mut sources = self.config.intent.sources.clone();
        let mut added = Vec::new();
        for p in paths {
            let rel = self.repo.relative_path(&self.cwd, p)?;
            let abs = abs_path(&self.repo.root, &rel);
            if !abs.is_file() && !abs.is_dir() {
                return Err(Error::NotFound(format!("the file {p}")));
            }
            let rel = match format {
                Some(f) => format!("{f}:{rel}"),
                None => rel,
            };
            if !sources.contains(&rel) {
                sources.push(rel.clone());
                added.push(rel);
            }
        }
        if !added.is_empty() {
            write_intent_sources(&self.repo.config_path(), &sources)?;
        }
        let report = self.reconcile(ReconcileOptions {
            force_intent: true,
            ..Default::default()
        })?;
        Ok(ImportView {
            sources,
            added_sources: added,
            intent: report.intent,
        })
    }

    /// Create a requirement (FR-120).
    pub fn req_add(
        &mut self,
        key: &str,
        text: &str,
        kind: Option<&str>,
    ) -> Result<RequirementView> {
        if !ids::is_requirement_key(key) {
            return Err(Error::Usage(format!(
                "`{key}` is not a requirement key; keys look like FR-042 or NFR-7 (capital letters, a dash, digits)"
            )));
        }
        nonempty("the requirement text", text)?;
        let id = ids::requirement_id(key);
        if self.reader().get_node(&id)?.is_some() {
            return Err(Error::Invalid(format!(
                "{key} already exists; use `resonance req edit`"
            )));
        }
        let kind = kind.unwrap_or_else(|| intent::kind_of(key)).to_string();
        let author = self.actor.id.clone();
        self.mutate(|tx| {
            let mut p = Props::new();
            p.insert("key".into(), json!(key));
            p.insert("text".into(), json!(text.trim()));
            p.insert("kind".into(), json!(kind));
            p.insert("specStatus".into(), Value::Null);
            p.insert("status".into(), json!("active"));
            p.insert("origin".into(), json!("authored"));
            p.insert("author".into(), json!(author));
            p.insert("createdAt".into(), json!(hlc::rfc3339(hlc::now_ms())));
            tx.create_node(&id, NodeType::Requirement, Layer::Authored, p)
        })?;
        self.requirement(&id)
    }

    fn requirement(&self, id: &str) -> Result<RequirementView> {
        let n = self
            .reader()
            .get_node(id)?
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        Ok(requirement_view(&n))
    }

    fn authored_requirement(&self, reference: &str) -> Result<Node> {
        let n = self.resolve_node(reference)?;
        if n.ntype != NodeType::Requirement {
            return Err(Error::Invalid(format!(
                "{reference} is a {}, not a requirement",
                n.ntype
            )));
        }
        if n.str_prop("origin") == Some("import") {
            let src = n
                .props
                .get("source")
                .map(|s| format!("{}:{}", s["path"].as_str().unwrap_or("?"), s["line"]))
                .unwrap_or_default();
            return Err(Error::Invalid(format!(
                "{} is imported from {src}; edit it there and Resonance will pick up the change",
                n.str_prop("key").unwrap_or(reference)
            )));
        }
        Ok(n)
    }

    /// Edit a requirement's text. Its links become stale (S7).
    pub fn req_edit(&mut self, reference: &str, text: &str) -> Result<RequirementView> {
        nonempty("the requirement text", text)?;
        let n = self.authored_requirement(reference)?;
        let id = n.id.clone();
        self.mutate(|tx| {
            if n.str_prop("text") != Some(text.trim()) {
                tx.set_prop(&id, "text", json!(text.trim()))?;
                links::refresh_statuses(tx, &id)?;
            }
            Ok(())
        })?;
        self.requirement(&id)
    }

    /// Retire a requirement; it is never deleted.
    pub fn req_retire(&mut self, reference: &str) -> Result<RequirementView> {
        let n = self.authored_requirement(reference)?;
        let id = n.id.clone();
        self.mutate(|tx| tx.set_prop(&id, "status", json!("retired")))?;
        self.requirement(&id)
    }

    /// Record a decision (FR-120).
    pub fn decision_add(&mut self, input: DecisionInput) -> Result<DecisionView> {
        let title = input.title.unwrap_or_default();
        let decision = input.decision.unwrap_or_default();
        nonempty("the decision title", &title)?;
        nonempty("the decision", &decision)?;
        let id = new_id();
        let author = self.actor.id.clone();
        self.mutate(|tx| {
            let mut p = Props::new();
            p.insert("title".into(), json!(title.trim()));
            p.insert("decision".into(), json!(decision.trim()));
            p.insert("context".into(), json!(input.context));
            p.insert("consequences".into(), json!(input.consequences));
            p.insert(
                "status".into(),
                json!(input.status.unwrap_or_else(|| "accepted".into())),
            );
            p.insert("author".into(), json!(author));
            p.insert("createdAt".into(), json!(hlc::rfc3339(hlc::now_ms())));
            tx.create_node(&id, NodeType::Decision, Layer::Authored, p)
        })?;
        Ok(decision_view(
            &self.reader().get_node(&id)?.expect("created"),
        ))
    }

    pub fn decision_edit(&mut self, reference: &str, input: DecisionInput) -> Result<DecisionView> {
        let n = self.resolve_node(reference)?;
        if n.ntype != NodeType::Decision {
            return Err(Error::Invalid(format!(
                "{reference} is a {}, not a decision",
                n.ntype
            )));
        }
        let id = n.id.clone();
        let fields = [
            ("title", input.title),
            ("decision", input.decision),
            ("context", input.context),
            ("consequences", input.consequences),
            ("status", input.status),
        ];
        if fields.iter().all(|(_, v)| v.is_none()) {
            return Err(Error::Usage(
                "nothing to change; give at least one field".into(),
            ));
        }
        self.mutate(|tx| {
            for (k, v) in fields {
                if let Some(v) = v {
                    if (k == "title" || k == "decision") && v.trim().is_empty() {
                        return Err(Error::Usage(format!("the decision {k} must not be empty")));
                    }
                    tx.set_prop(&id, k, json!(v.trim()))?;
                }
            }
            links::refresh_statuses(tx, &id)?;
            Ok(())
        })?;
        Ok(decision_view(
            &self.reader().get_node(&id)?.expect("exists"),
        ))
    }

    // ----- Code units and tests ------------------------------------------------------------

    /// Compute an anchor for a region of a tracked text file.
    fn anchor_for(
        &self,
        region: &str,
        match_text: Option<&str>,
        fallback_path: Option<&str>,
    ) -> Result<Anchor> {
        let spec = anchors::parse_region(region)?;
        let path = if spec.path.is_empty() {
            fallback_path
                .map(String::from)
                .ok_or_else(|| Error::Usage("give a file path".into()))?
        } else {
            self.repo.relative_path(&self.cwd, &spec.path)?
        };
        let file = self.reader().get_node(&ids::file_id(&path))?;
        if !file.is_some_and(|f| !f.retired) {
            return Err(Error::Invalid(format!(
                "{path} is not a file Resonance tracks (it is missing, ignored by .gitignore, or excluded by the built-in or configured ignores)"
            )));
        }
        let bytes = fs::read(abs_path(&self.repo.root, &path))
            .map_err(|e| Error::io(format!("reading {path}"), e))?;
        let content = String::from_utf8(bytes).map_err(|_| {
            Error::Invalid(format!(
                "{path} is not UTF-8 text; code units can only be declared on text files"
            ))
        })?;
        let (start, end) = match (spec.lines, match_text) {
            (Some(_), Some(_)) => {
                return Err(Error::Usage(
                    "give either a line range or --match, not both".into(),
                ));
            }
            (Some((a, b)), None) => anchors::lines_to_bytes(&content, a, b)?,
            (None, Some(m)) => anchors::find_unique(&content, m)?,
            (None, None) => {
                return Err(Error::Usage(format!(
                    "give a line range ({path}:START-END) or an exact snippet with --match"
                )));
            }
        };
        if start == end {
            return Err(Error::Invalid("the region is empty".into()));
        }
        Ok(Anchor::at(&path, &content, start, end, Method::Declared))
    }

    fn check_name(name: &str) -> Result<()> {
        nonempty("the name", name)?;
        if name.contains("::") || name.contains('#') || name.trim() != name {
            return Err(Error::Usage(format!(
                "`{name}` cannot be used as a name: names cannot contain `::` or `#`, or start or end with spaces"
            )));
        }
        Ok(())
    }

    /// Declare a code unit or test as a named region of a file (MVP-013, MVP-017).
    pub fn unit_add(&mut self, input: UnitInput) -> Result<UnitView> {
        Self::check_name(&input.name)?;
        if input.test {
            if input.run.as_deref().is_none_or(|r| r.trim().is_empty())
                && self.config.test.command.trim().is_empty()
            {
                return Err(Error::Usage(
                    "--run is required: give the exact command that runs this test alone (or set test.command in .resonance/config.toml)".into(),
                ));
            }
        } else if input.run.is_some() {
            return Err(Error::Usage(
                "--run is only for tests; use `resonance test add`".into(),
            ));
        }
        let anchor = self.anchor_for(&input.region, input.match_text.as_deref(), None)?;
        let id = new_id();
        let ntype = if input.test {
            NodeType::Test
        } else {
            NodeType::CodeUnit
        };
        let kind = input.kind.unwrap_or_else(|| {
            if input.test {
                "test".into()
            } else {
                "block".into()
            }
        });
        let author = self.actor.id.clone();
        let provenance = self.provenance();
        self.mutate(|tx| {
            let mut p = Props::new();
            p.insert("name".into(), json!(input.name));
            p.insert("kind".into(), json!(kind));
            p.insert("path".into(), json!(anchor.path));
            p.insert("anchor".into(), serde_json::to_value(&anchor)?);
            if let Some(run) = input.run.as_deref().filter(|r| !r.trim().is_empty()) {
                p.insert("run".into(), json!(run));
            }
            p.insert("author".into(), json!(author));
            p.insert("provenance".into(), json!(provenance));
            p.insert("createdAt".into(), json!(hlc::rfc3339(hlc::now_ms())));
            tx.create_node(&id, ntype, Layer::Authored, p)?;
            recompute_containment(tx, &anchor.path)
        })?;
        self.unit(&id)
    }

    fn unit(&self, id: &str) -> Result<UnitView> {
        let n = self
            .reader()
            .get_node(id)?
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        unit_view(self.reader(), &n)
    }

    fn anchored(&self, reference: &str) -> Result<Node> {
        let n = self.resolve_node(reference)?;
        if !n.ntype.is_anchored() {
            return Err(Error::Invalid(format!(
                "{reference} is a {}, not a code unit or test",
                n.ntype
            )));
        }
        if n.retired {
            return Err(Error::Invalid(format!("{reference} has been removed")));
        }
        Ok(n)
    }

    /// Re-anchor, rename or confirm a code unit or test (FR-037, FR-038).
    pub fn unit_edit(&mut self, reference: &str, edit: UnitEdit) -> Result<UnitView> {
        let n = self.anchored(reference)?;
        let old = anchor_of(&n).ok_or_else(|| Error::Store(format!("{} has no anchor", n.id)))?;
        if let Some(name) = &edit.name {
            Self::check_name(name)?;
        }
        if edit.run.is_some() && n.ntype != NodeType::Test {
            return Err(Error::Usage("--run is only for tests".into()));
        }
        let new_anchor = match (&edit.region, &edit.match_text) {
            (None, None) => None,
            (region, m) => Some(self.anchor_for(
                region.as_deref().unwrap_or(""),
                m.as_deref(),
                Some(&old.path),
            )?),
        };
        let confirm_only = new_anchor.is_none()
            && edit.name.is_none()
            && edit.kind.is_none()
            && edit.run.is_none();
        if confirm_only && old.state == AnchorState::Orphaned {
            return Err(Error::Invalid(format!(
                "{reference} is orphaned; give its new region, for example `resonance unit edit {reference} path:START-END`"
            )));
        }
        let id = n.id.clone();
        self.mutate(|tx| {
            if let Some(new) = &new_anchor {
                if new.path != old.path {
                    put_anchor(tx, &id, &old, new)?;
                } else {
                    // An explicit re-anchor is an identity decision made by a person or agent.
                    tx.apply(
                        kind::IDENTITY_RETARGET,
                        json!({"id": id, "from": old.location(), "to": new.location(), "confidence": 1.0, "anchor": new}),
                    )?;
                }
                if old.state == AnchorState::Orphaned {
                    links::restore_links(tx, &id)?;
                }
                links::refresh_statuses(tx, &id)?;
                recompute_containment(tx, &old.path)?;
                if new.path != old.path {
                    recompute_containment(tx, &new.path)?;
                }
            } else if confirm_only {
                let mut confirmed = old.clone();
                confirmed.confidence = 1.0;
                confirmed.method = Method::Confirmed;
                tx.set_prop(&id, "anchor", serde_json::to_value(&confirmed)?)?;
            }
            if let Some(name) = &edit.name {
                tx.set_prop(&id, "name", json!(name))?;
            }
            if let Some(k) = &edit.kind {
                tx.set_prop(&id, "kind", json!(k))?;
            }
            if let Some(run) = &edit.run {
                tx.set_prop(&id, "run", json!(run))?;
            }
            Ok(())
        })?;
        self.unit(&id)
    }

    /// Remove a code unit or test. It is retired, never deleted; its links become orphaned.
    pub fn unit_remove(&mut self, reference: &str) -> Result<UnitView> {
        let n = self.anchored(reference)?;
        let id = n.id.clone();
        let path = n.str_prop("path").unwrap_or_default().to_string();
        self.mutate(|tx| {
            tx.retire_node(&id)?;
            links::orphan_links(tx, &id)?;
            recompute_containment(tx, &path)
        })?;
        self.unit(&id)
    }

    /// Declare that one unit became several (FR-033, MVP-024). Its links and annotations are
    /// copied to each new unit as `asserted`, and it is retired.
    pub fn unit_split(&mut self, reference: &str, into: &[String]) -> Result<IdentityChange> {
        if into.is_empty() {
            return Err(Error::Usage(
                "give the units it was split into with --into".into(),
            ));
        }
        let old = self.anchored(reference)?;
        let mut parts = Vec::new();
        for r in into {
            let n = self.anchored(r)?;
            require_live(&n, r)?;
            if n.id == old.id || parts.iter().any(|p: &Node| p.id == n.id) {
                return Err(Error::Usage(format!(
                    "{r} is listed twice, or is the unit being split"
                )));
            }
            parts.push(n);
        }
        let provenance = self.provenance();
        let author = self.actor.id.clone();
        let from = anchor_of(&old).map(|a| a.location());
        let moved = self.mutate(|tx| {
            let ids: Vec<&str> = parts.iter().map(|p| p.id.as_str()).collect();
            tx.apply(
                kind::IDENTITY_SPLIT,
                json!({"id": old.id, "into": ids, "from": from}),
            )?;
            let mut moved = 0;
            for e in links::links_of(tx.r(), &old.id)? {
                for part in &parts {
                    moved += copy_link(tx, &e, &old.id, &part.id, provenance, &author)?;
                }
                tx.remove_edge(&e.id)?;
            }
            let mut paths: BTreeSet<String> = parts
                .iter()
                .filter_map(|p| p.str_prop("path").map(String::from))
                .collect();
            paths.extend(old.str_prop("path").map(String::from));
            for p in paths {
                recompute_containment(tx, &p)?;
            }
            Ok(moved)
        })?;
        let r = self.reader();
        Ok(IdentityChange {
            retired: vec![unit_view(r, &r.get_node(&old.id)?.expect("exists"))?],
            into: parts
                .iter()
                .map(|p| unit_view(r, p))
                .collect::<Result<Vec<_>>>()?,
            links_moved: moved,
        })
    }

    /// Declare that several units became one (FR-033, MVP-024). Their links and annotations
    /// move to the remaining unit, and they are retired.
    pub fn unit_merge(&mut self, references: &[String], into: &str) -> Result<IdentityChange> {
        if references.is_empty() {
            return Err(Error::Usage("give the units to merge".into()));
        }
        let target = self.anchored(into)?;
        require_live(&target, into)?;
        let mut sources = Vec::new();
        for r in references {
            let n = self.anchored(r)?;
            if n.id == target.id || sources.iter().any(|p: &Node| p.id == n.id) {
                return Err(Error::Usage(format!(
                    "{r} is listed twice, or is the unit merged into"
                )));
            }
            sources.push(n);
        }
        let provenance = self.provenance();
        let author = self.actor.id.clone();
        let moved = self.mutate(|tx| {
            let ids: Vec<&str> = sources.iter().map(|p| p.id.as_str()).collect();
            tx.apply(kind::IDENTITY_MERGE, json!({"ids": ids, "into": target.id}))?;
            let mut moved = 0;
            let mut paths = BTreeSet::new();
            for source in &sources {
                for e in links::links_of(tx.r(), &source.id)? {
                    moved += copy_link(tx, &e, &source.id, &target.id, provenance, &author)?;
                    tx.remove_edge(&e.id)?;
                }
                paths.extend(source.str_prop("path").map(String::from));
            }
            paths.extend(target.str_prop("path").map(String::from));
            for p in paths {
                recompute_containment(tx, &p)?;
            }
            Ok(moved)
        })?;
        let r = self.reader();
        Ok(IdentityChange {
            retired: sources
                .iter()
                .map(|s| unit_view(r, &r.get_node(&s.id)?.expect("exists")))
                .collect::<Result<Vec<_>>>()?,
            into: vec![unit_view(r, &target)?],
            links_moved: moved,
        })
    }

    /// Code units and tests, optionally under a path (a file or a directory).
    pub fn unit_list(&self, path: Option<&str>) -> Result<Vec<UnitView>> {
        let prefix = match path {
            Some(p) => Some(self.repo.relative_path(&self.cwd, p)?),
            None => None,
        };
        let r = self.reader();
        let mut out = Vec::new();
        for t in [NodeType::CodeUnit, NodeType::Test] {
            for n in r.find_nodes(&NodeQuery::of_type(t))? {
                let p = n.str_prop("path").unwrap_or_default();
                if let Some(pre) = &prefix {
                    let under = pre.is_empty() || p == pre || p.starts_with(&format!("{pre}/"));
                    if !under {
                        continue;
                    }
                }
                out.push(unit_view(r, &n)?);
            }
        }
        out.sort_by(|a, b| {
            let key = |u: &UnitView| {
                (
                    u.location
                        .as_ref()
                        .map(|l| l.path.clone())
                        .unwrap_or_default(),
                    u.location.as_ref().map(|l| l.start_line).unwrap_or(0),
                    u.address.clone(),
                )
            };
            key(a).cmp(&key(b))
        });
        Ok(out)
    }

    // ----- Links and annotations -----------------------------------------------------------

    /// Link two nodes. Agent links need a confidence and start as `asserted` (FR-134).
    pub fn link(
        &mut self,
        src: &str,
        etype: &str,
        dst: &str,
        confidence: Option<f64>,
    ) -> Result<LinkResult> {
        let ext_type = if etype.contains(':') {
            check_ext_name(etype)?;
            Some(etype.to_string())
        } else {
            None
        };
        let etype: EdgeType = match &ext_type {
            Some(_) => EdgeType::Extension,
            None => etype.parse().map_err(Error::Usage)?,
        };
        if etype == EdgeType::Extension && ext_type.is_none() {
            return Err(Error::Usage(
                "give an extension link a namespaced type such as acme:deploys (FR-007)".into(),
            ));
        }
        match etype {
            EdgeType::Annotates => {
                return Err(Error::Usage(
                    "use `resonance annotate` to attach annotations".into(),
                ));
            }
            EdgeType::Contains | EdgeType::Evidences => {
                return Err(Error::Usage(format!(
                    "{etype} links are recorded by Resonance itself; create satisfies, verifies or constrains links"
                )));
            }
            _ => {}
        }
        if let Some(c) = confidence
            && !(0.0..=1.0).contains(&c)
        {
            return Err(Error::Usage("confidence must be between 0 and 1".into()));
        }
        if self.actor.kind == ActorKind::Agent && confidence.is_none() {
            return Err(Error::Usage(
                "links made by agents need a confidence between 0 and 1 (FR-134)".into(),
            ));
        }
        let s = self.resolve_node(src)?;
        let d = self.resolve_node(dst)?;
        check_link_types(etype, &s, &d)?;
        require_live(&s, src)?;
        require_live(&d, dst)?;
        if let Some(existing) = self
            .reader()
            .find_edges(&EdgeQuery::from(&s.id).of_type(etype))?
            .into_iter()
            .find(|e| e.dst == d.id && e.str_prop("extType") == ext_type.as_deref())
        {
            return self.link_result(&existing, false);
        }
        let provenance = self.provenance();
        let author = self.actor.id.clone();
        let edge = self.mutate(|tx| {
            let mut props = new_link_props(tx.r(), &s.id, &d.id, provenance, &author, confidence)?;
            if let Some(t) = &ext_type {
                props.insert("extType".into(), json!(t));
            }
            let edge = new_link(etype, &s.id, &d.id, props);
            tx.add_edge(&edge)?;
            Ok(edge)
        })?;
        self.link_result(&edge, true)
    }

    fn link_result(&self, edge: &Edge, created: bool) -> Result<LinkResult> {
        let r = self.reader();
        Ok(LinkResult {
            source: node_ref_by_id(r, &edge.src)?,
            link: link_view(r, edge, &edge.src)?,
            created,
        })
    }

    fn authored_link(&self, reference: &str) -> Result<Edge> {
        let e = self.resolve_edge(reference)?;
        if !LINK_TYPES.contains(&e.etype) || e.layer != Layer::Authored {
            return Err(Error::Invalid(format!(
                "{} links are recorded by Resonance and cannot be changed by hand",
                e.etype
            )));
        }
        if e.removed {
            return Err(Error::Invalid(format!("link {} has been removed", e.id)));
        }
        Ok(e)
    }

    /// Remove a link. It is kept for history.
    pub fn unlink(&mut self, reference: &str) -> Result<LinkResult> {
        let e = self.authored_link(reference)?;
        let id = e.id.clone();
        self.mutate(|tx| tx.remove_edge(&id))?;
        let e = self.reader().get_edge(&id)?.expect("exists");
        self.link_result(&e, false)
    }

    /// Return a stale link to `asserted`, refreshing its endpoint hashes (MVP-012).
    pub fn confirm(&mut self, reference: &str) -> Result<LinkResult> {
        let e = self.authored_link(reference)?;
        match e.status {
            LinkStatus::Orphaned => {
                return Err(Error::Invalid(format!(
                    "link {} is orphaned; re-attach it with `resonance reattach {} <target>`",
                    e.id, e.id
                )));
            }
            LinkStatus::Stale => {
                let edge = e.clone();
                self.mutate(|tx| {
                    tx.set_edge(&edge.id, "status", json!(LinkStatus::Asserted))?;
                    links::refresh_hashes(tx, &edge)
                })?;
            }
            LinkStatus::Asserted | LinkStatus::Verified => {}
        }
        let e = self.reader().get_edge(&e.id)?.expect("exists");
        self.link_result(&e, false)
    }

    /// Annotate a node (S4). A call-out can name a task.
    pub fn annotate(
        &mut self,
        target: &str,
        kind: &str,
        text: &str,
        task: Option<&str>,
    ) -> Result<AnnotationView> {
        let kind = match kind {
            "callout" | "call_out" => "call-out",
            k => k,
        };
        if !ANNOTATION_KINDS.contains(&kind) {
            return Err(Error::Usage(format!(
                "unknown annotation kind `{kind}` (expected {})",
                ANNOTATION_KINDS.join(", ")
            )));
        }
        nonempty("the annotation text", text)?;
        if task.is_some() && kind != "call-out" {
            return Err(Error::Usage(
                "--task is for call-outs (--kind call-out)".into(),
            ));
        }
        let t = self.resolve_node(target)?;
        if matches!(t.ntype, NodeType::Evidence | NodeType::Annotation) {
            return Err(Error::Invalid(format!("a {} cannot be annotated", t.ntype)));
        }
        require_live(&t, target)?;
        let id = new_id();
        let author = self.actor.id.clone();
        let provenance = self.provenance();
        let target_id = t.id.clone();
        self.mutate(|tx| {
            let mut p = Props::new();
            p.insert("text".into(), json!(text));
            p.insert("kind".into(), json!(kind));
            if let Some(task) = task {
                p.insert("task".into(), json!(task));
            }
            p.insert("author".into(), json!(author));
            p.insert("provenance".into(), json!(provenance));
            p.insert("createdAt".into(), json!(hlc::rfc3339(hlc::now_ms())));
            tx.create_node(&id, NodeType::Annotation, Layer::Authored, p)?;
            let props = new_link_props(tx.r(), &id, &target_id, provenance, &author, None)?;
            tx.add_edge(&new_link(EdgeType::Annotates, &id, &target_id, props))
        })?;
        let r = self.reader();
        let n = r.get_node(&id)?.expect("created");
        let e = r
            .find_edges(&EdgeQuery::from(&id).of_type(EdgeType::Annotates))?
            .into_iter()
            .next();
        annotation_view(r, &n, e.as_ref(), false)
    }

    /// Re-attach an orphaned (or wrongly attached) link or annotation (FR-037).
    pub fn reattach(&mut self, reference: &str, target: &str) -> Result<LinkResult> {
        let edge = match self.resolve(reference)? {
            Resolved::Edge(_) => self.authored_link(reference)?,
            Resolved::Node(n) if n.ntype == NodeType::Annotation => self
                .reader()
                .find_edges(&EdgeQuery::from(&n.id).of_type(EdgeType::Annotates))?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    Error::Invalid(format!("annotation {} is not attached to anything", n.id))
                })?,
            Resolved::Node(n) => {
                return Err(Error::Invalid(format!(
                    "{reference} is a {}; reattach takes a link ID or an annotation",
                    n.ntype
                )));
            }
        };
        let to = self.resolve_node(target)?;
        require_live(&to, target)?;
        let r = self.reader();
        let live = |id: &str| -> Result<bool> { Ok(r.get_node(id)?.is_some_and(|n| is_live(&n))) };
        let (src_live, dst_live) = (live(&edge.src)?, live(&edge.dst)?);
        let end = match (src_live, dst_live) {
            (false, true) => "src",
            (true, false) => "dst",
            _ => match edge.etype {
                EdgeType::Satisfies | EdgeType::Verifies => "src",
                _ => "dst",
            },
        };
        let existing = |id: &str| -> Result<Node> {
            r.get_node(id)?.ok_or_else(|| {
                Error::Store(format!("link {} refers to unknown node {id}", edge.id))
            })
        };
        let (src, dst) = if end == "src" {
            (to.clone(), existing(&edge.dst)?)
        } else {
            (existing(&edge.src)?, to.clone())
        };
        check_link_types(edge.etype, &src, &dst)?;
        let from = if end == "src" {
            edge.src.clone()
        } else {
            edge.dst.clone()
        };
        let id = edge.id.clone();
        let to_id = to.id.clone();
        self.mutate(|tx| {
            let (sh, dh) = links::endpoint_hashes(tx.r(), &src.id, &dst.id)?;
            tx.apply(
                kind::IDENTITY_RETARGET,
                json!({
                    "edgeId": id, "end": end, "from": from, "to": to_id,
                    "status": LinkStatus::Asserted,
                    "props": {"srcHash": sh, "dstHash": dh, "priorStatus": null},
                }),
            )
        })?;
        let e = self.reader().get_edge(&edge.id)?.expect("exists");
        self.link_result(&e, false)
    }

    // ----- Reading --------------------------------------------------------------------------

    pub fn show(&self, reference: &str) -> Result<ShowView> {
        let target = self.resolve(reference)?;
        query::show::show(self.reader(), &target)
    }

    pub fn trace(&self, reference: &str) -> Result<TraceView> {
        let n = self.resolve_node(reference)?;
        query::trace::trace(self.reader(), &n)
    }

    pub fn why(&self, reference: &str) -> Result<WhyView> {
        let n = self.resolve_node(reference)?;
        query::why::why(self.reader(), &n)
    }

    pub fn search(
        &self,
        text: &str,
        ntype: Option<&str>,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let ntype = match ntype {
            Some(t) => Some(t.parse::<NodeType>().map_err(Error::Usage)?),
            None => None,
        };
        query::search::search(self.reader(), text, ntype, status, limit)
    }

    pub fn unverified(&self) -> Result<Vec<Unverified>> {
        query::search::unverified(self.reader())
    }

    pub fn context(&self, references: &[String], task: Option<&str>) -> Result<ContextPack> {
        if references.is_empty() {
            return Err(Error::Usage("give at least one requirement".into()));
        }
        let nodes = references
            .iter()
            .map(|r| self.resolve_node(r))
            .collect::<Result<Vec<_>>>()?;
        query::context::context(self.reader(), &nodes, task)
    }

    // ----- Verification ---------------------------------------------------------------------

    /// Run the tests linked to a requirement or unit, a single test, or the test of one
    /// `verifies` link, recording evidence (FR-135, FR-004).
    pub fn run_tests(&mut self, reference: &str) -> Result<TestReport> {
        let r = self.reader();
        let tests: Vec<Node> = match self.resolve(reference)? {
            Resolved::Node(n) if n.ntype == NodeType::Test => vec![n],
            Resolved::Node(n) if matches!(n.ntype, NodeType::Requirement | NodeType::CodeUnit) => {
                let mut seen = BTreeSet::new();
                let mut out = Vec::new();
                for e in r.find_edges(&EdgeQuery::to(&n.id).of_type(EdgeType::Verifies))? {
                    if e.status == LinkStatus::Orphaned || !seen.insert(e.src.clone()) {
                        continue;
                    }
                    if let Some(t) = r.get_node(&e.src)?
                        && !t.retired
                    {
                        out.push(t);
                    }
                }
                out
            }
            Resolved::Node(n) => {
                return Err(Error::Invalid(format!(
                    "{reference} is a {}; `test` takes a requirement, a code unit, a test or a verifies link",
                    n.ntype
                )));
            }
            Resolved::Edge(e) if e.etype == EdgeType::Verifies => {
                r.get_node(&e.src)?.into_iter().collect()
            }
            Resolved::Edge(e) => {
                return Err(Error::Invalid(format!(
                    "link {} is a {} link, not verifies",
                    e.id, e.etype
                )));
            }
        };
        let mut report = TestReport {
            target: reference.to_string(),
            runs: Vec::new(),
            passed: 0,
            failed: 0,
            errors: 0,
        };
        let commit = self.repo.head_commit();
        for test in tests {
            let run = self.run_one(&test, commit.clone())?;
            match run.outcome.result {
                TestResult::Pass => report.passed += 1,
                TestResult::Fail => report.failed += 1,
                TestResult::Error => report.errors += 1,
            }
            report.runs.push(run);
        }
        Ok(report)
    }

    fn run_one(&mut self, test: &Node, commit: Option<String>) -> Result<TestRun> {
        let view = unit_view(self.reader(), test)?;
        let command = match test.str_prop("run") {
            Some(c) => Some(c.to_string()),
            None if !self.config.test.command.trim().is_empty() => Some(verify::substitute(
                &self.config.test.command,
                &view.name,
                view.location
                    .as_ref()
                    .map(|l| l.path.as_str())
                    .unwrap_or(""),
                &view.address,
            )),
            None => None,
        };
        let outcome = match &command {
            Some(cmd) => verify::run_command(
                cmd,
                &self.repo.root,
                Duration::from_secs(self.config.test.timeout_seconds.max(1)),
                &self.config.test.zero_tests_pattern,
            ),
            None => RunOutcome {
                result: TestResult::Error,
                exit_code: None,
                output_tail: String::new(),
                duration_ms: 0,
                error: Some("the test has no --run command and test.command is not set".into()),
            },
        };
        let edges: Vec<Edge> = self
            .reader()
            .find_edges(&EdgeQuery::from(&test.id).of_type(EdgeType::Verifies))?
            .into_iter()
            .filter(|e| e.status != LinkStatus::Orphaned)
            .collect();
        let evidence_id = new_id();
        let now = hlc::now_ms();
        let mut props = Props::new();
        props.insert("kind".into(), json!("test"));
        props.insert("result".into(), json!(outcome.result.as_str()));
        props.insert("commit".into(), json!(commit));
        props.insert("timestamp".into(), json!(hlc::rfc3339(now)));
        props.insert("toolVersion".into(), json!(format!("resonance {VERSION}")));
        props.insert("exitCode".into(), json!(outcome.exit_code));
        props.insert("outputTail".into(), json!(outcome.output_tail));
        props.insert("durationMs".into(), json!(outcome.duration_ms));
        props.insert("command".into(), json!(command));
        props.insert("test".into(), json!(test.id));
        if let Some(e) = &outcome.error {
            props.insert("error".into(), json!(e));
        }
        let result = outcome.result;
        let edge_ids: Vec<String> = edges.iter().map(|e| e.id.clone()).collect();
        self.mutate(|tx| {
            tx.apply(
                kind::EVIDENCE_RECORD,
                json!({"id": evidence_id, "props": props, "edges": edge_ids}),
            )?;
            for e in &edges {
                match result {
                    TestResult::Pass => {
                        tx.set_edge(&e.id, "status", json!(LinkStatus::Verified))?;
                        links::refresh_hashes(tx, e)?;
                    }
                    TestResult::Fail => {
                        tx.set_edge(&e.id, "status", json!(LinkStatus::Asserted))?;
                    }
                    TestResult::Error => {}
                }
            }
            Ok(())
        })?;
        let r = self.reader();
        let mut links = Vec::new();
        for e in &edges {
            if let Some(cur) = r.get_edge(&e.id)? {
                links.push(link_view(r, &cur, &cur.src)?);
            }
        }
        Ok(TestRun {
            test: view,
            command,
            outcome,
            evidence_id,
            links,
        })
    }

    // ----- Lifecycle ------------------------------------------------------------------------

    /// Write the authored and identity data as edit-algebra operations (FR-166).
    pub fn export(&self, out: &mut dyn Write) -> Result<usize> {
        export::write(self.reader(), &self.repo.name(), out)
    }

    /// Drop derived data and rebuild it from the files and the authored data (FR-023).
    pub fn rebuild_derived(&mut self) -> Result<ReconcileReport> {
        self.mutate(|tx| {
            tx.w().clear_layer(Layer::Derived)?;
            tx.w().clear_files()
        })?;
        self.reconcile(ReconcileOptions {
            full: true,
            force_intent: true,
            rebuild_containment: true,
        })
    }

    /// Paths of the intent sources, for messages.
    /// Restore authored and identity data from an engine-neutral export (FR-166). Operations
    /// that would create something that already exists are skipped, so restoring into a
    /// non-empty graph only adds what is missing.
    pub fn restore(&mut self, jsonl: &str) -> Result<RestoreReport> {
        let mut ops: Vec<crate::ops::Op> = Vec::new();
        for (i, line) in jsonl.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(line)
                .map_err(|e| Error::Invalid(format!("line {}: not JSON: {e}", i + 1)))?;
            if i == 0 {
                if v["format"] != export::FORMAT {
                    return Err(Error::Invalid(
                        "this is not a Resonance export (the first line is not its header)".into(),
                    ));
                }
                continue;
            }
            ops.push(
                serde_json::from_value(v)
                    .map_err(|e| Error::Invalid(format!("line {}: {e}", i + 1)))?,
            );
        }
        let mut report = RestoreReport::default();
        self.store.write(GraphScope::Primary, &mut |w| {
            report = RestoreReport::default();
            for op in &ops {
                let id = op.payload["id"].as_str().unwrap_or_default();
                let exists = match op.kind.as_str() {
                    kind::NODE_CREATE => w.get_node(id)?.is_some(),
                    kind::EDGE_ADD => w.get_edge(id)?.is_some(),
                    _ => false,
                };
                if exists {
                    report.skipped += 1;
                    continue;
                }
                if crate::ops::apply(w, op).is_ok() {
                    w.append_op(op)?;
                    report.applied += 1;
                } else {
                    report.skipped += 1;
                }
            }
            Ok(())
        })?;
        self.after_write()?;
        report.reconcile = Some(self.reconcile(ReconcileOptions {
            force_intent: true,
            rebuild_containment: true,
            ..Default::default()
        })?);
        Ok(report)
    }

    // ----- Spec items (FR-120, §8.22) ---------------------------------------------------------

    /// The fields each spec-item type accepts, besides `key`.
    fn spec_fields(ntype: NodeType) -> &'static [&'static str] {
        match ntype {
            NodeType::Requirement => &["text", "kind", "priority", "status"],
            NodeType::Decision => &["title", "decision", "context", "consequences", "status"],
            NodeType::Design => &["title", "text", "status"],
            NodeType::Constraint => &["title", "text", "scope", "status"],
            _ => &[],
        }
    }

    fn check_status(ntype: NodeType, status: &str) -> Result<()> {
        let allowed: &[&str] = match ntype {
            NodeType::Requirement => &["draft", "active", "superseded", "retired"],
            NodeType::Decision => &[
                "proposed",
                "accepted",
                "superseded",
                "deprecated",
                "retired",
            ],
            _ => &["draft", "active", "superseded", "retired"],
        };
        if allowed.contains(&status) {
            Ok(())
        } else {
            Err(Error::Usage(format!(
                "`{status}` is not a {ntype} status (expected one of: {})",
                allowed.join(", ")
            )))
        }
    }

    /// Create a spec item: a requirement, decision, design or constraint (FR-120). `fields`
    /// are the type's fields (see `spec_fields`); requirements need `key` and `text`,
    /// decisions `title` and `decision`, designs `title`, constraints `text`.
    pub fn spec_add(
        &mut self,
        ntype: NodeType,
        key: Option<&str>,
        fields: &std::collections::BTreeMap<String, String>,
    ) -> Result<query::SpecItemView> {
        let allowed = Self::spec_fields(ntype);
        if allowed.is_empty() {
            return Err(Error::Usage(format!("a {ntype} is not a spec item")));
        }
        for k in fields.keys() {
            if !allowed.contains(&k.as_str()) {
                return Err(Error::Usage(format!(
                    "a {ntype} has no field `{k}` (fields: {})",
                    allowed.join(", ")
                )));
            }
        }
        if let Some(st) = fields.get("status") {
            Self::check_status(ntype, st)?;
        }
        let get = |k: &str| {
            fields
                .get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let id = match ntype {
            NodeType::Requirement => {
                let key = key.ok_or_else(|| {
                    Error::Usage("a requirement needs a key such as FR-042".into())
                })?;
                let text =
                    get("text").ok_or_else(|| Error::Usage("a requirement needs text".into()))?;
                let mut v = self.req_add(key, &text, get("kind").as_deref())?;
                let mut extra = Vec::new();
                for k in ["priority", "status"] {
                    if let Some(val) = get(k) {
                        extra.push((k, val));
                    }
                }
                if !extra.is_empty() {
                    let id = v.id.clone();
                    self.mutate(|tx| {
                        for (k, val) in &extra {
                            tx.set_prop(&id, k, json!(val))?;
                        }
                        Ok(())
                    })?;
                    v = self.requirement(&id)?;
                }
                v.id
            }
            NodeType::Decision => {
                let mut v = self.decision_add(DecisionInput {
                    title: get("title"),
                    decision: get("decision"),
                    context: get("context"),
                    consequences: get("consequences"),
                    status: get("status"),
                })?;
                if let Some(k) = key {
                    let id = v.id.clone();
                    self.mutate(|tx| tx.set_prop(&id, "key", json!(k)))?;
                    v.id = id;
                }
                v.id
            }
            NodeType::Design | NodeType::Constraint => {
                let title = get("title");
                let text = get("text");
                if ntype == NodeType::Design && title.is_none() {
                    return Err(Error::Usage("a design needs a title".into()));
                }
                if ntype == NodeType::Constraint && text.is_none() {
                    return Err(Error::Usage("a constraint needs its text".into()));
                }
                let prefix = if ntype == NodeType::Design {
                    "design"
                } else {
                    "constraint"
                };
                let id = match key {
                    Some(k) => {
                        if k.trim().is_empty() || k.contains(char::is_whitespace) {
                            return Err(Error::Usage(format!("`{k}` cannot be used as a key")));
                        }
                        let id = format!("{prefix}:{k}");
                        if self.reader().get_node(&id)?.is_some() {
                            return Err(Error::Invalid(format!("{k} already exists")));
                        }
                        id
                    }
                    None => new_id(),
                };
                let author = self.actor.id.clone();
                let status = get("status").unwrap_or_else(|| "active".into());
                self.mutate(|tx| {
                    let mut p = Props::new();
                    if let Some(k) = key {
                        p.insert("key".into(), json!(k));
                    }
                    p.insert("title".into(), json!(title));
                    p.insert("text".into(), json!(text));
                    if ntype == NodeType::Constraint {
                        p.insert("scope".into(), json!(get("scope")));
                    }
                    p.insert("status".into(), json!(status));
                    p.insert("origin".into(), json!("authored"));
                    p.insert("author".into(), json!(author));
                    p.insert("createdAt".into(), json!(hlc::rfc3339(hlc::now_ms())));
                    tx.create_node(&id, ntype, Layer::Authored, p)
                })?;
                id
            }
            _ => unreachable!("checked above"),
        };
        self.spec_item(&id)
    }

    /// One spec item.
    pub fn spec_item(&self, reference: &str) -> Result<query::SpecItemView> {
        let n = match self.reader().get_node(reference)? {
            Some(n) => n,
            None => self.resolve_node(reference)?,
        };
        if !NodeType::SPEC_ITEMS.contains(&n.ntype) {
            return Err(Error::Invalid(format!(
                "{reference} is a {}, not a spec item",
                n.ntype
            )));
        }
        Ok(query::spec_item_view(&n))
    }

    /// Spec items, filtered by type, status and text (for the TUI and `spec list`).
    pub fn spec_list(
        &self,
        ntype: Option<NodeType>,
        status: Option<&str>,
        text: Option<&str>,
    ) -> Result<Vec<query::SpecItemView>> {
        let types: Vec<NodeType> = match ntype {
            Some(t) => vec![t],
            None => NodeType::SPEC_ITEMS.to_vec(),
        };
        let needle = text.map(str::to_lowercase).filter(|t| !t.is_empty());
        let mut out = Vec::new();
        for t in types {
            for n in self
                .reader()
                .find_nodes(&NodeQuery::of_type(t).including_retired())?
            {
                let v = query::spec_item_view(&n);
                if status.is_some_and(|s| s != v.status) {
                    continue;
                }
                if let Some(nd) = &needle {
                    let hay = format!(
                        "{} {} {}",
                        v.key.as_deref().unwrap_or_default(),
                        v.title,
                        v.text
                    )
                    .to_lowercase();
                    if !hay.contains(nd) {
                        continue;
                    }
                }
                out.push(v);
            }
        }
        out.sort_by(|a, b| {
            (a.ntype, a.key.clone().unwrap_or_default(), a.title.clone()).cmp(&(
                b.ntype,
                b.key.clone().unwrap_or_default(),
                b.title.clone(),
            ))
        });
        Ok(out)
    }

    /// Change a spec item's fields. Items owned by a source document are edited there
    /// (FR-272). Links to a changed item become stale (S7).
    pub fn spec_edit(
        &mut self,
        reference: &str,
        fields: &std::collections::BTreeMap<String, String>,
    ) -> Result<query::SpecItemView> {
        let item = self.spec_item(reference)?;
        if item.origin == "import" {
            let src = item
                .source
                .as_ref()
                .map(|s| format!("{}:{}", s["path"].as_str().unwrap_or("?"), s["line"]))
                .unwrap_or_default();
            return Err(Error::Invalid(format!(
                "{} is imported from {src}; edit it there and Resonance picks up the change",
                item.key.as_deref().unwrap_or(&item.title)
            )));
        }
        let allowed = Self::spec_fields(item.ntype);
        for k in fields.keys() {
            if !allowed.contains(&k.as_str()) {
                return Err(Error::Usage(format!(
                    "a {} has no field `{k}` (fields: {})",
                    item.ntype,
                    allowed.join(", ")
                )));
            }
        }
        if fields.is_empty() {
            return Err(Error::Usage(
                "nothing to change; give at least one field".into(),
            ));
        }
        if let Some(st) = fields.get("status") {
            Self::check_status(item.ntype, st)?;
        }
        let required: &[&str] = match item.ntype {
            NodeType::Requirement => &["text"],
            NodeType::Decision => &["title", "decision"],
            NodeType::Design => &["title"],
            _ => &["text"],
        };
        for k in required {
            if fields.get(*k).is_some_and(|v| v.trim().is_empty()) {
                return Err(Error::Usage(format!(
                    "the {} {k} must not be empty",
                    item.ntype
                )));
            }
        }
        let id = item.id.clone();
        let current = item.props.clone();
        self.mutate(|tx| {
            let mut changed = false;
            for (k, v) in fields {
                let v = v.trim();
                if current.get(k).and_then(Value::as_str) != Some(v) {
                    tx.set_prop(&id, k, json!(v))?;
                    changed = true;
                }
            }
            if changed {
                links::refresh_statuses(tx, &id)?;
            }
            Ok(())
        })?;
        self.spec_item(&id)
    }

    /// Retire a spec item: it is never deleted (FR-271). Returns it with its link count.
    pub fn spec_retire(&mut self, reference: &str) -> Result<(query::SpecItemView, usize)> {
        let item = self.spec_item(reference)?;
        if item.origin == "import" {
            return Err(Error::Invalid(format!(
                "{} is imported; remove it from its source document",
                item.key.as_deref().unwrap_or(&item.title)
            )));
        }
        let id = item.id.clone();
        let link_count = links::links_of(self.reader(), &id)?.len();
        self.mutate(|tx| {
            tx.set_prop(&id, "status", json!("retired"))?;
            links::refresh_statuses(tx, &id)?;
            Ok(())
        })?;
        Ok((self.spec_item(&id)?, link_count))
    }

    /// `new` supersedes `old` (§6.3): a `supersedes` link, and `old` becomes superseded, so
    /// its links become stale (S7).
    pub fn supersede(&mut self, old: &str, new: &str) -> Result<LinkResult> {
        let o = self.resolve_node(old)?;
        let n = self.resolve_node(new)?;
        if o.id == n.id {
            return Err(Error::Usage("an item cannot supersede itself".into()));
        }
        let result = self.link(
            &n.id,
            "supersedes",
            &o.id,
            (self.actor.kind == ActorKind::Agent).then_some(1.0),
        )?;
        let id = o.id.clone();
        let supersedes = result.link.id.clone();
        self.mutate(|tx| {
            tx.set_prop(&id, "status", json!("superseded"))?;
            // Links to what was superseded no longer hold as they were (S7 AC1).
            for e in links::links_of(tx.r(), &id)? {
                if e.id != supersedes
                    && matches!(e.status, LinkStatus::Asserted | LinkStatus::Verified)
                    && e.etype != EdgeType::Annotates
                {
                    tx.set_edge(&e.id, "status", json!(LinkStatus::Stale))?;
                }
            }
            Ok(())
        })?;
        Ok(result)
    }

    /// Create a node of a namespaced extension type (FR-007).
    pub fn ext_add(
        &mut self,
        ext_type: &str,
        name: &str,
        text: Option<&str>,
        props: &std::collections::BTreeMap<String, String>,
    ) -> Result<ShowView> {
        check_ext_name(ext_type)?;
        nonempty("the name", name)?;
        let id = new_id();
        let author = self.actor.id.clone();
        self.mutate(|tx| {
            let mut p = Props::new();
            for (k, v) in props {
                p.insert(k.clone(), json!(v));
            }
            p.insert("extType".into(), json!(ext_type));
            p.insert("name".into(), json!(name));
            p.insert("text".into(), json!(text));
            p.insert("author".into(), json!(author));
            p.insert("createdAt".into(), json!(hlc::rfc3339(hlc::now_ms())));
            tx.create_node(&id, NodeType::Extension, Layer::Authored, p)
        })?;
        self.show(&id)
    }

    /// Project graph-native requirements into `<dir>/requirements.md` in this spec's Markdown
    /// conventions (FR-121, one-shot). With `apply`, the file is written and becomes their
    /// import source, so the document owns them from then on; without, only a preview.
    pub fn intent_project(&mut self, dir: &str, apply: bool) -> Result<ProjectView> {
        let rel_dir = self.repo.relative_path(&self.cwd, dir)?;
        let rel = if rel_dir.is_empty() {
            "requirements.md".to_string()
        } else {
            format!("{rel_dir}/requirements.md")
        };
        let target = abs_path(&self.repo.root, &rel);
        if target.exists() {
            return Err(Error::Invalid(format!(
                "{rel} exists; project into a directory without a requirements.md"
            )));
        }
        let mut reqs: Vec<Node> = self
            .reader()
            .find_nodes(&NodeQuery::of_type(NodeType::Requirement))?
            .into_iter()
            .filter(|n| n.str_prop("origin") != Some("import"))
            .filter(|n| n.str_prop("status") != Some("retired"))
            .collect();
        reqs.sort_by(|a, b| a.str_prop("key").cmp(&b.str_prop("key")));
        let mut text = String::from(
            "# Requirements\n\nProjected from the Resonance graph (FR-121). This file now owns these\nrequirements: edit them here.\n\n",
        );
        let mut lines = Vec::new();
        for n in &reqs {
            let line = text.lines().count() + 1;
            let body = n.str_prop("text").unwrap_or_default().replace('\n', "\n  ");
            text.push_str(&format!(
                "- **{}** {body}\n",
                n.str_prop("key").unwrap_or_default()
            ));
            lines.push((n.id.clone(), line));
        }
        let keys: Vec<String> = reqs
            .iter()
            .map(|n| n.str_prop("key").unwrap_or_default().to_string())
            .collect();
        if !apply || reqs.is_empty() {
            return Ok(ProjectView {
                path: rel,
                keys,
                written: false,
            });
        }
        crate::recovery::write_file(&target, text.as_bytes())?;
        let path = rel.clone();
        self.mutate(|tx| {
            for (id, line) in &lines {
                tx.set_prop(id, "origin", json!("import"))?;
                tx.set_prop(id, "source", json!({"path": path, "line": line}))?;
            }
            Ok(())
        })?;
        let mut sources = self.config.intent.sources.clone();
        if !sources.contains(&rel) {
            sources.push(rel.clone());
            write_intent_sources(&self.repo.config_path(), &sources)?;
        }
        self.reconcile(ReconcileOptions {
            force_intent: true,
            ..Default::default()
        })?;
        Ok(ProjectView {
            path: rel,
            keys,
            written: true,
        })
    }

    // ----- Recovery and value (§8.18, FR-196, FR-240) ---------------------------------------

    /// Unresolved divergences, the detached pool, a held re-fold and recent recovery records.
    pub fn recover_list(&self) -> Result<RecoverView> {
        let r = self.reader();
        let mut detached = Vec::new();
        for (id, rec) in history::detached(r)? {
            let ops = history::find_delta(&self.repo, &id)
                .and_then(|p| history::Delta::read(&p).ok())
                .map(|d| d.ops.len())
                .unwrap_or(0);
            detached.push(DetachedDelta {
                id,
                commit: rec.commit,
                operations: ops,
            });
        }
        Ok(RecoverView {
            divergences: crate::recovery::list(r, false)?,
            detached,
            held: fold::held(r)?,
            records: crate::recovery::records(&self.repo)
                .into_iter()
                .take(20)
                .collect(),
        })
    }

    /// Put back what a resolution displaced (FR-192).
    pub fn recover_undo(&mut self, id: &str) -> Result<UndoView> {
        let (record, saved) = crate::recovery::undo(&self.repo, id)?;
        for d in crate::recovery::list(self.reader(), true)? {
            if d.recovery.as_deref() == Some(record.id.as_str()) && !d.resolved {
                crate::recovery::resolve(&mut *self.store, &d.id, "undone")?;
            }
        }
        let reconcile = self.reconcile(ReconcileOptions::default())?;
        Ok(UndoView {
            record,
            saved,
            reconcile,
        })
    }

    /// Mark a divergence handled.
    pub fn recover_dismiss(&mut self, id: &str, note: &str) -> Result<crate::recovery::Divergence> {
        if let Some(conflict) = self
            .reader()
            .get_node(id)?
            .filter(|n| n.ntype == NodeType::Conflict)
        {
            // An identity conflict (§9.4): closed by recording how it was resolved.
            let cid = conflict.id.clone();
            let note = note.to_string();
            self.mutate(|tx| {
                tx.set_prop(&cid, "status", json!("resolved"))?;
                tx.set_prop(&cid, "resolution", json!(note))
            })?;
            let mut d = crate::recovery::Divergence::new(
                crate::recovery::Class::C9,
                crate::recovery::Level::Info,
                format!("identity conflict {cid} resolved"),
            );
            d.resolution = "dismissed".into();
            return Ok(d);
        }
        let how = if note.is_empty() {
            format!("dismissed by {}", self.actor)
        } else {
            format!("dismissed by {}: {note}", self.actor)
        };
        crate::recovery::resolve(&mut *self.store, id, &how)
    }

    /// Attach a detached offline delta to `HEAD` (MVP-030).
    pub fn recover_attach(&mut self, delta: &str) -> Result<ReconcileReport> {
        let (id, mut rec) = self.detached_delta(delta)?;
        rec.commit = self.repo.head_commit();
        rec.detached = false;
        let key = format!("{}{id}", history::keys::DELTA_PREFIX);
        let text = serde_json::to_string(&rec)?;
        self.store
            .write(GraphScope::Primary, &mut |w| w.set_meta(&key, &text))?;
        self.after_write()?;
        let (cu, _) = self.catch_up(true)?;
        let mut report = reconcile(
            &mut *self.store,
            &self.repo.root,
            &self.config,
            ReconcileOptions::default(),
            &mut self.index,
        )?;
        self.after_write()?;
        report.history = Some(cu);
        Ok(report)
    }

    /// Drop a detached offline delta after exporting it outside the repository.
    pub fn recover_drop(&mut self, delta: &str, home: &Path) -> Result<PathBuf> {
        let (id, _) = self.detached_delta(delta)?;
        let path = history::find_delta(&self.repo, &id)
            .ok_or_else(|| Error::NotFound(format!("the file of delta {id}")))?;
        let dest = home
            .join(".resonance")
            .join("exports")
            .join(format!("{}-{id}.delta", self.repo.name()));
        if let Some(d) = dest.parent() {
            fs::create_dir_all(d).map_err(|e| Error::io("creating the exports directory", e))?;
        }
        fs::copy(&path, &dest).map_err(|e| Error::io("exporting the delta", e))?;
        fs::remove_file(&path).map_err(|e| Error::io("removing the delta", e))?;
        let key = format!("{}{id}", history::keys::DELTA_PREFIX);
        self.store
            .write(GraphScope::Primary, &mut |w| w.delete_meta(&key))?;
        Ok(dest)
    }

    fn detached_delta(&self, delta: &str) -> Result<(String, history::DeltaRecord)> {
        let matches: Vec<(String, history::DeltaRecord)> = history::detached(self.reader())?
            .into_iter()
            .filter(|(id, _)| id.starts_with(delta))
            .collect();
        match matches.len() {
            1 => Ok(matches.into_iter().next().expect("one")),
            0 => Err(Error::NotFound(format!("a detached delta {delta}"))),
            _ => Err(Error::Invalid(format!(
                "{delta} matches several detached deltas; give more of the ID"
            ))),
        }
    }

    /// What Resonance did for this developer, locally (FR-240, MVP-039).
    pub fn value(&self) -> Result<ValueView> {
        let counters = crate::recovery::counters(self.reader())?;
        let get = |k: &str| counters.get(k).copied().unwrap_or(0);
        let upgrade = if self.mode.shared() {
            None
        } else {
            Some(
                "Offline, you repair the graph yourself after teammates' changes. When you share \
                 it (`resonance share`) or join a shared graph (`resonance init` in a shared \
                 repository), their renames arrive as identity operations, annotations are \
                 shared, and your commits are checked for consistency."
                    .to_string(),
            )
        };
        Ok(ValueView {
            traces: get("served.trace"),
            whys: get("served.why"),
            context_packs: get("served.context"),
            searches: get("served.search"),
            test_runs: get("served.test"),
            anchors_followed: get("anchors.followed"),
            repairs: get("repair.count"),
            repair_tokens: get("repair.tokens"),
            repair_seconds: get("repair.seconds"),
            divergences: counters
                .iter()
                .filter(|(k, _)| k.starts_with("divergence."))
                .map(|(k, v)| (k["divergence.".len()..].to_string(), *v))
                .collect(),
            counters,
            upgrade,
        })
    }

    /// Order the repair list by relevance to current work (FR-239): items in files changed in
    /// the working tree first, then the rest, each group in its original order.
    fn prioritize(&self, items: &mut [RepairItem]) {
        let Ok(st) = self
            .repo
            .git(&["status", "--porcelain", "--untracked-files=all"])
        else {
            return;
        };
        let changed: BTreeSet<String> = st
            .lines()
            .filter_map(|l| l.get(3..))
            .map(|p| {
                p.rsplit(" -> ")
                    .next()
                    .unwrap_or(p)
                    .trim_matches('"')
                    .to_string()
            })
            .collect();
        if changed.is_empty() {
            return;
        }
        items.sort_by_key(|i| {
            let path = i
                .location
                .as_ref()
                .map(|l| l.path.clone())
                .unwrap_or_default();
            !changed.contains(&path)
        });
    }

    pub fn intent_sources(&self) -> &[String] {
        &self.config.intent.sources
    }

    /// The working directory relative paths are resolved against.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
}
