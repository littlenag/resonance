//! Setup and lifecycle: init (with the git hooks and the agent kit), status, reconcile,
//! rebuild, export, restore, backup, eject, version, and the agent harness hook.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use resonance_core::agent::{self, AgentInstall, KitOptions, KitReport};
use resonance_core::api::{GateDecision, StatusView, version_info};
use resonance_core::daemon::client;
use resonance_core::error::{Error, Result};
use resonance_core::hooks::HookChange;
use resonance_core::ingest::ReconcileReport;
use resonance_core::lifecycle::{self, OFFLINE_EXPLANATION};
use resonance_core::repo::Repo;
use serde_json::{Value, json};

use super::{Ctx, confirm, exe, open_store};
use crate::cli::AgentChoice;
use crate::render;

/// `resonance init` (S1, FR-180, FR-210): mirror the repository into the graph, install the git
/// hooks for this working tree's participation, and install the agent kit. In a repository
/// with committed data, join it first (FR-172). Running it again refreshes hooks and kit.
pub fn init(ctx: &Ctx, agent_choice: AgentChoice, offline: bool) -> Result<()> {
    let repo = Repo::discover(&ctx.cwd)?;
    let shared = repo.has_committed_data();
    let setup = lifecycle::init(&repo, &open_store)?;
    let join: Option<resonance_core::api::JoinView> =
        if shared && !offline && !mode_of(&repo)?.opted_in() {
            // A teammate shared the graph: join it and opt in (FR-172, FR-186).
            Some(ctx.run("join", json!({"exe": exe()?}))?)
        } else {
            None
        };
    // The first reconcile reads every file: in the daemon as it starts, or here.
    let status: StatusView = ctx.run("status", json!({}))?;
    let hooks: Vec<HookChange> = match &join {
        Some(j) => j.hooks.clone(),
        None => ctx.run("install_hooks", json!({"exe": exe()?}))?,
    };
    let kit = match agent_choice {
        AgentChoice::None => None,
        AgentChoice::ClaudeCode | AgentChoice::Generic => {
            let mode = mode_of(&repo)?;
            Some(agent::install_kit(
                &repo,
                mode,
                &kit_options(exe()?, true, agent_choice == AgentChoice::Generic, false),
            )?)
        }
    };
    let view = json!({"setup": setup, "join": join, "hooks": hooks, "counts": status.counts, "agent": kit});
    ctx.emit(&view, |_| {
        let mut out = String::new();
        match &join {
            Some(join) => {
                out.push_str(&format!(
                    "Joined the shared Resonance graph in {}; you have opted in (stage 2).\n",
                    repo.root.display()
                ));
                if join.merged_operations > 0 {
                    out.push_str(&format!(
                        "Your offline work ({} operation(s)) joins the shared graph as new deltas; commit them with your next commit.\n",
                        join.merged_operations
                    ));
                }
                out.push_str("Your commits are now checked for consistency with the graph (pre-commit, pre-push). `resonance leave` steps out.\n");
            }
            None if setup.already_initialized => out.push_str(&format!(
                "Resonance is already set up in {}; refreshed its hooks and agent kit.\n",
                repo.root.display()
            )),
            None => {
                out.push_str(&format!(
                    "Initialized Resonance in {}/.resonance ({}).\n",
                    repo.root.display(),
                    if status.stage == "1" { "offline mode" } else { "offline, over the shared graph" }
                ));
                if setup.exclusion_added {
                    out.push_str("Added /.resonance/ to .git/info/exclude; `git status` is unchanged.\n");
                }
                out.push('\n');
                out.push_str(OFFLINE_EXPLANATION);
                out.push_str("\n\n");
            }
        }
        out.push_str(&render::counts_line(&status.counts));
        out.push('\n');
        for h in &hooks {
            if h.action != "unchanged" {
                out.push_str(&format!("hook {}: {}\n", h.hook, h.action));
            }
            if let Some(i) = &h.instructions {
                out.push_str(i);
                out.push('\n');
            }
        }
        match &kit {
            Some(k) => out.push_str(&kit_text(k)),
            None => out.push_str("No agent kit installed; `resonance init --agent claude-code` (or `generic`) installs it (FR-210).\n"),
        }
        out
    })
}

fn agent_text(a: &AgentInstall) -> String {
    let mut out = String::new();
    if a.ran && a.succeeded {
        out.push_str("Registered the Resonance MCP server with Claude Code (local scope: this project, this developer).\n");
    } else if let Some(i) = &a.instructions {
        if a.ran {
            out.push_str(&format!(
                "Registering with Claude Code failed:\n{}\n",
                a.output
            ));
        }
        out.push_str(i);
        out.push('\n');
    }
    out
}

/// This working tree's mode, read without the daemon.
fn mode_of(repo: &Repo) -> Result<resonance_core::history::Mode> {
    let config = resonance_core::repo::config::Config::load(&repo.config_path())?;
    Ok(resonance_core::history::mode(repo, &config))
}

