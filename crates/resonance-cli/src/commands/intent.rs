//! Intent: import, req, decision.

use resonance_core::api::ImportView;
use resonance_core::error::Result;
use resonance_core::query::{DecisionView, RequirementView};
use serde_json::json;

use super::Ctx;
use crate::cli::{DecisionCommand, ReqCommand};
use crate::render;

pub fn import(ctx: &Ctx, paths: &[String]) -> Result<()> {
    let view: ImportView = ctx.run("import", json!({"paths": paths}))?;
    ctx.emit(&view, render::import)
}

pub fn req(ctx: &Ctx, cmd: ReqCommand) -> Result<()> {
    let view: RequirementView = match cmd {
        ReqCommand::Add { key, text, kind } => {
            ctx.run("req_add", json!({"key": key, "text": text, "kind": kind}))?
        }
        ReqCommand::Edit { requirement, text } => {
            ctx.run("req_edit", json!({"ref": requirement, "text": text}))?
        }
        ReqCommand::Retire { requirement } => ctx.run("req_retire", json!({"ref": requirement}))?,
    };
    ctx.emit(&view, render::requirement)
}

pub fn decision(ctx: &Ctx, cmd: DecisionCommand) -> Result<()> {
    let view: DecisionView = match cmd {
        DecisionCommand::Add {
            title,
            decision,
            context,
            consequences,
        } => ctx.run(
            "decision_add",
            json!({"title": title, "decision": decision, "context": context, "consequences": consequences}),
        )?,
        DecisionCommand::Edit {
            decision_ref,
            title,
            decision,
            context,
            consequences,
            status,
        } => ctx.run(
            "decision_edit",
            json!({
                "ref": decision_ref, "title": title, "decision": decision,
                "context": context, "consequences": consequences, "status": status,
            }),
        )?,
    };
    ctx.emit(&view, render::decision)
}
