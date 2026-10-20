//! Services: `mcp` (MVP-003), `daemon` (§8.7), `registry` (FR-088–FR-094) and `doctor` (FR-096).

use std::path::{Path, PathBuf};

use resonance_core::api::VERSION;
use resonance_core::daemon::client::{self, Remote};
use resonance_core::daemon::runtime::{self, Health};
use resonance_core::daemon::{self, DaemonOptions};
use resonance_core::error::{Error, Result};
use resonance_core::mcp;
use resonance_core::registry;
use resonance_core::repo::{Repo, exclude};
use resonance_core::store::SCHEMA_VERSION;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Ctx, confirm, exe, human_actor, open_store};
use crate::cli::{DaemonCommand, RegistryCommand};
use crate::render;

pub fn mcp(ctx: &Ctx) -> Result<()> {
    // Paths given by agents are relative to the repository root.
    let root = ctx.repo()?.root;
    let inner = Ctx {
        json: ctx.json,
        cwd: root,
        no_daemon: ctx.no_daemon,
        cached: ctx.cached,
    };
    let backend = inner.backend()?;
    let stdin = std::io::stdin().lock();
    let stdout = std::io::stdout().lock();
    mcp::serve(backend, stdin, stdout)
}

pub fn daemon(ctx: &Ctx, action: DaemonCommand) -> Result<()> {
    let repo = ctx.repo()?;
    let stop = |ctx: &Ctx| -> Result<bool> {
        let was_running = client::stop(&repo)?;
        if !ctx.json {
            ctx.note(if was_running {
                "Stopped the daemon."
            } else {
                "No daemon is running for this repository."
            });
        }
        Ok(was_running)
    };
    match action {
        DaemonCommand::Stop => {
            let was_running = client::stop(&repo)?;
            ctx.emit(&json!({"stopped": was_running}), |_| {
                if was_running {
                    "Stopped the daemon.\n".to_string()
                } else {
                    "No daemon is running for this repository.\n".to_string()
                }
            })
        }
        DaemonCommand::Run => {
            let actor = human_actor(&repo);
            let opts = DaemonOptions {
                binary_id: runtime::binary_id(&exe()?),
                idle: None,
            };
            daemon::run(repo, &open_store, actor, opts)
        }
        DaemonCommand::Restart | DaemonCommand::Start | DaemonCommand::Status => {
            if matches!(action, DaemonCommand::Restart) {
                stop(ctx)?;
            }
            if !repo.is_initialized() {
                return Err(Error::NotInitialized);
            }
            // Start one if needed, and report on it.
            let mut remote = Remote::connect(&repo, &exe()?, human_actor(&repo), ctx.cwd.clone())?;
            let health = remote.health()?;
            ctx.emit(&health, render::health)
        }
    }
}

pub fn registry(ctx: &Ctx, cmd: RegistryCommand) -> Result<()> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Invalid("HOME is not set".into()))?;
    match cmd {
        RegistryCommand::Run => registry::run(&runtime::runtime_dir()),
        RegistryCommand::Install { dry_run, yes } => {
            let plan = registry::service_plan(&exe()?, &home)?;
            let preview = render::service_plan("install", &plan);
            if dry_run {
                return ctx.emit(&plan, |_| preview);
            }
            let go = yes || confirm(&format!("{preview}Proceed?")).unwrap_or(false);
            if !go {
                ctx.note(&format!(
                    "{preview}Nothing changed. Run `resonance registry install --yes` to proceed."
                ));
                return Err(Error::Invalid("not confirmed".into()));
            }
            let out = registry::install(&plan)?;
            ctx.emit(&json!({"installed": plan.path, "output": out}), |_| {
                format!(
                    "Installed and started the registry service ({}).\n",
                    plan.path.display()
                )
            })
        }
        RegistryCommand::Uninstall { dry_run, yes } => {
            let plan = registry::service_plan(&exe()?, &home)?;
            let preview = render::service_plan("uninstall", &plan);
            if dry_run {
                return ctx.emit(&plan, |_| preview);
            }
            let go = yes || confirm(&format!("{preview}Proceed?")).unwrap_or(false);
            if !go {
                ctx.note(&format!(
                    "{preview}Nothing changed. Run `resonance registry uninstall --yes` to proceed."
                ));
                return Err(Error::Invalid("not confirmed".into()));
            }
            let out = registry::uninstall(&plan)?;
            ctx.emit(&json!({"removed": plan.path, "output": out}), |_| {
                "Stopped and removed the registry service.\n".to_string()
            })
        }
    }
}

/// One daemon, as `doctor` sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonCheck {
    pub root: String,
    pub pid: u32,
    pub endpoint: String,
    /// `registry` or `daemon.json`.
    pub found_through: String,
    pub health: Option<Health>,
    /// Running a different version or build of resonance than this one.
    pub version_skew: bool,
}

