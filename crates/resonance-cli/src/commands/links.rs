//! Linking and annotation: link, unlink, confirm, annotate, reattach.

use resonance_core::error::Result;

use super::{Ctx, reconciled};
use crate::render;

pub fn link(ctx: &Ctx, src: &str, etype: &str, dst: &str, confidence: Option<f64>) -> Result<()> {
    let (mut engine, _) = reconciled(ctx)?;
    let result = engine.link(src, etype, dst, confidence)?;
    ctx.emit(&result, render::link_result)
}

pub fn unlink(ctx: &Ctx, edge: &str) -> Result<()> {
    let (mut engine, _) = reconciled(ctx)?;
    let view = engine.unlink(edge)?;
    ctx.emit(&view, |l| {
        format!("Removed (kept for history): {}", render::link(l))
    })
}

pub fn confirm(ctx: &Ctx, edge: &str) -> Result<()> {
    let (mut engine, _) = reconciled(ctx)?;
    let view = engine.confirm(edge)?;
    ctx.emit(&view, render::link)
}

pub fn annotate(ctx: &Ctx, target: &str, kind: &str, text: &str, task: Option<&str>) -> Result<()> {
    let (mut engine, _) = reconciled(ctx)?;
    let view = engine.annotate(target, kind, text, task)?;
    ctx.emit(&view, render::annotation)
}

pub fn reattach(ctx: &Ctx, reference: &str, target: &str) -> Result<()> {
    let (mut engine, _) = reconciled(ctx)?;
    let view = engine.reattach(reference, target)?;
    ctx.emit(&view, |l| format!("Re-attached: {}", render::link(l)))
}
