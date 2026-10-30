//! The command line (§11.3, FR-259): setup, the daemon, status, checks and repair. Graph work
//! (bringing in intent, declaring code units and tests, linking, annotating, tracing, the
//! change log) is not here: agents do it through MCP, and people in the web UI and the spec TUI.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "resonance",
    version,
    about = "Traceability between intent and implementation, for your coding agent.",
    long_about = "Resonance keeps a graph of this repository's requirements, decisions, code units, tests, links and annotations, beside your git repository. Your agent works with it through the MCP server (`resonance mcp`, set up by `resonance init`); you browse and edit the spec with `resonance tui`. These commands set Resonance up, run its daemon, report its status and repair it.",
    propagate_version = true
)]
pub struct Cli {
    /// Run as if resonance was started in PATH.
    #[arg(short = 'C', global = true, value_name = "PATH")]
    pub dir: Option<PathBuf>,

    /// Print machine-readable JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,

    /// Run in this process instead of through the daemon (for debugging): reconcile, then answer.
    #[arg(long, global = true, env = "RESONANCE_NO_DAEMON", value_parser = clap::builder::FalseyValueParser::new())]
    pub no_daemon: bool,

    /// Answer from the graph as it is, without waiting for changes still being applied.
    #[arg(long, global = true)]
    pub cached: bool,

    #[command(subcommand)]
    pub command: Command,
}

