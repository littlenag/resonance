//! The shared graph: share, leave, unshare and check, and the git plumbing (hooks, the
//! snapshot merge driver and the delta textconv driver).

use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};

use resonance_core::api::{CheckReport, LeaveView, ShareView, render_delta};
use resonance_core::error::{Error, Result};
use resonance_core::history::compact;
use resonance_core::store::GraphStore;
use resonance_store_sqlite::SqliteStore;
use serde_json::json;

use super::{Ctx, confirm, exe, home_dir};
use crate::cli::GitDriverCommand;

fn render_share(v: &ShareView) -> String {
    let mut out = String::new();
    if v.applied {
        out.push_str("Shared the graph (stage 2); you have opted in.\n");
    } else {
        out.push_str(
            "Sharing would commit this graph next to the code (privacy review, NFR-012):\n",
        );
    }
    for (t, n) in &v.counts {
        let _ = writeln!(out, "  {n} {t}(s)");
    }
    if !v.annotations.is_empty() {
        out.push_str("Annotations (their full text is committed unless kept local):\n");
        for a in &v.annotations {
            let _ = writeln!(
                out,
                "  {} [{}] {}{}",
                a["id"].as_str().unwrap_or_default(),
                a["kind"].as_str().unwrap_or_default(),
                a["text"].as_str().unwrap_or_default(),
                if a["local"] == true {
                    "  (kept local)"
                } else {
                    ""
                }
            );
        }
    }
    let _ = writeln!(out, "Operations in the snapshot: {}", v.operations);
    let _ = writeln!(
        out,
        "{} files under .resonance/:",
        if v.applied {
            "New"
        } else {
            "It would write these"
        }
    );
    for f in &v.files {
        let _ = writeln!(out, "  {f}");
    }
    for h in &v.hooks {
        let _ = writeln!(out, "  hook {}: {}", h.hook, h.action);
        if let Some(i) = &h.instructions {
            let _ = writeln!(out, "{i}");
        }
    }
    if v.applied {
        out.push_str("\nCommit them: git add .resonance && git commit -m \"Share the Resonance graph\"\nUndo with `resonance unshare`.\n");
    } else {
        out.push_str("\nKeep an annotation private with --keep-local <id>. Run with --yes (or answer the prompt) to share.\n");
    }
    out
}

pub fn share(ctx: &Ctx, keep_local: &[String], dry_run: bool, yes: bool) -> Result<()> {
    let exe = exe()?;
    let plan: ShareView = ctx.run(
        "share",
        json!({"exe": exe, "keepLocal": keep_local, "apply": false}),
    )?;
    let repo = ctx.repo()?;
    let section = resonance_core::agent::write_shared_section(&repo, true)?;
    let preview = |v: &ShareView| {
        let mut out = render_share(v);
        if section.action != "unchanged" {
            let _ = writeln!(
                out,
                "It also adds the agent kit's section to {} (FR-213), worded for teammates whether or not they use Resonance.",
                section.path
            );
        }
        out
    };
    if dry_run {
        return ctx.emit(&json!({"share": plan, "instructions": section}), |_| {
            preview(&plan)
        });
    }
    let go = yes || {
        eprint!("{}", preview(&plan));
        confirm("Share the graph now?").unwrap_or(false)
    };
    if !go {
        return Err(Error::Invalid(
            "not shared; pass --yes to share without a terminal (FR-172)".into(),
        ));
    }
    let done: ShareView = ctx.run(
        "share",
        json!({"exe": exe, "keepLocal": keep_local, "apply": true}),
    )?;
    let section = resonance_core::agent::write_shared_section(&repo, false)?;
    let kit = super::setup::refresh_kit(&repo)?;
    ctx.emit(
        &json!({"share": done, "instructions": section, "agent": kit}),
        |_| {
            let mut out = render_share(&done);
            if section.action != "unchanged" {
                let _ = writeln!(
                    out,
                    "Added the agent kit's section to {}; commit it with .resonance/.",
                    section.path
                );
            }
            out.push_str(match kit {
                Some(_) => "Refreshed your agent kit for stage 2.\n",
                None => "Onboard your agent with `resonance init` (FR-210).\n",
            });
            out
        },
    )
}

pub fn leave(ctx: &Ctx, export: Option<PathBuf>) -> Result<()> {
    let export = export.map(|p| if p.is_absolute() { p } else { ctx.cwd.join(p) });
    let v: LeaveView = ctx.run("leave", json!({"export": export}))?;
    ctx.emit(&v, |v| {
        let mut out = String::from(
            "You have left: Resonance's hooks are removed and you are no longer opted in here.\n",
        );
        if let Some(e) = &v.export {
            let _ = writeln!(out, "Your uncommitted graph changes are in {e}.");
        }
        out.push_str(
            "Your local graph stays in .resonance/local/; `resonance init` joins again.\n",
        );
        out
    })
}