fn kit_options(exe: PathBuf, register_mcp: bool, generic: bool, dry_run: bool) -> KitOptions {
    KitOptions {
        exe,
        home: super::home_dir(),
        register_mcp,
        generic,
        dry_run,
    }
}

fn kit_text(r: &KitReport) -> String {
    let mut out = String::new();
    if r.harness == "generic" {
        out.push_str("For your agent (FR-214):\n\n1. Run the MCP server as a stdio server in this directory:\n   ");
        out.push_str(r.mcp_command.as_deref().unwrap_or("resonance mcp"));
        out.push_str(
            "\n\n2. Add this section to AGENTS.md (or your agent's instructions file):\n\n",
        );
        out.push_str(r.section.as_deref().unwrap_or_default());
        out.push('\n');
    } else if let Some(m) = &r.mcp {
        out.push_str(&agent_text(m));
    }
    let changed: Vec<_> = r.steps.iter().filter(|s| s.action != "unchanged").collect();
    if !changed.is_empty() {
        out.push_str(&format!(
            "Agent kit {} (stage {}, {}):\n",
            r.version, r.stage, r.participation
        ));
        for s in changed {
            out.push_str(&format!("  {}: {} ({})\n", s.what, s.path, s.action));
        }
        out.push_str("Start a new agent session here and ask it for the Resonance orientation. Ask it to bring your requirements documents into Resonance.\n");
    }
    out
}

/// After `share`: refresh an installed kit for stage 2 (MVP-037).
pub fn refresh_kit(repo: &Repo) -> Result<Option<KitReport>> {
    let Some(rec) = agent::kit_record(repo) else {
        return Ok(None);
    };
    let generic = rec.harness == "generic";
    let mode = mode_of(repo)?;
    Ok(Some(agent::install_kit(
        repo,
        mode,
        &kit_options(exe()?, false, generic, false),
    )?))
}

/// `resonance agent-hook pre-git` (FR-222): the harness hook. Reads Claude Code's hook input
/// on standard input and, to stop the command, prints a `deny` decision with the reason.
pub fn agent_hook(ctx: &Ctx, event: &str) -> Result<()> {
    if event != "pre-git" {
        return Err(Error::Usage(format!(
            "unknown agent hook event `{event}` (expected pre-git)"
        )));
    }
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    let v: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let command = v["tool_input"]["command"]
        .as_str()
        .or(v["command"].as_str())
        .unwrap_or(input.trim())
        .to_string();
    let (git, _) = resonance_core::api::git_subcommands(&command);
    if git.is_empty() {
        return Ok(());
    }
    if !Repo::discover(&ctx.cwd).is_ok_and(|r| r.is_initialized()) {
        return Ok(()); // Not a Resonance working tree: nothing to gate.
    }
    let d: GateDecision = match ctx.run("agent_gate", json!({"command": command})) {
        Ok(d) => d,
        Err(e) => {
            ctx.note(&format!(
                "resonance: the agent gate could not run ({e}); the git hooks still check"
            ));
            return Ok(());
        }
    };
    if ctx.json {
        return ctx.emit(&d, |_| String::new());
    }
    if d.decision == "deny" {
        let out = json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": d.reason,
            }
        });
        return ctx.emit(&out, |o| format!("{o}\n"));
    }
    if d.overridden {
        ctx.note("resonance: the developer overrode the agent gate; the git hooks still check.");
    }
    Ok(())
}

pub fn status(ctx: &Ctx) -> Result<()> {
    let view: StatusView = ctx.run("status", json!({}))?;
    ctx.emit(&view, render::status)
}

pub fn reconcile(ctx: &Ctx, full: bool) -> Result<()> {
    let report: ReconcileReport = ctx.run("reconcile", json!({"full": full}))?;
    ctx.emit(&report, render::reconcile)
}

pub fn rebuild_derived(ctx: &Ctx) -> Result<()> {
    let report: ReconcileReport = ctx.run("rebuild_derived", json!({}))?;
    ctx.emit(&report, |r| {
        format!("Rebuilt derived data.\n{}", render::reconcile(r))
    })
}

pub fn rebuild(ctx: &Ctx) -> Result<()> {
    let report: ReconcileReport = ctx.run("rebuild", json!({}))?;
    ctx.emit(&report, |r| {
        format!(
            "Re-folded the graph from its snapshot and deltas.\n{}",
            render::reconcile(r)
        )
    })
}

pub fn restore(ctx: &Ctx, file: &std::path::Path) -> Result<()> {
    let path = if file.is_absolute() {
        file.to_path_buf()
    } else {
        ctx.cwd.join(file)
    };
    let jsonl = fs::read_to_string(&path)
        .map_err(|e| Error::io(format!("reading {}", path.display()), e))?;
    let report: resonance_core::api::RestoreReport = ctx.run("restore", json!({"jsonl": jsonl}))?;
    ctx.emit(&report, |r| {
        format!(
            "Restored {} operation(s) from {}; skipped {} for things that already exist.\n",
            r.applied,
            path.display(),
            r.skipped
        )
    })
}