/// Which agent harness `init` sets up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AgentChoice {
    /// Claude Code: the MCP server, a skill, harness hooks and a CLAUDE.local.md section.
    ClaudeCode,
    /// Any other agent: print an AGENTS.md section and the MCP command line.
    Generic,
    /// No agent kit.
    None,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    // Setup and lifecycle
    /// Set up Resonance here: mirror the repository into the graph, and install the git hooks and the agent kit. In a shared repository, join it. Running it again refreshes the hooks and the kit.
    Init {
        /// The agent to onboard (default claude-code).
        #[arg(long, value_enum, conflicts_with = "no_agent")]
        agent: Option<AgentChoice>,
        /// Install no agent kit (the same as --agent none).
        #[arg(long)]
        no_agent: bool,
        /// In a shared repository: try Resonance privately instead of joining (FR-187).
        #[arg(long)]
        offline: bool,
    },
    /// Stage, participation, divergences, the repair list, pending changes and the agent kit.
    Status,
    /// Health of this repository and of every known daemon.
    Doctor,
    /// Browse and edit spec items in a terminal UI; edits land in the change log for your agent.
    Tui,
    /// Open the web UI in your browser, starting the daemon if needed: spec items, the change log, diagnostics and logs.
    Ui {
        /// A spec item to open, by key (for example FR-001).
        key: Option<String>,
        /// Print the address instead of opening a browser.
        #[arg(long)]
        no_open: bool,
    },
    /// Share the graph with the team (stage 1 to stage 2): preview, then new files to commit.
    Share {
        /// Annotations to keep local (private), by ID (repeatable).
        #[arg(long = "keep-local", value_name = "ANNOTATION")]
        keep_local: Vec<String>,
        /// Show what would be committed and change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Stop sharing the graph (stage 2 to stage 1): export, then stage the removal of .resonance/.
    Unshare {
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
        /// Where to write the export (default: ~/.resonance/exports/<repo>-<time>.jsonl).
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Stop being a Resonance user here, without loss (removes hooks, the agent kit and opting in).
    Leave {
        /// Write the open delta's uncommitted changes here first.
        #[arg(long, value_name = "FILE")]
        export: Option<PathBuf>,
    },
    /// Export the graph, then remove .resonance/, the hooks, the agent kit and the exclusion.
    Eject {
        /// Show what would happen and change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
        /// Where to write the export (default: ~/.resonance/exports/<repo>-<time>.jsonl).
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Version of the binary, store schema and MCP protocol.
    Version,

    // The daemon
    /// The repository's daemon: its health, or start, stop and restart it.
    Daemon {
        #[command(subcommand)]
        action: Option<DaemonCommand>,
    },

    // Checks and repair
    /// Consistency checks for commits and pull requests (FR-215, FR-216).
    Check {
        /// Check what is staged (what `pre-commit` runs).
        #[arg(long, conflicts_with = "range")]
        staged: bool,
        /// Check a range of commits, such as main..HEAD (what `pre-push` and CI run).
        #[arg(long)]
        range: Option<String>,
    },
    /// Divergences between git, the files and the graph, and how to recover (§8.18).
    Recover {
        #[command(subcommand)]
        action: Option<RecoverCommand>,
    },
    /// Bring the graph up to date with the files on disk now.
    Reconcile {
        /// Re-read every file, ignoring size and modification times.
        #[arg(long)]
        full: bool,
    },
    /// Re-fold the graph from its snapshot and deltas (FR-150), then bring it up to date.
    Rebuild,
    /// Drop derived data (containment, file cache) and rebuild it.
    RebuildDerived,
    /// Write all authored data as edit-algebra operations (JSON Lines).
    Export {
        /// Write to this file instead of standard output.
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Restore authored data from an export written by `resonance export` or `eject` (FR-166).
    Restore {
        /// The export file (JSON Lines).
        file: PathBuf,
    },
    /// Back up .resonance/ outside the repository, or restore a missing .resonance/ (FR-184).
    Backup {
        /// Restore .resonance/ from the newest backup (or from --from) when it is missing.
        #[arg(long)]
        restore: bool,
        /// The backup directory to restore from.
        #[arg(long, value_name = "DIR", requires = "restore")]
        from: Option<PathBuf>,
        /// List this repository's backups.
        #[arg(long, conflicts_with = "restore")]
        list: bool,
    },

    // Services and plumbing
    /// Run the MCP server over stdio (started by the agent harness).
    Mcp,
    /// The system-wide registry: discovery and health of daemons.
    #[command(subcommand)]
    Registry(RegistryCommand),
    /// Entry point for the git hooks Resonance installs (plumbing).
    #[command(hide = true)]
    Hook {
        name: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Entry point for the agent harness hooks the agent kit installs (plumbing, FR-222).
    #[command(hide = true)]
    AgentHook {
        /// The event: `pre-git`.
        event: String,
    },
    /// Git drivers (plumbing).
    #[command(hide = true, subcommand)]
    GitDriver(GitDriverCommand),
}

#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// The daemon's health (the default); starts it if needed.
    Status,
    /// Start the daemon if it is not running.
    Start,
    /// Stop the running daemon.
    Stop,
    /// Stop the daemon, then start a new one.
    Restart,
    /// Run the daemon in this process until idle or stopped (what clients start).
    #[command(hide = true)]
    Run,
}

#[derive(Debug, Subcommand)]
pub enum GitDriverCommand {
    /// The merge driver for .resonance/graph.db: union of both snapshots (FR-157).
    Merge {
        base: PathBuf,
        ours: PathBuf,
        theirs: PathBuf,
    },
    /// The textconv driver for *.delta: a delta file for people (FR-066).
    Textconv { file: PathBuf },
}

#[derive(Debug, Subcommand)]
pub enum RecoverCommand {
    /// Repair a conflicted .resonance/graph.db mechanically, without the merge driver (C6).
    RepairSnapshot,
    /// Put back what an automatic resolution displaced.
    Undo {
        /// A recovery record ID (a unique prefix is enough).
        id: String,
    },
    /// Mark a divergence as handled.
    Dismiss {
        /// A divergence ID (a unique prefix is enough).
        id: String,
        /// What was done.
        #[arg(long)]
        note: Option<String>,
    },
    /// Attach a detached offline delta to the current commit.
    Attach {
        /// A delta ID from the detached pool.
        delta: String,
    },
    /// Export a detached offline delta outside the repository, then remove it.
    Drop {
        /// A delta ID from the detached pool.
        delta: String,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Write the graph's stored version of a divergence's file.
    TakeGraph {
        /// A divergence ID (a unique prefix is enough).
        id: String,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Keep the file's version, or the displaced disk version, and ingest it.
    TakeDisk {
        /// A divergence ID (a unique prefix is enough).
        id: String,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Leave the graph's version in place and write the other bytes beside the file.
    KeepBoth {
        /// A divergence ID (a unique prefix is enough).
        id: String,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Merge the file three ways: the current bytes, HEAD, and the displaced version.
    Merge {
        /// A divergence ID (a unique prefix is enough).
        id: String,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Apply the inverse of a reverted delta that the snapshot already folded (FR-159).
    Inverse {
        /// The delta ID from the recover list.
        delta: String,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum RegistryCommand {
    /// Run the registry in this process.
    Run,
    /// Install the registry as a user service (launchd or systemd --user) and start it.
    Install {
        /// Show the service definition and commands, and change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Stop and remove the registry's user service.
    Uninstall {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
}
