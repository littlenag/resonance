//! Code units and tests: unit add|edit|remove|list|split|merge, test add.

use resonance_core::api::IdentityChange;
use resonance_core::error::Result;
use resonance_core::query::UnitView;
use serde_json::json;

use super::Ctx;
use crate::cli::{TestCommand, UnitCommand};
use crate::render;

pub fn unit(ctx: &Ctx, cmd: UnitCommand) -> Result<()> {
    match cmd {
        UnitCommand::Add(r) => {
            let view: UnitView = ctx.run(
                "unit_add",
                json!({"region": r.region, "match": r.match_text, "name": r.name, "kind": r.kind, "test": false}),
            )?;
            ctx.emit(&view, render::unit)
        }
        UnitCommand::Edit {
            unit,
            region,
            match_text,
            name,
            kind,
            run,
        } => {
            let confirming = region.is_none()
                && match_text.is_none()
                && name.is_none()
                && kind.is_none()
                && run.is_none();
            let view: UnitView = ctx.run(
                "unit_edit",
                json!({"ref": unit, "region": region, "match": match_text, "name": name, "kind": kind, "run": run}),
            )?;
            ctx.emit(&view, |u| {
                let head = if confirming {
                    "Confirmed the current anchor.\n"
                } else {
                    "Updated.\n"
                };
                format!("{head}{}", render::unit(u))
            })
        }
        UnitCommand::Remove { unit } => {
            let view: UnitView = ctx.run("unit_remove", json!({"ref": unit}))?;
            ctx.emit(&view, |u| {
                format!(
                    "Removed {} (kept for history; its links are now orphaned).\n",
                    u.address
                )
            })
        }
        UnitCommand::List { path } => {
            let list: Vec<UnitView> = ctx.run("unit_list", json!({"path": path}))?;
            ctx.emit(&list, |l| render::units(l))
        }
        UnitCommand::Split { unit, into } => {
            let change: IdentityChange =
                ctx.run("unit_split", json!({"ref": unit, "into": into}))?;
            ctx.emit(&change, |c| render::identity_change("Split", c))
        }
        UnitCommand::Merge { units, into } => {
            let change: IdentityChange =
                ctx.run("unit_merge", json!({"refs": units, "into": into}))?;
            ctx.emit(&change, |c| render::identity_change("Merged", c))
        }
    }
}

pub fn test_add(ctx: &Ctx, cmd: TestCommand) -> Result<()> {
    let TestCommand::Add { region, run } = cmd;
    let view: UnitView = ctx.run(
        "unit_add",
        json!({
            "region": region.region, "match": region.match_text, "name": region.name,
            "kind": region.kind, "run": run, "test": true,
        }),
    )?;
    ctx.emit(&view, render::unit)
}