pub fn unshare(ctx: &Ctx, yes: bool, out: Option<PathBuf>) -> Result<()> {
    let repo = ctx.repo()?;
    let export =
        out.unwrap_or_else(|| resonance_core::lifecycle::default_export_path(&repo, &home_dir()));
    let go = yes
        || confirm("Stop sharing the graph? The committed .resonance/ files will be staged for removal (your graph stays here, offline).")
            .unwrap_or(false);
    if !go {
        return Err(Error::Invalid(
            "not unshared; pass --yes to proceed without a terminal".into(),
        ));
    }
    let v: LeaveView = ctx.run("unshare", json!({"export": export, "exe": exe()?}))?;
    ctx.emit(&v, |v| {
        format!(
            "Exported the graph to {}. Staged the removal of {} file(s) under .resonance/; commit it to stop sharing.\nYour graph stays here, offline (stage 1).\n",
            v.export.as_deref().unwrap_or("?"),
            v.staged_removals.len()
        )
    })
}

pub fn render_check(r: &CheckReport) -> String {
    let mut out = String::new();
    if r.findings.is_empty() {
        let _ = writeln!(out, "resonance check ({}): ok", r.scope);
    }
    for f in &r.findings {
        let _ = writeln!(out, "{} {} {}", f.code, f.severity, f.message);
        for p in f.paths.iter().take(10) {
            let _ = writeln!(out, "    {p}");
        }
        let _ = writeln!(out, "    fix: {}", f.fix);
    }
    if !r.staged.is_empty() {
        let _ = writeln!(out, "Staged with your commit: {}", r.staged.join(", "));
    }
    out
}

fn fail_on_errors(ctx: &Ctx, r: &CheckReport) -> Result<()> {
    ctx.emit(r, render_check)?;
    if r.errors > 0 {
        return Err(Error::CheckFailed(format!(
            "{} consistency error(s); fix them, then try again",
            r.errors
        )));
    }
    Ok(())
}

pub fn check(ctx: &Ctx, staged: bool, range: Option<String>) -> Result<()> {
    if !staged && range.is_none() {
        return Err(Error::Usage("give --staged or --range <a..b>".into()));
    }
    let r: CheckReport = ctx.run("check", json!({"range": range, "stage": false}))?;
    fail_on_errors(ctx, &r)
}

/// Git's textconv driver for `*.delta`: a delta file for people (FR-066).
fn textconv(ctx: &Ctx, file: &Path) -> Result<()> {
    let path = if file.is_absolute() {
        file.to_path_buf()
    } else {
        ctx.cwd.join(file)
    };
    let bytes =
        std::fs::read(&path).map_err(|e| Error::io(format!("reading {}", path.display()), e))?;
    let text = render_delta(&bytes);
    ctx.emit(&json!({"text": text}), |_| text.clone())
}

pub fn hook(ctx: &Ctx, name: &str, args: &[String]) -> Result<()> {
    let mut stdin = String::new();
    if name == "pre-push" {
        let _ = std::io::stdin().read_to_string(&mut stdin);
    }
    let repo = ctx.repo()?;
    if !repo.is_initialized() {
        return Ok(()); // Resonance is not set up in this working tree: nothing to do.
    }
    let r: CheckReport = ctx.run("hook", json!({"name": name, "args": args, "stdin": stdin}))?;
    if r.findings.is_empty() && r.staged.is_empty() {
        return Ok(());
    }
    eprint!("{}", render_check(&r));
    if r.errors > 0 {
        eprintln!(
            "resonance: the {name} hook found consistency errors (bypassing with --no-verify is detected before the push, C8)"
        );
        return Err(Error::CheckFailed(format!(
            "{} consistency error(s)",
            r.errors
        )));
    }
    Ok(())
}

pub fn git_driver(ctx: &Ctx, cmd: GitDriverCommand) -> Result<()> {
    let (ours, theirs) = match cmd {
        GitDriverCommand::Merge {
            base: _,
            ours,
            theirs,
        } => (ours, theirs),
        GitDriverCommand::Textconv { file } => return textconv(ctx, &file),
    };
    let store = SqliteStore::open_in_memory()?;
    let merged = compact::merge_files(&store as &dyn GraphStore, &ours, &theirs)?;
    eprintln!(
        "resonance: merged two snapshots ({} operations, {} folded deltas)",
        merged.ops.len(),
        merged.meta.folded_deltas.len()
    );
    Ok(())
}
