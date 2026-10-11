//! Command dispatch. Every command except `init`, `version` and `mcp` reconciles first
//! (MVP-002), so answers always reflect the files on disk.

mod intent;
mod links;
mod read;
mod services;
mod setup;
mod units;
mod verify;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use resonance_core::api::Engine;
use resonance_core::error::{Error, Result};
use resonance_core::ingest::{ReconcileOptions, ReconcileReport};
use resonance_core::ops::Actor;
use resonance_core::repo::Repo;
use resonance_core::store::GraphStore;
use resonance_store_sqlite::SqliteStore;
use serde::Serialize;

use crate::cli::{Cli, Command};

/// What every command needs.
pub struct Ctx {
    pub json: bool,
    /// Where relative paths are resolved from.
    pub cwd: PathBuf,
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
}

/// Open the SQLite store; the binary is where the engine is chosen (FR-161).
pub fn open_store(path: &Path) -> Result<Box<dyn GraphStore>> {
    Ok(Box::new(SqliteStore::open(path)?))
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

/// Open the engine for an initialized repository.
pub fn engine(ctx: &Ctx) -> Result<Engine> {
    let repo = Repo::discover(&ctx.cwd)?;
    if !repo.is_initialized() {
        return Err(Error::NotInitialized);
    }
    let store = open_store(&repo.store_path())?;
    let actor = human_actor(&repo);
    Engine::open(repo, store, actor, ctx.cwd.clone())
}

/// Open the engine and reconcile (MVP-002).
pub fn reconciled(ctx: &Ctx) -> Result<(Engine, ReconcileReport)> {
    let mut engine = engine(ctx)?;
    let report = engine.reconcile(ReconcileOptions::default())?;
    Ok((engine, report))
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
    };
    match cli.command {
        Command::Init { agent, no_agent } => setup::init(&ctx, agent, no_agent),
        Command::Status => setup::status(&ctx),
        Command::Reconcile { full } => setup::reconcile(&ctx, full),
        Command::RebuildDerived => setup::rebuild_derived(&ctx),
        Command::Mcp => services::mcp(&ctx),
        Command::Agent(cmd) => setup::agent(&ctx, cmd),
        Command::Export { out } => setup::export(&ctx, out),
        Command::Eject { dry_run, yes, out } => setup::eject(&ctx, dry_run, yes, out),
        Command::Version => setup::version(&ctx),
        Command::Import { paths } => intent::import(&ctx, &paths),
        Command::Req(cmd) => intent::req(&ctx, cmd),
        Command::Decision(cmd) => intent::decision(&ctx, cmd),
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
