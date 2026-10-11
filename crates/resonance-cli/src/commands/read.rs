//! Reading: show, trace, why, search, unverified, context.

use resonance_core::error::Result;

use super::{Ctx, reconciled};
use crate::render;

pub fn show(ctx: &Ctx, reference: &str) -> Result<()> {
    let (engine, _) = reconciled(ctx)?;
    ctx.emit(&engine.show(reference)?, render::show)
}

pub fn trace(ctx: &Ctx, requirement: &str) -> Result<()> {
    let (engine, _) = reconciled(ctx)?;
    ctx.emit(&engine.trace(requirement)?, render::trace)
}

pub fn why(ctx: &Ctx, unit: &str) -> Result<()> {
    let (engine, _) = reconciled(ctx)?;
    ctx.emit(&engine.why(unit)?, render::why)
}

pub fn search(
    ctx: &Ctx,
    text: &str,
    ntype: Option<&str>,
    status: Option<&str>,
    limit: usize,
) -> Result<()> {
    let (engine, _) = reconciled(ctx)?;
    let hits = engine.search(text, ntype, status, limit)?;
    ctx.emit(&hits, |h| render::search(h))
}

pub fn unverified(ctx: &Ctx) -> Result<()> {
    let (engine, _) = reconciled(ctx)?;
    let list = engine.unverified()?;
    ctx.emit(&list, |l| render::unverified(l))
}

pub fn context(ctx: &Ctx, requirements: &[String], task: Option<&str>) -> Result<()> {
    let (engine, _) = reconciled(ctx)?;
    ctx.emit(&engine.context(requirements, task)?, render::context)
}
