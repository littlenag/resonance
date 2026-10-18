//! Recovery and value: `recover` (FR-196, §8.18) and `value` (FR-240).

use std::fmt::Write;

use resonance_core::api::{RecoverView, UndoView, ValueView};
use resonance_core::error::{Error, Result};
use resonance_core::ingest::ReconcileReport;
use resonance_core::recovery::Divergence;
use serde_json::{Value, json};

use super::{Ctx, confirm};
use crate::cli::RecoverCommand;
use crate::render;

pub fn recover(ctx: &Ctx, action: Option<RecoverCommand>) -> Result<()> {
    match action {
        None => {
            let v: RecoverView = ctx.run("recover", json!({"action": "list"}))?;
            ctx.emit(&v, render_recover)
        }
        Some(RecoverCommand::RepairSnapshot) => {
            let v: Value = ctx.run("repair_snapshot", json!({}))?;
            ctx.emit(&v, |v| {
                if v["repaired"] == true {
                    "Merged both sides of .resonance/graph.db and staged the result; finish the merge with `git commit`.\n".into()
                } else {
                    "No conflict on .resonance/graph.db.\n".into()
                }
            })
        }
        Some(RecoverCommand::Undo { id }) => {
            let v: UndoView = ctx.run("recover", json!({"action": "undo", "id": id}))?;
            ctx.emit(&v, |v| {
                format!(
                    "Put back the version of {} that recovery {} had kept.\nThe version that was in place is kept too: `resonance recover undo {}` reverses this.\n",
                    v.record.path, v.record.id, v.saved.id
                )
            })
        }
        Some(RecoverCommand::Dismiss { id, note }) => {
            let d: Divergence = ctx.run(
                "recover",
                json!({"action": "dismiss", "id": id, "note": note}),
            )?;
            ctx.emit(&d, |d| format!("Dismissed {}: {}\n", d.id, d.summary))
        }
        Some(RecoverCommand::Attach { delta }) => {
            let r: ReconcileReport =
                ctx.run("recover", json!({"action": "attach", "id": delta}))?;
            ctx.emit(&r, |r| {
                format!(
                    "Attached the delta to the current commit.\n{}",
                    render::reconcile(r)
                )
            })
        }
        Some(RecoverCommand::Drop { delta, yes }) => {
            let go = yes || confirm(&format!(
                "Export delta {delta} to ~/.resonance/exports and remove it from this repository?"
            ))
            .unwrap_or(false);
            if !go {
                return Err(Error::Invalid(
                    "not dropped; pass --yes to drop without a terminal".into(),
                ));
            }
            let v: Value = ctx.run("recover", json!({"action": "drop", "id": delta}))?;
            ctx.emit(&v, |v| {
                format!(
                    "Exported the delta to {} and removed it.\n",
                    v["exported"].as_str().unwrap_or("?")
                )
            })
        }
    }
}

pub fn divergence_line(d: &Divergence) -> String {
    format!(
        "{} [{:?}, {}] {}",
        &d.id[..d.id.len().min(10)],
        d.class,
        d.level.as_str(),
        d.summary
    )
}

fn render_recover(v: &RecoverView) -> String {
    let mut out = String::new();
    if v.divergences.is_empty() && v.detached.is_empty() && v.held.is_none() {
        out.push_str("Nothing to recover: git, the files and the graph agree.\n");
    }
    if let Some(h) = &v.held {
        let _ = writeln!(
            out,
            "Action required: .resonance/ changed unexpectedly at {} ({} operation(s) would be lost). Run `resonance rebuild` to accept it, or restore the delta files.",
            h.at, h.missing
        );
    }
    for d in &v.divergences {
        let _ = writeln!(out, "{}", divergence_line(d));
        for e in &d.evidence {
            let _ = writeln!(out, "    evidence: {e}");
        }
        if !d.resolution.is_empty() {
            let _ = writeln!(out, "    done: {}", d.resolution);
        }
        if !d.paths.is_empty() {
            let _ = writeln!(out, "    files: {}", d.paths.join(", "));
        }
        let _ = writeln!(
            out,
            "    next: `resonance recover dismiss {}` once handled",
            &d.id[..d.id.len().min(10)]
        );
    }
    if !v.detached.is_empty() {
        out.push_str("\nDetached pool (offline deltas whose commits were rewritten):\n");
        for d in &v.detached {
            let _ = writeln!(
                out,
                "  {} ({} operation(s), was on {}): `resonance recover attach {}` or `resonance recover drop {}`",
                d.id,
                d.operations,
                d.commit
                    .as_deref()
                    .map(|c| &c[..c.len().min(8)])
                    .unwrap_or("?"),
                d.id,
                d.id
            );
        }
    }
    let open: Vec<_> = v.records.iter().filter(|r| r.undone_at.is_none()).collect();
    if !open.is_empty() {
        out.push_str("\nRecent recovery records (displaced versions you can put back):\n");
        for r in open.iter().take(10) {
            let _ = writeln!(
                out,
                "  {} {} ({:?}, {}): `resonance recover undo {}`",
                r.id, r.path, r.class, r.explanation, r.id
            );
        }
    }
    out
}

pub fn value(ctx: &Ctx) -> Result<()> {
    let v: ValueView = ctx.run("value", json!({}))?;
    ctx.emit(&v, |v| {
        let mut out = String::from(
            "What Resonance did for you here (counted locally; nothing leaves this machine):\n",
        );
        let _ = writeln!(out, "  traces answered:        {}", v.traces);
        let _ = writeln!(out, "  why answered:           {}", v.whys);
        let _ = writeln!(out, "  context packs:          {}", v.context_packs);
        let _ = writeln!(out, "  searches:               {}", v.searches);
        let _ = writeln!(out, "  test runs:              {}", v.test_runs);
        let _ = writeln!(
            out,
            "  anchors that followed edits, moves and renames: {}",
            v.anchors_followed
        );
        let _ = writeln!(
            out,
            "  repairs made:           {} (recorded cost: {} tokens, {} s)",
            v.repairs, v.repair_tokens, v.repair_seconds
        );
        if !v.divergences.is_empty() {
            let list: Vec<String> = v
                .divergences
                .iter()
                .map(|(k, n)| format!("{k}: {n}"))
                .collect();
            let _ = writeln!(out, "  divergences classified: {}", list.join(", "));
        }
        if let Some(u) = &v.upgrade {
            let _ = writeln!(out, "\n{u}");
        }
        out
    })
}
