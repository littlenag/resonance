//! Linking and annotation: link, unlink, confirm, annotate, reattach.

use resonance_core::api::LinkResult;
use resonance_core::error::Result;
use resonance_core::query::AnnotationView;
use serde_json::json;

use super::Ctx;
use crate::render;

pub fn link(ctx: &Ctx, src: &str, etype: &str, dst: &str, confidence: Option<f64>) -> Result<()> {
    let result: LinkResult = ctx.run(
        "link",
        json!({"src": src, "type": etype, "dst": dst, "confidence": confidence}),
    )?;
    ctx.emit(&result, render::link_result)
}

pub fn unlink(ctx: &Ctx, edge: &str) -> Result<()> {
    let view: LinkResult = ctx.run("unlink", json!({"ref": edge}))?;
    ctx.emit(&view, |l| {
        render::link_change("Removed (kept for history)", l)
    })
}

pub fn confirm(ctx: &Ctx, edge: &str) -> Result<()> {
    let view: LinkResult = ctx.run("confirm", json!({"ref": edge}))?;
    ctx.emit(&view, |l| render::link_change("Confirmed", l))
}

pub fn annotate(ctx: &Ctx, target: &str, kind: &str, text: &str, task: Option<&str>) -> Result<()> {
    let view: AnnotationView = ctx.run(
        "annotate",
        json!({"target": target, "kind": kind, "text": text, "task": task}),
    )?;
    ctx.emit(&view, render::annotation)
}

pub fn reattach(ctx: &Ctx, reference: &str, target: &str) -> Result<()> {
    let view: LinkResult = ctx.run("reattach", json!({"ref": reference, "target": target}))?;
    ctx.emit(&view, |l| render::link_change("Re-attached", l))
}
