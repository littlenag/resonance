//! Code units and tests: unit add|edit|remove|list, test add.

use resonance_core::api::{UnitEdit, UnitInput};
use resonance_core::error::Result;

use super::{Ctx, reconciled};
use crate::cli::{TestCommand, UnitCommand};
use crate::render;

pub fn unit(ctx: &Ctx, cmd: UnitCommand) -> Result<()> {
    let (mut engine, _) = reconciled(ctx)?;
    match cmd {
        UnitCommand::Add(r) => {
            let view = engine.unit_add(UnitInput {
                region: r.region,
                match_text: r.match_text,
                name: r.name,
                kind: r.kind,
                run: None,
                test: false,
            })?;
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
            let view = engine.unit_edit(
                &unit,
                UnitEdit {
                    region,
                    match_text,
                    name,
                    kind,
                    run,
                },
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
            let view = engine.unit_remove(&unit)?;
            ctx.emit(&view, |u| {
                format!(
                    "Removed {} (kept for history; its links are now orphaned).\n",
                    u.address
                )
            })
        }
        UnitCommand::List { path } => {
            let list = engine.unit_list(path.as_deref())?;
            ctx.emit(&list, |l| render::units(l))
        }
    }
}

pub fn test_add(ctx: &Ctx, cmd: TestCommand) -> Result<()> {
    let TestCommand::Add { region, run } = cmd;
    let (mut engine, _) = reconciled(ctx)?;
    let view = engine.unit_add(UnitInput {
        region: region.region,
        match_text: region.match_text,
        name: region.name,
        kind: region.kind,
        run,
        test: true,
    })?;
    ctx.emit(&view, render::unit)
}