pub fn backup(ctx: &Ctx, restore: bool, from: Option<PathBuf>, list: bool) -> Result<()> {
    let repo = Repo::discover(&ctx.cwd)?;
    let home = super::home_dir();
    if list {
        let dir = lifecycle::backups_dir(&repo, &home);
        let mut all: Vec<String> = fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok().map(|e| e.path().display().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        all.sort();
        return ctx.emit(&all, |all| {
            if all.is_empty() {
                format!("No backups in {}.\n", dir.display())
            } else {
                all.iter().map(|p| format!("{p}\n")).collect()
            }
        });
    }
    if restore {
        let from = match from {
            Some(f) => f,
            None => lifecycle::latest_backup(&repo, &home).ok_or_else(|| {
                Error::NotFound(
                    "a backup of this repository (see `resonance backup --list`)".into(),
                )
            })?,
        };
        lifecycle::restore_backup(&repo, &from)?;
        return ctx.emit(&json!({"restoredFrom": from}), |_| {
            format!(
                "Restored .resonance/ from {}. The next command brings it up to date.\n",
                from.display()
            )
        });
    }
    if !repo.is_initialized() {
        return Err(Error::NotInitialized);
    }
    let dest = lifecycle::backup(&repo, &home, "")?;
    ctx.emit(&json!({"backup": dest}), |_| {
        format!("Backed up .resonance/ to {}.\n", dest.display())
    })
}

pub fn version(ctx: &Ctx) -> Result<()> {
    ctx.emit(&version_info(), render::version)
}

/// The export as JSON Lines, and its number of operations (FR-166).
fn export_text(ctx: &Ctx) -> Result<(String, usize)> {
    let v: Value = ctx.run("export", json!({}))?;
    Ok((
        v["jsonl"].as_str().unwrap_or_default().to_string(),
        v["operations"].as_u64().unwrap_or(0) as usize,
    ))
}

pub fn export(ctx: &Ctx, out: Option<PathBuf>) -> Result<()> {
    let (text, n) = export_text(ctx)?;
    match out {
        None => {
            let mut stdout = std::io::stdout().lock();
            stdout
                .write_all(text.as_bytes())
                .and_then(|_| stdout.flush())
                .map_err(|e| Error::io("writing output", e))
        }
        Some(path) => {
            let path = if path.is_relative() {
                ctx.cwd.join(path)
            } else {
                path
            };
            write_file(&path, &text)?;
            ctx.emit(&json!({"path": path, "operations": n}), |_| {
                format!("Exported {n} operations to {}\n", path.display())
            })
        }
    }
}

fn write_file(path: &PathBuf, text: &str) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
    }
    let mut file =
        fs::File::create(path).map_err(|e| Error::io(format!("creating {}", path.display()), e))?;
    file.write_all(text.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| Error::io(format!("writing {}", path.display()), e))
}

pub fn eject(ctx: &Ctx, dry_run: bool, yes: bool, out: Option<PathBuf>) -> Result<()> {
    let repo = ctx.repo()?;
    if !repo.is_initialized() {
        return Err(Error::NotInitialized);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        Error::Invalid("HOME is not set; give the export location with --out".into())
    })?;
    let path = match out {
        Some(p) if p.is_relative() => ctx.cwd.join(p),
        Some(p) => p,
        None => lifecycle::default_export_path(&repo, &home),
    };
    if path.starts_with(repo.resonance_dir()) {
        return Err(Error::Usage(
            "the export cannot go inside .resonance/, which eject removes".into(),
        ));
    }
    if path.starts_with(&repo.root) {
        ctx.note(&format!(
            "Note: {} is inside the repository; git will see it.",
            path.display()
        ));
    }
    let plan = format!(
        "resonance eject will:\n  1. export the graph to {}\n  2. stop the daemon, if one is running\n  3. remove Resonance's blocks from the git hooks, and the agent kit's local files and settings\n  4. remove {}\n  5. remove the /.resonance/ line from .git/info/exclude\nNo other file is touched.\n",
        path.display(),
        repo.resonance_dir().display()
    );
    if dry_run {
        return ctx.emit(&json!({"dryRun": true, "export": path}), |_| plan);
    }
    let (text, n) = export_text(ctx)?;
    write_file(&path, &text)?;
    ctx.note(&format!("Exported {n} operations to {}", path.display()));
    let go = yes
        || confirm(&format!(
            "Remove {} and its exclusion line? The export above is the only copy of your graph.",
            repo.resonance_dir().display()
        ))
        .unwrap_or(false);
    if !go {
        ctx.note("Nothing was removed. Run `resonance eject --yes` to remove .resonance/.");
        return Err(Error::Invalid("not confirmed".into()));
    }
    client::stop(&repo)?;
    let removal = lifecycle::remove(&repo)?;
    ctx.emit(
        &json!({"export": path, "operations": n, "removal": removal}),
        |_| {
            format!(
                "Removed .resonance/ and its exclusion. Your graph is saved in {}.\n",
                path.display()
            )
        },
    )
}
