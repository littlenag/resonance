//! The command line (§11.3, mvp.md §3.9).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "resonance",
    version,
    about = "Traceability between intent and implementation, kept in a graph beside your git repository.",
    long_about = "Resonance keeps a private graph of this repository's requirements, decisions, declared code units and tests, the links between them, and annotations. It parses no source code: code units are named regions of files that people and agents declare. Every command first brings the graph up to date with the files on disk.",
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

#[derive(Debug, Subcommand)]
pub enum Command {
    // Setup and lifecycle
    /// Set up Resonance in this repository (offline mode): creates .resonance/, excludes it locally, reads every file.
    Init {
        /// Also register the MCP server with Claude Code, without asking.
        #[arg(long, conflicts_with = "no_agent")]
        agent: bool,
        /// Do not offer to register the MCP server.
        #[arg(long)]
        no_agent: bool,
    },
    /// Counts and the repair list.
    Status,
    /// Bring the graph up to date with the files on disk now.
    Reconcile {
        /// Re-read every file, ignoring size and modification times.
        #[arg(long)]
        full: bool,
    },
    /// Drop derived data (containment, file cache) and rebuild it.
    RebuildDerived,
    /// Run the MCP server over stdio (for agents).
    Mcp,
    /// Agent integration.
    #[command(subcommand)]
    Agent(AgentCommand),
    /// Write all authored data as edit-algebra operations (JSON Lines).
    Export {
        /// Write to this file instead of standard output.
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Export the graph, then remove .resonance/ and its exclusion.
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
    /// The per-repository daemon. Started on demand by every command; rarely needed by hand.
    Daemon {
        /// Run in this process until idle or stopped (this is what clients start).
        #[arg(long, conflicts_with = "stop")]
        foreground: bool,
        /// Stop the running daemon.
        #[arg(long)]
        stop: bool,
    },
    /// The system-wide registry: discovery and health of daemons.
    #[command(subcommand)]
    Registry(RegistryCommand),
    /// Health of this repository and of every known daemon.
    Doctor,

    // Intent
    /// Import requirements from Markdown files and keep them in sync.
    Import {
        /// Markdown files with `- **FR-001** text` items or `| **FR-001** | text |` rows.
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// Requirements.
    #[command(subcommand)]
    Req(ReqCommand),
    /// Decisions.
    #[command(subcommand)]
    Decision(DecisionCommand),

    // Code units and tests
    /// Code units: named regions of files.
    #[command(subcommand)]
    Unit(UnitCommand),
    /// Run the tests linked to a requirement, unit or link (`test <ref>`), or declare one (`test add`).
    Test(TestArgs),

    // Links and annotations
    /// Link two nodes: satisfies, verifies or constrains.
    Link {
        /// The source: a unit, test, file or decision.
        src: String,
        /// satisfies, verifies or constrains.
        #[arg(value_name = "TYPE")]
        etype: String,
        /// The target: a requirement, unit or other node.
        dst: String,
        /// How sure you are (0 to 1).
        #[arg(long)]
        confidence: Option<f64>,
    },
    /// Remove a link (it is kept for history).
    Unlink {
        /// A link ID (or a unique prefix of one).
        edge: String,
    },
    /// Return a stale link to asserted after checking that it still holds.
    Confirm {
        /// A link ID (or a unique prefix of one).
        edge: String,
    },
    /// Annotate a code unit, file or other node.
    Annotate {
        /// What to annotate.
        target: String,
        /// note, warning, call-out or instruction.
        #[arg(long)]
        kind: String,
        /// The annotation.
        #[arg(long)]
        text: String,
        /// For call-outs: the task this code matters for.
        #[arg(long)]
        task: Option<String>,
    },
    /// Re-attach an orphaned link or annotation to a new target.
    Reattach {
        /// A link ID or an annotation ID.
        reference: String,
        /// The new target.
        target: String,
    },

    // Reading
    /// One node or link in full.
    Show { reference: String },
    /// From a requirement: the code that satisfies it and the tests that verify it.
    Trace { requirement: String },
    /// Why a code unit or file exists: requirements, decisions, tests, annotations.
    Why { unit: String },
    /// Search by text, type and status.
    Search {
        /// Text to look for (case-insensitive).
        #[arg(default_value = "")]
        text: String,
        /// Requirement, Decision, CodeUnit, Test, Annotation, File or Evidence.
        #[arg(long = "type", value_name = "TYPE")]
        ntype: Option<String>,
        /// A link status (asserted, verified, stale, orphaned) or a node status (active, retired, attached, removed).
        #[arg(long)]
        status: Option<String>,
        /// Maximum results.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Requirements with no verified test.
    Unverified,
    /// A context pack for a task: requirements, decisions, code locations, tests, annotations.
    Context {
        #[arg(required = true)]
        requirements: Vec<String>,
        /// Include call-outs made for this task label.
        #[arg(long)]
        task: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Register the MCP server with Claude Code at local scope (writes nothing to tracked files).
    Install {
        /// Show the command and change nothing.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ReqCommand {
    /// Create a requirement.
    Add {
        /// A key such as FR-300.
        key: String,
        /// The requirement text.
        #[arg(long)]
        text: String,
        /// functional or non-functional (default from the key).
        #[arg(long)]
        kind: Option<String>,
    },
    /// Change a requirement's text (its links become stale).
    Edit {
        requirement: String,
        #[arg(long)]
        text: String,
    },
    /// Retire a requirement (it is never deleted).
    Retire { requirement: String },
}

#[derive(Debug, Subcommand)]
pub enum DecisionCommand {
    /// Record a decision.
    Add {
        #[arg(long)]
        title: String,
        /// What was decided.
        #[arg(long)]
        decision: String,
        #[arg(long)]
        context: Option<String>,
        #[arg(long)]
        consequences: Option<String>,
    },
    /// Change a decision.
    Edit {
        decision_ref: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        decision: Option<String>,
        #[arg(long)]
        context: Option<String>,
        #[arg(long)]
        consequences: Option<String>,
        /// For example accepted, superseded or rejected.
        #[arg(long)]
        status: Option<String>,
    },
}

/// A region of a file, as lines or as an exact snippet.
#[derive(Debug, Args)]
pub struct RegionArgs {
    /// `path:START-END` (lines, 1-based, inclusive), `path:LINE`, or `path` with --match.
    pub region: String,
    /// An exact snippet that occurs exactly once in the file.
    #[arg(long = "match", value_name = "TEXT")]
    pub match_text: Option<String>,
    /// The name to give it.
    #[arg(long)]
    pub name: String,
    /// Free text: function, type, block, test, ...
    #[arg(long)]
    pub kind: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum UnitCommand {
    /// Declare a code unit.
    Add(RegionArgs),
    /// Re-anchor, rename or confirm a code unit or test. With no options, confirms its current anchor.
    Edit {
        unit: String,
        /// The new region: `path:START-END`, `path:LINE`, or `path` with --match.
        region: Option<String>,
        #[arg(long = "match", value_name = "TEXT")]
        match_text: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        /// A new run command (tests only).
        #[arg(long)]
        run: Option<String>,
    },
    /// Remove a code unit or test (it is retired; its links become orphaned).
    Remove { unit: String },
    /// List code units and tests, optionally under a file or directory.
    List { path: Option<String> },
    /// Declare that a unit became several (declare the new units first).
    Split {
        unit: String,
        /// The units it became.
        #[arg(long, required = true, num_args = 1..)]
        into: Vec<String>,
    },
    /// Declare that several units became one.
    Merge {
        #[arg(required = true)]
        units: Vec<String>,
        /// The unit they became.
        #[arg(long)]
        into: String,
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

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct TestArgs {
    #[command(subcommand)]
    pub sub: Option<TestCommand>,
    /// A requirement, code unit, test or verifies link whose tests to run.
    pub reference: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum TestCommand {
    /// Declare a test: a region plus the exact command that runs it alone.
    Add {
        #[command(flatten)]
        region: RegionArgs,
        /// The exact command that runs this test alone (exit code 0 is a pass).
        #[arg(long)]
        run: Option<String>,
    },
}
