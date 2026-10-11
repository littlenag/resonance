//! Reading: show, trace, why, search, unverified, context.

use resonance_core::error::Result;
use resonance_core::query::context::ContextPack;
use resonance_core::query::search::{SearchHit, Unverified};
use resonance_core::query::show::ShowView;
use resonance_core::query::trace::TraceView;
use resonance_core::query::why::WhyView;
use serde_json::json;

use super::Ctx;
use crate::render;

pub fn show(ctx: &Ctx, reference: &str) -> Result<()> {
    let view: ShowView = ctx.run("show", json!({"ref": reference}))?;
    ctx.emit(&view, render::show)
}

pub fn trace(ctx: &Ctx, requirement: &str) -> Result<()> {
    let view: TraceView = ctx.run("trace", json!({"ref": requirement}))?;
    ctx.emit(&view, render::trace)
}

pub fn why(ctx: &Ctx, unit: &str) -> Result<()> {
    let view: WhyView = ctx.run("why", json!({"ref": unit}))?;
    ctx.emit(&view, render::why)
}

pub fn search(
    ctx: &Ctx,
    text: &str,
    ntype: Option<&str>,
    status: Option<&str>,
    limit: usize,
) -> Result<()> {
    let hits: Vec<SearchHit> = ctx.run(
        "search",
        json!({"text": text, "type": ntype, "status": status, "limit": limit}),
    )?;
    ctx.emit(&hits, |h| render::search(h))
}

pub fn unverified(ctx: &Ctx) -> Result<()> {
    let list: Vec<Unverified> = ctx.run("unverified", json!({}))?;
    ctx.emit(&list, |l| render::unverified(l))
}

pub fn context(ctx: &Ctx, requirements: &[String], task: Option<&str>) -> Result<()> {
    let pack: ContextPack = ctx.run("context", json!({"refs": requirements, "task": task}))?;
    ctx.emit(&pack, render::context)
}
