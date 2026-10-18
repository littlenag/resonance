//! Intent: import, req, decision.

use std::collections::BTreeMap;

use resonance_core::api::{ImportView, LinkResult};
use resonance_core::error::{Error, Result};
use resonance_core::query::show::ShowView;
use resonance_core::query::{DecisionView, RequirementView, SpecItemView};
use serde_json::{Value, json};

use super::Ctx;
use crate::cli::{
    DecisionCommand, ExtCommand, IntentCommand, ItemCommand, ReqCommand, SpecCommand,
};
use crate::render;

fn fields(pairs: &[(&str, Option<String>)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .filter_map(|(k, v)| v.clone().map(|v| (k.to_string(), v)))
        .collect()
}

pub fn import(ctx: &Ctx, paths: &[String], format: Option<&str>) -> Result<()> {
    let view: ImportView = ctx.run("import", json!({"paths": paths, "format": format}))?;
    ctx.emit(&view, render::import)
}

pub fn req(ctx: &Ctx, cmd: ReqCommand) -> Result<()> {
    let view: RequirementView = match cmd {
        ReqCommand::Add {
            key,
            text,
            kind,
            priority,
            status,
        } => {
            let f = fields(&[
                ("text", Some(text)),
                ("kind", kind),
                ("priority", priority),
                ("status", status),
            ]);
            let item: SpecItemView = ctx.run(
                "spec_add",
                json!({"type": "Requirement", "key": key, "fields": f}),
            )?;
            requirement_of(ctx, &item.id)?
        }
        ReqCommand::Edit {
            requirement,
            text,
            kind,
            priority,
            status,
        } => {
            let f = fields(&[
                ("text", text),
                ("kind", kind),
                ("priority", priority),
                ("status", status),
            ]);
            let item: SpecItemView =
                ctx.run("spec_edit", json!({"ref": requirement, "fields": f}))?;
            requirement_of(ctx, &item.id)?
        }
        ReqCommand::Retire { requirement } => ctx.run("req_retire", json!({"ref": requirement}))?,
        ReqCommand::Supersede { requirement, by } => {
            let r: LinkResult = ctx.run("supersede", json!({"old": requirement, "new": by}))?;
            return ctx.emit(&r, |r| render::link_change("Recorded", r));
        }
    };
    ctx.emit(&view, render::requirement)
}

fn requirement_of(ctx: &Ctx, id: &str) -> Result<RequirementView> {
    let item: SpecItemView = ctx.run("spec_show", json!({"ref": id}))?;
    Ok(RequirementView {
        id: item.id,
        key: item.key.unwrap_or_default(),
        text: item.text,
        kind: item.kind.unwrap_or_default(),
        status: item.status,
        spec_status: item.props["specStatus"].as_str().map(String::from),
        origin: item.origin,
        source: item.source,
        priority: item.priority,
    })
}

pub fn item(ctx: &Ctx, ntype: &str, cmd: ItemCommand) -> Result<()> {
    let view: SpecItemView = match cmd {
        ItemCommand::Add {
            key,
            title,
            text,
            scope,
            status,
        } => {
            let mut f = fields(&[("title", title), ("text", text), ("status", status)]);
            if ntype == "Constraint" {
                if let Some(s) = scope {
                    f.insert("scope".into(), s);
                }
            } else if scope.is_some() {
                return Err(Error::Usage("--scope is only for constraints".into()));
            }
            ctx.run("spec_add", json!({"type": ntype, "key": key, "fields": f}))?
        }
        ItemCommand::Edit {
            item,
            title,
            text,
            scope,
            status,
        } => {
            let f = fields(&[
                ("title", title),
                ("text", text),
                ("scope", scope),
                ("status", status),
            ]);
            ctx.run("spec_edit", json!({"ref": item, "fields": f}))?
        }
        ItemCommand::Retire { item } => {
            let v: Value = ctx.run("spec_retire", json!({"ref": item}))?;
            serde_json::from_value(v["item"].clone())
                .map_err(|e| Error::Daemon(format!("unexpected answer: {e}")))?
        }
        ItemCommand::Supersede { item, by } => {
            let r: LinkResult = ctx.run("supersede", json!({"old": item, "new": by}))?;
            return ctx.emit(&r, |r| render::link_change("Recorded", r));
        }
    };
    ctx.emit(&view, render::spec_item)
}

pub fn spec(ctx: &Ctx, cmd: SpecCommand) -> Result<()> {
    match cmd {
        SpecCommand::List {
            ntype,
            status,
            text,
        } => {
            let items: Vec<SpecItemView> = ctx.run(
                "spec_list",
                json!({"type": ntype, "status": status, "text": text}),
            )?;
            ctx.emit(&items, |items| render::spec_list(items))
        }
        SpecCommand::Show { item } => {
            let v: SpecItemView = ctx.run("spec_show", json!({"ref": item}))?;
            ctx.emit(&v, render::spec_item)
        }
    }
}

pub fn ext(ctx: &Ctx, cmd: ExtCommand) -> Result<()> {
    let ExtCommand::Add {
        ext_type,
        name,
        text,
        props,
    } = cmd;
    let mut map = BTreeMap::new();
    for p in props {
        let (k, v) = p
            .split_once('=')
            .ok_or_else(|| Error::Usage(format!("--prop {p}: expected KEY=VALUE")))?;
        map.insert(k.to_string(), v.to_string());
    }
    let v: ShowView = ctx.run(
        "ext_add",
        json!({"type": ext_type, "name": name, "text": text, "props": map}),
    )?;
    ctx.emit(&v, render::show)
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

pub fn project(ctx: &Ctx, cmd: IntentCommand) -> Result<()> {
    let IntentCommand::Project {
        directory: dir,
        dry_run,
        yes,
    } = cmd;
    let plan: resonance_core::api::ProjectView =
        ctx.run("intent_project", json!({"dir": dir, "apply": false}))?;
    if plan.keys.is_empty() {
        return ctx.emit(&plan, |_| {
            "No graph-native requirements to project.\n".into()
        });
    }
    let preview = format!(
        "Would write {} requirement(s) to {}: {}\nThat file then owns them: edit them there, and Resonance re-imports changes.\n",
        plan.keys.len(),
        plan.path,
        plan.keys.join(", ")
    );
    if dry_run {
        return ctx.emit(&plan, |_| preview);
    }
    let go = yes || super::confirm(&format!("{}Write it?", preview)).unwrap_or(false);
    if !go {
        return Err(Error::Invalid(
            "nothing written; pass --yes to write without a terminal (FR-179)".into(),
        ));
    }
    let done: resonance_core::api::ProjectView =
        ctx.run("intent_project", json!({"dir": dir, "apply": true}))?;
    ctx.emit(&done, |d| {
        format!(
            "Wrote {} requirement(s) to {}; it is now an import source.\n",
            d.keys.len(),
            d.path
        )
    })
}
