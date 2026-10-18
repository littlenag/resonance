//! The shared graph: share, leave, unshare, compact, check, review, show-delta, and the git
//! plumbing (hooks and the snapshot merge driver).

use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};

use resonance_core::api::{CheckReport, LeaveView, ReviewView, ShareView, render_delta};
use resonance_core::error::{Error, Result};
use resonance_core::history::compact::{self, Compaction};
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
    if dry_run {
        return ctx.emit(&plan, render_share);
    }
    let go = yes || {
        eprint!("{}", render_share(&plan));
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
    let kit = super::setup::refresh_kit(&ctx.repo()?)?;
    ctx.emit(&done, |d| {
        let mut out = render_share(d);
        out.push_str(match kit {
            Some(_) => "Refreshed the agent kit for stage 2 (`resonance agent install --shared` also adds a section to the shared CLAUDE.md).\n",
            None => "Onboard your agent for stage 2 with `resonance agent install` (FR-210).\n",
        });
        out
    })
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

pub fn compact(ctx: &Ctx, commit: bool, anywhere: bool) -> Result<()> {
    let c: Compaction = ctx.run("compact", json!({"commit": commit, "anywhere": anywhere}))?;
    ctx.emit(&c, |c| {
        if c.folded.is_empty() {
            return "Nothing to compact: no sealed deltas in effect.\n".into();
        }
        let mut out = format!(
            "Compacted {} delta(s) into a new snapshot ({} operation(s), epoch {}).\n",
            c.folded.len(),
            c.operations,
            &c.epoch[..c.epoch.len().min(12)]
        );
        match &c.committed {
            Some(h) => {
                let _ = writeln!(out, "Committed as {}.", &h[..h.len().min(10)]);
            }
            None => out.push_str("Commit .resonance/graph.db and the removed delta files.\n"),
        }
        out
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

pub fn render_review(v: &ReviewView, markdown: bool) -> String {
    let mut out = String::new();
    let (h, b) = if markdown { ("### ", "- ") } else { ("", "  ") };
    let _ = writeln!(out, "{h}Resonance graph changes ({})", v.range);
    let section = |out: &mut String, title: &str, items: &[String]| {
        if items.is_empty() {
            return;
        }
        let _ = writeln!(
            out,
            "\n{}{title} ({})",
            if markdown { "**" } else { "" },
            items.len()
        );
        if markdown {
            out.pop();
            out.push_str("**\n");
        }
        for i in items {
            let _ = writeln!(out, "{b}{i}");
        }
    };
    section(&mut out, "Intent added", &v.intent_added);
    section(&mut out, "Intent changed", &v.intent_changed);
    section(&mut out, "Intent retired", &v.intent_retired);
    section(&mut out, "Links added", &v.links_added);
    section(&mut out, "Links removed", &v.links_removed);
    section(&mut out, "Annotations", &v.annotations);
    section(&mut out, "Conflicts", &v.conflicts);
    section(
        &mut out,
        "Files changed without Resonance",
        &v.files_outside_resonance,
    );
    if v.link_status_changes > 0 || v.identity_operations > 0 {
        let _ = writeln!(
            out,
            "\n{b}{} link status change(s), {} identity operation(s)",
            v.link_status_changes, v.identity_operations
        );
    }
    if v.compacted {
        let _ = writeln!(out, "{b}the snapshot was compacted");
    }
    if v.deltas.is_empty() && !v.compacted {
        let _ = writeln!(out, "{b}no graph changes");
    }
    out
}

pub fn review(ctx: &Ctx, range: &str, markdown: bool) -> Result<()> {
    let v: ReviewView = ctx.run("review", json!({"range": range}))?;
    ctx.emit(&v, |v| render_review(v, markdown))
}

pub fn show_delta(ctx: &Ctx, file: &Path) -> Result<()> {
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

pub fn git_driver(cmd: GitDriverCommand) -> Result<()> {
    let GitDriverCommand::Merge {
        base: _,
        ours,
        theirs,
    } = cmd;
    let store = SqliteStore::open_in_memory()?;
    let merged = compact::merge_files(&store as &dyn GraphStore, &ours, &theirs)?;
    eprintln!(
        "resonance: merged two snapshots ({} operations, {} folded deltas)",
        merged.ops.len(),
        merged.meta.folded_deltas.len()
    );
    Ok(())
}
