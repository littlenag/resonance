//! Every operation as a named method with JSON parameters (FR-101, FR-251).
//!
//! The daemon serves these methods over its socket, direct mode calls them in process, and the
//! MCP server translates its tools into them, so every mode runs the same code.
//!
//! A request carries, besides the method's own arguments, who is asking (`actor`, for
//! provenance), where relative paths are resolved from (`cwd`), and whether the answer may
//! skip the read-your-writes sync (`cached`, FR-102).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::{DecisionInput, Engine, UnitEdit, UnitInput};
use crate::error::{Error, Result};
use crate::ingest::ReconcileOptions;
use crate::model::NodeType;
use crate::ops::Actor;

/// One call of an operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    pub method: String,
    #[serde(default)]
    pub args: Value,
    pub actor: Option<Actor>,
    pub cwd: Option<PathBuf>,
    /// Answer from the graph as it is, without waiting for pending file changes (FR-102).
    #[serde(default)]
    pub cached: bool,
    /// The caller's `RESONANCE_*` environment, which overrides the configuration file for
    /// this call (FR-253), even when a daemon started by someone else serves it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// This process's `RESONANCE_*` environment variables.
pub fn resonance_env() -> BTreeMap<String, String> {
    std::env::vars()
        .filter(|(k, _)| k.starts_with("RESONANCE_"))
        .collect()
}

impl Call {
    pub fn new(method: &str, args: Value) -> Call {
        Call {
            method: method.to_string(),
            args,
            actor: None,
            cwd: None,
            cached: false,
            env: BTreeMap::new(),
        }
    }
}

/// Methods that change the graph (and so notify subscribers, FR-104).
pub const MUTATIONS: &[&str] = &[
    "import",
    "req_add",
    "req_edit",
    "req_retire",
    "decision_add",
    "decision_edit",
    "unit_add",
    "unit_edit",
    "unit_remove",
    "unit_split",
    "unit_merge",
    "link",
    "unlink",
    "confirm",
    "annotate",
    "reattach",
    "test",
    "reconcile",
    "rebuild_derived",
    "rebuild",
    "restore",
    "recover",
    "spec_add",
    "spec_edit",
    "spec_retire",
    "supersede",
    "ext_add",
    "intent_project",
];

/// Every method `dispatch` understands.
pub const METHODS: &[&str] = &[
    "status",
    "repair_list",
    "reconcile",
    "rebuild_derived",
    "rebuild",
    "restore",
    "recover",
    "value",
    "spec_add",
    "spec_edit",
    "spec_retire",
    "spec_list",
    "spec_show",
    "supersede",
    "ext_add",
    "intent_project",
    "export",
    "import",
    "req_add",
    "req_edit",
    "req_retire",
    "decision_add",
    "decision_edit",
    "unit_add",
    "unit_edit",
    "unit_remove",
    "unit_list",
    "unit_split",
    "unit_merge",
    "link",
    "unlink",
    "confirm",
    "annotate",
    "reattach",
    "show",
    "trace",
    "why",
    "search",
    "unverified",
    "context",
    "test",
];

fn args<T: DeserializeOwned>(method: &str, v: &Value) -> Result<T> {
    let v = if v.is_null() { json!({}) } else { v.clone() };
    serde_json::from_value(v).map_err(|e| Error::Usage(format!("{method}: {e}")))
}

fn out<T: Serialize>(v: T) -> Result<Value> {
    Ok(serde_json::to_value(v)?)
}

#[derive(Deserialize)]
struct Ref {
    #[serde(rename = "ref")]
    reference: String,
}

#[derive(Deserialize)]
struct Full {
    #[serde(default)]
    full: bool,
}

#[derive(Deserialize)]
struct Paths {
    paths: Vec<String>,
    #[serde(default)]
    format: Option<String>,
}

#[derive(Deserialize)]
struct ReqAdd {
    key: String,
    text: String,
    kind: Option<String>,
}