/// This repository, as `doctor` sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoCheck {
    pub root: String,
    pub initialized: bool,
    pub excluded: bool,
    pub schema_version: Option<u32>,
    pub integrity_problems: Vec<String>,
    pub daemon_running: bool,
    /// The agent kit (FR-210, FR-214).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_kit: Option<resonance_core::agent::KitStatus>,
    /// The repository's stage (1 offline, 2 shared).
    #[serde(default)]
    pub stage: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub version: String,
    pub runtime_dir: String,
    pub registry_running: bool,
    pub repository: Option<RepoCheck>,
    pub daemons: Vec<DaemonCheck>,
    pub problems: Vec<String>,
}

fn check_repo(repo: &Repo, problems: &mut Vec<String>) -> RepoCheck {
    let initialized = repo.is_initialized();
    let mut check = RepoCheck {
        root: repo.root.display().to_string(),
        initialized,
        excluded: exclude::is_present(repo),
        schema_version: None,
        integrity_problems: Vec::new(),
        daemon_running: runtime::lock_held(&repo.root),
        agent_kit: None,
        stage: 0,
    };
    if !initialized {
        return check;
    }
    let config =
        resonance_core::repo::config::Config::load(&repo.config_path()).unwrap_or_default();
    let mode = resonance_core::history::mode(repo, &config);
    check.stage = mode.stage;
    check.agent_kit = Some(resonance_core::agent::kit_status(repo, mode));
    if !check.excluded && !mode.shared() {
        problems.push("/.resonance/ is missing from .git/info/exclude; git will see .resonance/ (run `resonance init` to restore it)".into());
    }
    if let Ok(text) = std::fs::read_to_string(repo.config_path()) {
        problems.extend(resonance_core::repo::config::legacy_config_notes(&text));
    }
    match open_store(&repo.store_path()) {
        Ok(store) => {
            if let Ok(keys) = resonance_core::api::legacy_unlinked_sources(&*store)
                && !keys.is_empty()
            {
                problems.push(format!(
                    "{} spec item{} still {} a legacy `source` property and no document link (for example {}). Re-import with `resonance_import_items`.",
                    keys.len(),
                    if keys.len() == 1 { "" } else { "s" },
                    if keys.len() == 1 { "has" } else { "have" },
                    keys[0]
                ));
            }
            check.schema_version = store.schema_version().ok();
            if check.schema_version.is_some_and(|v| v > SCHEMA_VERSION) {
                problems.push(format!(
                    "the graph uses schema version {}, newer than this binary understands ({SCHEMA_VERSION})",
                    check.schema_version.unwrap_or_default()
                ));
            }
            match store.integrity_check() {
                Ok(p) => check.integrity_problems = p,
                Err(e) => check.integrity_problems = vec![e.to_string()],
            }
            for p in &check.integrity_problems {
                problems.push(format!("integrity: {p}"));
            }
        }
        Err(e) => problems.push(format!("the graph could not be opened: {e}")),
    }
    check
}

pub fn doctor(ctx: &Ctx) -> Result<()> {
    let dir = runtime::runtime_dir();
    let ours = runtime::binary_id(&exe()?);
    let mut problems = Vec::new();
    let repository = ctx.repo().ok().map(|r| check_repo(&r, &mut problems));

    // Daemons: through the registry if it runs, else through the known repositories'
    // endpoint files (discovery without the registry, S14).
    let (registry_running, mut candidates) = match registry::list(&dir) {
        Ok(entries) => (
            true,
            entries
                .into_iter()
                .map(|e| (e.info, "registry".to_string(), e.health))
                .collect::<Vec<_>>(),
        ),
        Err(_) => (false, Vec::new()),
    };
    let mut roots: Vec<PathBuf> = runtime::known_roots();
    if let Some(r) = &repository {
        roots.push(PathBuf::from(&r.root));
    }
    for root in roots {
        if candidates
            .iter()
            .any(|(i, _, _)| Path::new(&i.root) == root)
        {
            continue;
        }
        if !runtime::lock_held(&root) {
            continue;
        }
        if let Some(info) = runtime::read_info(&root) {
            candidates.push((info, "daemon.json".into(), None));
        }
    }
    let mut daemons = Vec::new();
    for (info, found_through, health) in candidates {
        let health = registry::daemon_health(Path::new(&info.endpoint)).or(health);
        let version_skew = info.version != VERSION || info.binary_id != ours;
        match &health {
            None => problems.push(format!(
                "the daemon for {} (pid {}) does not answer",
                info.root, info.pid
            )),
            Some(h) if !h.ready => {
                problems.push(format!("the daemon for {} is {}", info.root, h.state))
            }
            Some(h) => {
                if let Some(e) = &h.last_error {
                    problems.push(format!("the daemon for {} reported: {e}", info.root));
                }
            }
        }
        if version_skew {
            problems.push(format!(
                "the daemon for {} runs a different build ({}); the next command there replaces it",
                info.root, info.binary_id
            ));
        }
        daemons.push(DaemonCheck {
            root: info.root,
            pid: info.pid,
            endpoint: info.endpoint,
            found_through,
            health,
            version_skew,
        });
    }
    daemons.sort_by(|a, b| a.root.cmp(&b.root));
    let report = DoctorReport {
        version: VERSION.into(),
        runtime_dir: dir.display().to_string(),
        registry_running,
        repository,
        daemons,
        problems,
    };
    ctx.emit(&report, render::doctor)
}
