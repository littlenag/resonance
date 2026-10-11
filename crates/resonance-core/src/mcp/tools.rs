//! MCP tool definitions, mirroring CLI commands as `resonance_<command>` (FR-130, FR-254).

use serde_json::{Map, Value, json};

use crate::error::{Error, Result};
use crate::service::Call;

/// Tool names, descriptions and input schemas.
pub fn definitions() -> Vec<Value> {
    let s = |d: &str| json!({"type": "string", "description": d});
    let i = |d: &str| json!({"type": "integer", "minimum": 1, "description": d});
    let reference = "A requirement key (FR-042), unit address (src/lib.rs::open), unique unit name, file path, or ID.";
    let region_props = |extra: Vec<(&str, Value)>| -> Value {
        let mut props = Map::new();
        props.insert("path".into(), s("Repository-relative file path."));
        props.insert("startLine".into(), i("First line of the region (1-based)."));
        props.insert(
            "endLine".into(),
            i("Last line of the region (inclusive). Defaults to startLine."),
        );
        props.insert(
            "match".into(),
            s("Instead of lines: an exact snippet that occurs exactly once in the file."),
        );
        for (k, v) in extra {
            props.insert(k.into(), v);
        }
        Value::Object(props)
    };
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {"type": "object", "properties": properties, "required": required},
        })
    };
    vec![
        tool(
            "resonance_status",
            "Counts and the repair list (orphaned units and links, low-confidence anchors, stale links, unreadable intent sources).",
            json!({}),
            &[],
        ),
        tool(
            "resonance_search",
            "Search requirements, decisions, code units, tests, annotations and files by text (case-insensitive), type and status. Status may be a link status (asserted, verified, stale, orphaned) or a node status (active, retired, attached, removed).",
            json!({
                "text": s("Text to look for; empty lists everything that matches the filters."),
                "type": s("Requirement, Decision, CodeUnit, Test, Annotation, File or Evidence."),
                "status": s("A link or node status."),
                "limit": i("Maximum results (default 50)."),
            }),
            &[],
        ),
        tool(
            "resonance_show",
            "One node or link in full, with its links and annotations.",
            json!({"ref": s(reference)}),
            &["ref"],
        ),
        tool(
            "resonance_trace",
            "From a requirement: the code units that satisfy it, the tests that verify it, each link's status and latest test evidence, and the decisions that constrain it.",
            json!({"requirement": s("A requirement key or ID.")}),
            &["requirement"],
        ),
        tool(
            "resonance_why",
            "Why a code unit, test or file exists: the requirements it satisfies, the decisions that constrain it, the tests that cover it, and the annotations on it and on the units that contain it. Call before changing code, and honor the annotations.",
            json!({"ref": s(reference)}),
            &["ref"],
        ),
        tool(
            "resonance_context",
            "A context pack for a task: the requirements, related decisions, linked code units (as locations), linked tests with latest evidence, annotations in scope, and call-outs for the task label.",
            json!({
                "requirements": {"type": "array", "items": {"type": "string"}, "description": "Requirement keys or IDs."},
                "task": s("A task label; call-outs made for this task are included."),
            }),
            &["requirements"],
        ),
        tool(
            "resonance_unverified",
            "Active requirements with no verified test.",
            json!({}),
            &[],
        ),
        tool(
            "resonance_repair_list",
            "Things that need an agent's judgment: orphaned units and links, low-confidence re-anchors, stale links, unreadable intent sources, retired requirements that still have links.",
            json!({}),
            &[],
        ),
        tool(
            "resonance_create_requirement",
            "Create a requirement.",
            json!({
                "key": s("A key such as FR-300 or NFR-12."),
                "text": s("The requirement."),
                "kind": s("functional or non-functional (default from the key)."),
            }),
            &["key", "text"],
        ),
        tool(
            "resonance_create_decision",
            "Record a decision and its rationale.",
            json!({
                "title": s("Short title."),
                "decision": s("What was decided."),
                "context": s("Why it came up."),
                "consequences": s("What follows from it."),
            }),
            &["title", "decision"],
        ),
        tool(
            "resonance_declare_unit",
            "Declare a code unit: a named region of a file (give lines or an exact snippet). Resonance does not parse code; declare the code you write or need to talk about.",
            region_props(vec![
                (
                    "name",
                    s("The unit's name, for example a function or type name."),
                ),
                (
                    "kind",
                    s("Free text: function, type, block, ... (default block)."),
                ),
            ]),
            &["path", "name"],
        ),
        tool(
            "resonance_declare_test",
            "Declare a test: a named region of a file plus the exact shell command that runs this test alone (exit code 0 means pass).",
            region_props(vec![
                ("name", s("The test's name.")),
                (
                    "run",
                    s("The exact command that runs this test alone, from the repository root."),
                ),
                ("kind", s("Free text (default test).")),
            ]),
            &["path", "name", "run"],
        ),
        tool(
            "resonance_list_units",
            "Declared code units and tests, optionally under a file or directory.",
            json!({"path": s("A file or directory, repository-relative.")}),
            &[],
        ),
        tool(
            "resonance_edit_unit",
            "Re-anchor a code unit or test to a new region (path and lines, or match), rename it, or change its kind or run command. With no changes, confirms a low-confidence anchor as correct.",
            region_props(vec![
                ("ref", s(reference)),
                ("name", s("New name.")),
                ("kind", s("New kind.")),
                ("run", s("New run command (tests only).")),
            ]),
            &["ref"],
        ),
        tool(
            "resonance_link",
            "Link two nodes: satisfies (code unit, test or file -> requirement), verifies (test -> requirement or code unit), constrains (decision -> anything). Agent links need a confidence and start as asserted.",
            json!({
                "source": s(reference),
                "type": s("satisfies, verifies or constrains."),
                "target": s(reference),
                "confidence": {"type": "number", "minimum": 0, "maximum": 1, "description": "How sure you are that the link holds."},
            }),
            &["source", "type", "target", "confidence"],
        ),
        tool(
            "resonance_unlink",
            "Remove a link (kept for history).",
            json!({"link": s("A link ID.")}),
            &["link"],
        ),
        tool(
            "resonance_confirm",
            "Return a stale link to asserted after checking that it still holds.",
            json!({"link": s("A link ID.")}),
            &["link"],
        ),
        tool(
            "resonance_annotate",
            "Attach an annotation to a code unit, file or other node: note, warning, call-out (optionally for a task label) or instruction.",
            json!({
                "target": s(reference),
                "kind": s("note, warning, call-out or instruction."),
                "text": s("The annotation."),
                "task": s("For call-outs: a task label."),
            }),
            &["target", "kind", "text"],
        ),
        tool(
            "resonance_reattach",
            "Re-attach an orphaned or wrongly attached link or annotation to a new target.",
            json!({"ref": s("A link ID or annotation ID."), "target": s(reference)}),
            &["ref", "target"],
        ),
        tool(
            "resonance_test",
            "Run the tests linked to a requirement or code unit (or one test, or one verifies link) and record evidence. Passing tests mark their verifies links verified.",
            json!({"ref": s(reference)}),
            &["ref"],
        ),
        tool(
            "resonance_split_unit",
            "Declare that a code unit became several (for example a function split in two). Declare the new units first; their links and annotations are copied from the old unit, which is retired.",
            json!({
                "ref": s(reference),
                "into": {"type": "array", "items": {"type": "string"}, "description": "The new units."},
            }),
            &["ref", "into"],
        ),
        tool(
            "resonance_merge_unit",
            "Declare that several code units became one. Their links and annotations move to the remaining unit, and they are retired.",
            json!({
                "refs": {"type": "array", "items": {"type": "string"}, "description": "The units that were merged away."},
                "into": s(reference),
            }),
            &["refs", "into"],
        ),
    ]
}

