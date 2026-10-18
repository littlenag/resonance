//! Command dispatch. Operations are served by the repository's daemon, started on demand
//! (FR-252), or run in this process with `--no-daemon` (direct mode, MVP-002). Either way the
//! graph is brought up to date with the disk before the answer.

mod intent;
mod links;
mod recover;
pub use recover::divergence_line as recover_line;
mod read;
pub mod services;
mod setup;
mod units;
mod verify;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use resonance_core::api::Engine;
use resonance_core::daemon::client::Remote;
use resonance_core::error::{Error, Result};
use resonance_core::ops::Actor;
use resonance_core::repo::Repo;
use resonance_core::service::{Backend, Call, Direct};
use resonance_core::store::GraphStore;
use resonance_store_sqlite::SqliteStore;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::cli::{Cli, Command};

/// What every command needs.
pub struct Ctx {
    pub json: bool,
    /// Where relative paths are resolved from.
    pub cwd: PathBuf,
    pub no_daemon: bool,
    pub cached: bool,
}

impl Ctx {
    /// Print a view as JSON, or as text.
    pub fn emit<T: Serialize>(&self, view: &T, text: impl FnOnce(&T) -> String) -> Result<()> {
        let mut stdout = std::io::stdout().lock();
        let rendered = if self.json {
            let mut s = serde_json::to_string_pretty(view)?;
            s.push('\n');
            s
        } else {
            text(view)
        };
        stdout
            .write_all(rendered.as_bytes())
            .map_err(|e| Error::io("writing output", e))
    }

    /// A note for people, on standard error so that it never mixes with JSON output.
    pub fn note(&self, text: &str) {
        if !text.is_empty() {
            eprintln!("{}", text.trim_end());
        }
    }

    pub fn repo(&self) -> Result<Repo> {
        Repo::discover(&self.cwd)
    }

    /// The backend for an initialized repository: the daemon, or this process.
    pub fn backend(&self) -> Result<Box<dyn Backend>> {
        let repo = self.repo()?;
        if let Some(note) = resonance_core::lifecycle::ensure_layout(&repo, &open_store)? {
            self.note(&note);
        }
        if !repo.is_initialized() {
            let home = home_dir();
            if !repo.resonance_dir().exists()
                && resonance_core::lifecycle::latest_backup(&repo, &home).is_some()
            {
                return Err(Error::Invalid(
                    ".resonance/ is missing, but a backup exists; restore it with `resonance backup --restore` (FR-184)".into(),
                ));
            }
            return Err(Error::NotInitialized);
        }
        let actor = human_actor(&repo);
        if self.no_daemon {
            let store = open_store(&repo.store_path())?;
            let engine = Engine::open(repo, store, actor, self.cwd.clone())?;
            return Ok(Box::new(DirectCached {
                inner: Direct { engine },
                cached: self.cached,
            }));
        }
        let mut remote = Remote::connect(&repo, &exe()?, actor, self.cwd.clone())?;
        remote.cached = self.cached;
        Ok(Box::new(remote))
    }

    /// Run one operation and decode its view.
    pub fn call<T: DeserializeOwned>(
        &self,
        backend: &mut dyn Backend,
        method: &str,
        args: Value,
    ) -> Result<T> {
        let v = backend.call(Call::new(method, args))?;
        serde_json::from_value(v)
            .map_err(|e| Error::Daemon(format!("unexpected answer to {method}: {e}")))
    }

    /// Open a backend, run one operation and decode its view.
    pub fn run<T: DeserializeOwned>(&self, method: &str, args: Value) -> Result<T> {
        let mut b = self.backend()?;
        self.call(&mut *b, method, args)
    }
}

/// Direct mode, honoring `--cached` by skipping the reconcile.
struct DirectCached {
    inner: Direct,
    cached: bool,
}

impl Backend for DirectCached {
    fn call(&mut self, mut call: Call) -> Result<Value> {
        if call.cwd.is_none() {
            call.cwd = Some(self.inner.engine.cwd().to_path_buf());
        }
        if self.cached || call.cached {
            return resonance_core::service::dispatch(&mut self.inner.engine, &call);
        }
        self.inner.call(call)
    }
}

/// Open the SQLite store; the binary is where the engine is chosen (FR-161).
pub fn open_store(path: &Path) -> Result<Box<dyn GraphStore>> {
    Ok(Box::new(SqliteStore::open(path)?))
}