#[derive(Deserialize)]
struct ReqEdit {
    #[serde(rename = "ref")]
    reference: String,
    text: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Decision {
    #[serde(rename = "ref")]
    reference: Option<String>,
    title: Option<String>,
    decision: Option<String>,
    context: Option<String>,
    consequences: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnitAdd {
    region: String,
    #[serde(rename = "match")]
    match_text: Option<String>,
    name: String,
    kind: Option<String>,
    run: Option<String>,
    #[serde(default)]
    test: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UnitEditArgs {
    #[serde(rename = "ref")]
    reference: String,
    region: Option<String>,
    #[serde(rename = "match")]
    match_text: Option<String>,
    name: Option<String>,
    kind: Option<String>,
    run: Option<String>,
}

#[derive(Deserialize)]
struct UnitList {
    path: Option<String>,
}

#[derive(Deserialize)]
struct Split {
    #[serde(rename = "ref")]
    reference: String,
    into: Vec<String>,
}

#[derive(Deserialize)]
struct Merge {
    refs: Vec<String>,
    into: String,
}

#[derive(Deserialize)]
struct Link {
    src: String,
    #[serde(rename = "type")]
    etype: String,
    dst: String,
    confidence: Option<f64>,
}

#[derive(Deserialize)]
struct Annotate {
    target: String,
    kind: String,
    text: String,
    task: Option<String>,
}

#[derive(Deserialize)]
struct Reattach {
    #[serde(rename = "ref")]
    reference: String,
    target: String,
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    text: String,
    #[serde(rename = "type")]
    ntype: Option<String>,
    status: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct Context {
    refs: Vec<String>,
    task: Option<String>,
}

/// Run one call against an engine. The caller is responsible for bringing the graph up to
/// date with the disk first (MVP-002, MVP-020).
pub fn dispatch(engine: &mut Engine, call: &Call) -> Result<Value> {
    if call.env.is_empty() {
        return dispatch_inner(engine, call);
    }
    let saved = engine.config.clone();
    let result = engine
        .config
        .apply_env(|k| call.env.get(k).cloned())
        .and_then(|_| dispatch_inner(engine, call));
    engine.config = saved;
    result
}

/// Record the cost an agent reports for a repair (FR-239).
fn count_repair(engine: &mut Engine, a: &Value) -> Result<()> {
    let n = |k: &str| a.get(k).and_then(Value::as_u64).unwrap_or(0);
    engine.count(&[
        ("repair.count", 1),
        ("repair.tokens", n("costTokens")),
        ("repair.seconds", n("costSeconds")),
    ])
}

/// Methods whose use `resonance value` reports (FR-240).
const SERVED: &[&str] = &["trace", "why", "context", "search", "test"];

fn dispatch_inner(engine: &mut Engine, call: &Call) -> Result<Value> {
    let result = dispatch_method(engine, call)?;
    if SERVED.contains(&call.method.as_str()) {
        let name = format!("served.{}", call.method);
        engine.count(&[(name.as_str(), 1)])?;
    }
    Ok(result)
}

fn dispatch_method(engine: &mut Engine, call: &Call) -> Result<Value> {
    let root = engine.repo.root.clone();
    engine.set_cwd(call.cwd.clone().unwrap_or(root));
    if let Some(actor) = &call.actor {
        engine.set_actor(actor.clone());
    }
    let m = call.method.as_str();
    let a = &call.args;
    match m {
        "status" => out(engine.status()?),
        "repair_list" => out(engine.repair_list()?),
        "reconcile" => {
            let f: Full = args(m, a)?;
            out(engine.reconcile(ReconcileOptions {
                full: f.full,
                ..Default::default()
            })?)
        }
        "rebuild_derived" => out(engine.rebuild_derived()?),
        "rebuild" => out(engine.rebuild()?),
        "restore" => {
            #[derive(Deserialize)]
            struct Restore {
                jsonl: String,
            }
            let r: Restore = args(m, a)?;
            out(engine.restore(&r.jsonl)?)
        }
        "export" => {
            let mut buf = Vec::new();
            let operations = engine.export(&mut buf)?;
            out(json!({"operations": operations, "jsonl": String::from_utf8_lossy(&buf)}))
        }
        "import" => {
            let p: Paths = args(m, a)?;
            out(engine.import(&p.paths, p.format.as_deref())?)
        }
        "req_add" => {
            let r: ReqAdd = args(m, a)?;
            out(engine.req_add(&r.key, &r.text, r.kind.as_deref())?)
        }
        "req_edit" => {
            let r: ReqEdit = args(m, a)?;
            out(engine.req_edit(&r.reference, &r.text)?)
        }
        "req_retire" => {
            let r: Ref = args(m, a)?;
            out(engine.req_retire(&r.reference)?)
        }
        "decision_add" | "decision_edit" => {
            let d: Decision = args(m, a)?;
            let input = DecisionInput {
                title: d.title,
                decision: d.decision,
                context: d.context,
                consequences: d.consequences,
                status: d.status,
            };
            if m == "decision_add" {
                out(engine.decision_add(input)?)
            } else {
                let reference = d
                    .reference
                    .ok_or_else(|| Error::Usage("decision_edit: missing `ref`".into()))?;
                out(engine.decision_edit(&reference, input)?)
            }
        }
        "unit_add" => {
            let u: UnitAdd = args(m, a)?;
            out(engine.unit_add(UnitInput {
                region: u.region,
                match_text: u.match_text,
                name: u.name,
                kind: u.kind,
                run: u.run,
                test: u.test,
            })?)
        }
        "unit_edit" => {
            let u: UnitEditArgs = args(m, a)?;
            out(engine.unit_edit(
                &u.reference,
                UnitEdit {
                    region: u.region,
                    match_text: u.match_text,
                    name: u.name,
                    kind: u.kind,
                    run: u.run,
                },
            )?)
        }
        "unit_remove" => {
            let r: Ref = args(m, a)?;
            out(engine.unit_remove(&r.reference)?)
        }
        "unit_list" => {
            let u: UnitList = args(m, a)?;
            out(engine.unit_list(u.path.as_deref())?)
        }
        "unit_split" => {
            let s: Split = args(m, a)?;
            out(engine.unit_split(&s.reference, &s.into)?)
        }
        "unit_merge" => {
            let s: Merge = args(m, a)?;
            out(engine.unit_merge(&s.refs, &s.into)?)
        }
        "link" => {
            let l: Link = args(m, a)?;
            out(engine.link(&l.src, &l.etype, &l.dst, l.confidence)?)
        }
        "unlink" => {
            let r: Ref = args(m, a)?;
            out(engine.unlink(&r.reference)?)
        }
        "confirm" => {
            let r: Ref = args(m, a)?;
            let result = out(engine.confirm(&r.reference)?);
            count_repair(engine, a)?;
            result
        }
        "annotate" => {
            let n: Annotate = args(m, a)?;
            out(engine.annotate(&n.target, &n.kind, &n.text, n.task.as_deref())?)
        }
        "reattach" => {
            let r: Reattach = args(m, a)?;
            let result = out(engine.reattach(&r.reference, &r.target)?);
            count_repair(engine, a)?;
            result
        }
        "recover" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct Recover {
                action: Option<String>,
                id: Option<String>,
                note: Option<String>,
            }
            let r: Recover = args(m, a)?;
            let id = || {
                r.id.clone()
                    .ok_or_else(|| Error::Usage("recover: give the ID to act on".into()))
            };
            match r.action.as_deref().unwrap_or("list") {
                "list" => out(engine.recover_list()?),
                "undo" => out(engine.recover_undo(&id()?)?),
                "dismiss" => out(engine.recover_dismiss(&id()?, r.note.as_deref().unwrap_or(""))?),
                "attach" => out(engine.recover_attach(&id()?)?),
                "drop" => {
                    let home = std::env::var_os("HOME")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_default();
                    out(json!({"exported": engine.recover_drop(&id()?, &home)?}))
                }
                other => Err(Error::Usage(format!(
                    "recover: unknown action `{other}` (expected list, undo, dismiss, attach or drop)"
                ))),
            }
        }
        "value" => out(engine.value()?),
        "spec_add" => {
            #[derive(Deserialize)]
            struct SpecAdd {
                #[serde(rename = "type")]
                ntype: String,
                key: Option<String>,
                #[serde(default)]
                fields: BTreeMap<String, String>,
            }
            let r: SpecAdd = args(m, a)?;
            let t: NodeType = r.ntype.parse().map_err(Error::Usage)?;
            out(engine.spec_add(t, r.key.as_deref(), &r.fields)?)
        }
        "spec_edit" => {
            #[derive(Deserialize)]
            struct SpecEdit {
                #[serde(rename = "ref")]
                reference: String,
                #[serde(default)]
                fields: BTreeMap<String, String>,
            }
            let r: SpecEdit = args(m, a)?;
            out(engine.spec_edit(&r.reference, &r.fields)?)
        }
        "intent_project" => {
            #[derive(Deserialize)]
            struct Project {
                dir: String,
                #[serde(default)]
                apply: bool,
            }
            let r: Project = args(m, a)?;
            out(engine.intent_project(&r.dir, r.apply)?)
        }
        "spec_retire" => {
            let r: Ref = args(m, a)?;
            let (item, links) = engine.spec_retire(&r.reference)?;
            out(json!({"item": item, "links": links}))
        }
        "spec_show" => {
            let r: Ref = args(m, a)?;
            out(engine.spec_item(&r.reference)?)
        }
        "spec_list" => {
            #[derive(Deserialize, Default)]
            #[serde(default)]
            struct SpecList {
                #[serde(rename = "type")]
                ntype: Option<String>,
                status: Option<String>,
                text: Option<String>,
            }
            let r: SpecList = args(m, a)?;
            let t = match r.ntype.as_deref() {
                Some(t) if !t.is_empty() => Some(t.parse::<NodeType>().map_err(Error::Usage)?),
                _ => None,
            };
            out(engine.spec_list(t, r.status.as_deref(), r.text.as_deref())?)
        }
        "supersede" => {
            #[derive(Deserialize)]
            struct Supersede {
                old: String,
                new: String,
            }
            let r: Supersede = args(m, a)?;
            out(engine.supersede(&r.old, &r.new)?)
        }
        "ext_add" => {
            #[derive(Deserialize)]
            struct ExtAdd {
                #[serde(rename = "type")]
                ext_type: String,
                name: String,
                text: Option<String>,
                #[serde(default)]
                props: BTreeMap<String, String>,
            }
            let r: ExtAdd = args(m, a)?;
            out(engine.ext_add(&r.ext_type, &r.name, r.text.as_deref(), &r.props)?)
        }
        "show" => {
            let r: Ref = args(m, a)?;
            out(engine.show(&r.reference)?)
        }
        "trace" => {
            let r: Ref = args(m, a)?;
            out(engine.trace(&r.reference)?)
        }
        "why" => {
            let r: Ref = args(m, a)?;
            out(engine.why(&r.reference)?)
        }
        "search" => {
            let q: Search = args(m, a)?;
            out(engine.search(
                &q.text,
                q.ntype.as_deref(),
                q.status.as_deref(),
                q.limit.unwrap_or(50),
            )?)
        }
        "unverified" => out(engine.unverified()?),
        "context" => {
            let c: Context = args(m, a)?;
            out(engine.context(&c.refs, c.task.as_deref())?)
        }
        "test" => {
            let r: Ref = args(m, a)?;
            out(engine.run_tests(&r.reference)?)
        }
        other => Err(Error::Usage(format!("unknown method `{other}`"))),
    }
}

/// Where operations run: in this process (direct mode) or in the daemon.
pub trait Backend {
    fn call(&mut self, call: Call) -> Result<Value>;
}

/// Direct mode (MVP-002): reconcile in this process, then run the operation.
pub struct Direct {
    pub engine: Engine,
}

impl Backend for Direct {
    fn call(&mut self, mut call: Call) -> Result<Value> {
        if call.cwd.is_none() {
            call.cwd = Some(self.engine.cwd().to_path_buf());
        }
        if !matches!(
            call.method.as_str(),
            "reconcile" | "rebuild_derived" | "rebuild"
        ) {
            self.engine.reconcile(ReconcileOptions::default())?;
        }
        dispatch(&mut self.engine, &call)
    }
}
