//! Intent: import, req, decision.

use resonance_core::api::DecisionInput;
use resonance_core::error::Result;

use super::{Ctx, reconciled};
use crate::cli::{DecisionCommand, ReqCommand};
use crate::render;

pub fn import(ctx: &Ctx, paths: &[String]) -> Result<()> {
    let (mut engine, _) = reconciled(ctx)?;
    let view = engine.import(paths)?;
    ctx.emit(&view, render::import)
}

pub fn req(ctx: &Ctx, cmd: ReqCommand) -> Result<()> {
    let (mut engine, _) = reconciled(ctx)?;
    let view = match cmd {
        ReqCommand::Add { key, text, kind } => engine.req_add(&key, &text, kind.as_deref())?,
        ReqCommand::Edit { requirement, text } => engine.req_edit(&requirement, &text)?,
        ReqCommand::Retire { requirement } => engine.req_retire(&requirement)?,
    };
    ctx.emit(&view, render::requirement)
}

pub fn decision(ctx: &Ctx, cmd: DecisionCommand) -> Result<()> {
    let (mut engine, _) = reconciled(ctx)?;
    let view = match cmd {
        DecisionCommand::Add {
            title,
            decision,
            context,
            consequences,
        } => engine.decision_add(DecisionInput {
            title: Some(title),
            decision: Some(decision),
            context,
            consequences,
            status: None,
        })?,
        DecisionCommand::Edit {
            decision_ref,
            title,
            decision,
            context,
            consequences,
            status,
        } => engine.decision_edit(
            &decision_ref,
            DecisionInput {
                title,
                decision,
                context,
                consequences,
                status,
            },
        )?,
    };
    ctx.emit(&view, render::decision)
}