/// The user's home directory.
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// This executable, for starting the daemon.
pub fn exe() -> Result<PathBuf> {
    let p =
        std::env::current_exe().map_err(|e| Error::io("finding the resonance executable", e))?;
    Ok(p.canonicalize().unwrap_or(p))
}

/// The person running the CLI, from git's configuration.
pub fn human_actor(repo: &Repo) -> Actor {
    let id = repo
        .git(&["config", "user.email"])
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "unknown".into());
    Actor::human(id)
}

/// Ask a yes/no question on the terminal. Returns `None` when there is no terminal.
pub fn confirm(question: &str) -> Option<bool> {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return None;
    }
    eprint!("{question} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).ok()?;
    Some(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Run a parsed command line.
pub fn run(cli: Cli) -> Result<()> {
    let cwd = match &cli.dir {
        Some(d) => {
            let d = d
                .canonicalize()
                .map_err(|e| Error::io(format!("-C {}", d.display()), e))?;
            if !d.is_dir() {
                return Err(Error::Usage(format!("-C {}: not a directory", d.display())));
            }
            d
        }
        None => {
            std::env::current_dir().map_err(|e| Error::io("finding the current directory", e))?
        }
    };
    let ctx = Ctx {
        json: cli.json,
        cwd,
        no_daemon: cli.no_daemon,
        cached: cli.cached,
    };
    match cli.command {
        Command::Init { agent, no_agent } => setup::init(&ctx, agent, no_agent),
        Command::Status => setup::status(&ctx),
        Command::Reconcile { full } => setup::reconcile(&ctx, full),
        Command::RebuildDerived => setup::rebuild_derived(&ctx),
        Command::Rebuild => setup::rebuild(&ctx),
        Command::Restore { file } => setup::restore(&ctx, &file),
        Command::Recover { action } => recover::recover(&ctx, action),
        Command::Value => recover::value(&ctx),
        Command::Backup {
            restore,
            from,
            list,
        } => setup::backup(&ctx, restore, from, list),
        Command::Mcp => services::mcp(&ctx),
        Command::Daemon { foreground, stop } => services::daemon(&ctx, foreground, stop),
        Command::Registry(cmd) => services::registry(&ctx, cmd),
        Command::Doctor => services::doctor(&ctx),
        Command::Agent(cmd) => setup::agent(&ctx, cmd),
        Command::Export { out } => setup::export(&ctx, out),
        Command::Eject { dry_run, yes, out } => setup::eject(&ctx, dry_run, yes, out),
        Command::Version => setup::version(&ctx),
        Command::Import { paths, format } => intent::import(&ctx, &paths, format.as_deref()),
        Command::Req(cmd) => intent::req(&ctx, cmd),
        Command::Decision(cmd) => intent::decision(&ctx, cmd),
        Command::Design(cmd) => intent::item(&ctx, "Design", cmd),
        Command::Constraint(cmd) => intent::item(&ctx, "Constraint", cmd),
        Command::Spec(cmd) => intent::spec(&ctx, cmd),
        Command::Ext(cmd) => intent::ext(&ctx, cmd),
        Command::Intent(cmd) => intent::project(&ctx, cmd),
        Command::Unit(cmd) => units::unit(&ctx, cmd),
        Command::Test(args) => match (args.sub, args.reference) {
            (Some(sub), _) => units::test_add(&ctx, sub),
            (None, Some(reference)) => verify::test(&ctx, &reference),
            (None, None) => Err(Error::Usage(
                "give what to test (a requirement, test, unit or link), or `resonance test add` to declare a test".into(),
            )),
        },
        Command::Link {
            src,
            etype,
            dst,
            confidence,
        } => links::link(&ctx, &src, &etype, &dst, confidence),
        Command::Unlink { edge } => links::unlink(&ctx, &edge),
        Command::Confirm { edge } => links::confirm(&ctx, &edge),
        Command::Annotate {
            target,
            kind,
            text,
            task,
        } => links::annotate(&ctx, &target, &kind, &text, task.as_deref()),
        Command::Reattach { reference, target } => links::reattach(&ctx, &reference, &target),
        Command::Show { reference } => read::show(&ctx, &reference),
        Command::Trace { requirement } => read::trace(&ctx, &requirement),
        Command::Why { unit } => read::why(&ctx, &unit),
        Command::Search {
            text,
            ntype,
            status,
            limit,
        } => read::search(&ctx, &text, ntype.as_deref(), status.as_deref(), limit),
        Command::Unverified => read::unverified(&ctx),
        Command::Context { requirements, task } => read::context(&ctx, &requirements, task.as_deref()),
    }
}