fn arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn req<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    arg(args, key).ok_or_else(|| Error::Usage(format!("missing required argument `{key}`")))
}

fn owned(args: &Value, key: &str) -> Option<String> {
    arg(args, key).map(String::from)
}

/// A region from `path` plus `startLine`/`endLine`, or just `path` when matching a snippet.
fn region(args: &Value) -> Result<Option<String>> {
    let Some(path) = arg(args, "path") else {
        return Ok(None);
    };
    let start = args.get("startLine").and_then(Value::as_u64);
    let end = args.get("endLine").and_then(Value::as_u64);
    Ok(Some(match (start, end) {
        (Some(s), Some(e)) => format!("{path}:{s}-{e}"),
        (Some(s), None) => format!("{path}:{s}"),
        (None, Some(_)) => return Err(Error::Usage("endLine needs startLine".into())),
        (None, None) => path.to_string(),
    }))
}

fn strings(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// Translate a tool call into the operation it mirrors (FR-254).
pub fn to_call(name: &str, args: &Value) -> Result<Call> {
    let (method, a) = match name {
        "resonance_status" => ("status", json!({})),
        "resonance_search" => (
            "search",
            json!({
                "text": arg(args, "text").unwrap_or(""),
                "type": arg(args, "type"),
                "status": arg(args, "status"),
                "limit": args.get("limit").and_then(Value::as_u64),
            }),
        ),
        "resonance_show" => ("show", json!({"ref": req(args, "ref")?})),
        "resonance_trace" => ("trace", json!({"ref": req(args, "requirement")?})),
        "resonance_why" => ("why", json!({"ref": req(args, "ref")?})),
        "resonance_context" => (
            "context",
            json!({"refs": strings(args, "requirements"), "task": arg(args, "task")}),
        ),
        "resonance_unverified" => ("unverified", json!({})),
        "resonance_repair_list" => ("repair_list", json!({})),
        "resonance_create_requirement" => (
            "req_add",
            json!({"key": req(args, "key")?, "text": req(args, "text")?, "kind": arg(args, "kind")}),
        ),
        "resonance_create_decision" => (
            "decision_add",
            json!({
                "title": owned(args, "title"),
                "decision": owned(args, "decision"),
                "context": owned(args, "context"),
                "consequences": owned(args, "consequences"),
            }),
        ),
        "resonance_declare_unit" | "resonance_declare_test" => {
            let test = name == "resonance_declare_test";
            if test {
                req(args, "run")?;
            }
            (
                "unit_add",
                json!({
                    "region": region(args)?.ok_or_else(|| Error::Usage("missing required argument `path`".into()))?,
                    "match": owned(args, "match"),
                    "name": req(args, "name")?,
                    "kind": owned(args, "kind"),
                    "run": owned(args, "run"),
                    "test": test,
                }),
            )
        }
        "resonance_list_units" => ("unit_list", json!({"path": arg(args, "path")})),
        "resonance_edit_unit" => (
            "unit_edit",
            json!({
                "ref": req(args, "ref")?,
                "region": region(args)?,
                "match": owned(args, "match"),
                "name": owned(args, "name"),
                "kind": owned(args, "kind"),
                "run": owned(args, "run"),
            }),
        ),
        "resonance_link" => (
            "link",
            json!({
                "src": req(args, "source")?,
                "type": req(args, "type")?,
                "dst": req(args, "target")?,
                "confidence": args.get("confidence").and_then(Value::as_f64),
            }),
        ),
        "resonance_unlink" => ("unlink", json!({"ref": req(args, "link")?})),
        "resonance_confirm" => ("confirm", json!({"ref": req(args, "link")?})),
        "resonance_annotate" => (
            "annotate",
            json!({
                "target": req(args, "target")?,
                "kind": req(args, "kind")?,
                "text": req(args, "text")?,
                "task": arg(args, "task"),
            }),
        ),
        "resonance_reattach" => (
            "reattach",
            json!({"ref": req(args, "ref")?, "target": req(args, "target")?}),
        ),
        "resonance_test" => ("test", json!({"ref": req(args, "ref")?})),
        "resonance_split_unit" => (
            "unit_split",
            json!({"ref": req(args, "ref")?, "into": strings(args, "into")}),
        ),
        "resonance_merge_unit" => (
            "unit_merge",
            json!({"refs": strings(args, "refs"), "into": req(args, "into")?}),
        ),
        other => return Err(Error::NotFound(format!("the tool `{other}`"))),
    };
    Ok(Call::new(method, a))
}

/// Whether a tool name exists.
pub fn exists(name: &str) -> bool {
    definitions().iter().any(|d| d["name"] == name)
}
