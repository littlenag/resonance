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
            "Things that need an agent's judgment: orphaned units and links, low-confidence re-anchors, stale links (including derivedFrom links between items and the documents they came from), retired requirements that still have links.",
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
                    s(
                        "The exact command that runs this test alone, from the repository root. Without it, test.command from the configuration is used.",
                    ),
                ),
                ("kind", s("Free text (default test).")),
            ]),
            &["path", "name"],
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
            "resonance_remove_unit",
            "Remove a code unit or test that no longer exists. It is retired, never deleted, and its links and annotations become orphaned for re-attaching.",
            json!({"ref": s(reference)}),
            &["ref"],
        ),
        tool(
            "resonance_link",
            "Link two nodes: satisfies (code unit, test or file -> requirement), verifies (test -> requirement or code unit), constrains (decision or constraint -> anything), realizes (code unit -> design), derivedFrom (requirement -> requirement it refines), supersedes, or a namespaced extension type. Agent links need a confidence and start as asserted.",
            json!({
                "source": s(reference),
                "type": s("satisfies, verifies, constrains, realizes, derivedFrom, supersedes, or namespace:name."),
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
            json!({
                "link": s("A link ID."),
                "costTokens": i("Optional: tokens this repair cost you, for resonance_value."),
                "costSeconds": i("Optional: seconds this repair took."),
            }),
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
            json!({
                "ref": s("A link ID or annotation ID."),
                "target": s(reference),
                "costTokens": i("Optional: tokens this repair cost you, for resonance_value."),
                "costSeconds": i("Optional: seconds this repair took."),
            }),
            &["ref", "target"],
        ),
        tool(
            "resonance_import_items",
            "Bring spec items in from a document you have read (Resonance does not parse documents). Give the document's repository path and, per item, its type, key, fields and the region it came from (startLine/endLine, or an exact match). Call first with dryRun: true and show the developer the plan (what is created, updated or unchanged, any errors, and for an existing document link its linkStatus and whether refreshLink would confirm it) together with any reformatting of the document you propose; reformat first if agreed, then call again without dryRun. Each item is linked (derivedFrom) to a code unit of kind spec over its region, so later edits on either side make that link stale. Nothing is written unless every item is valid. Applying confirms a stale document link only when this call changed the item or its region and the item text agrees with the region.",
            json!({
                "document": s("The document, as a repository-relative path."),
                "items": {"type": "array", "description": "The items found in the document.", "items": {
                    "type": "object",
                    "properties": {
                        "type": s("Requirement, Decision, Design or Constraint."),
                        "key": s("The item's key, for example FR-042 or ADR-0007."),
                        "fields": {"type": "object", "additionalProperties": {"type": "string"}, "description": "Requirement: text, kind, priority, status. Decision: title, decision, context, consequences, status. Design: title, text, status. Constraint: title, text, scope, status."},
                        "region": {"type": "object", "properties": {
                            "startLine": {"type": "integer", "minimum": 1},
                            "endLine": {"type": "integer", "minimum": 1},
                            "match": s("Instead of lines: an exact snippet that occurs once in the document."),
                        }},
                    },
                    "required": ["type", "key", "region"],
                }},
                "dryRun": {"type": "boolean", "description": "Return the plan and write nothing."},
            }),
            &["document", "items"],
        ),
        tool(
            "resonance_show_item",
            "One spec item with every field, its status, link counts and the document region it came from, if any.",
            json!({"ref": s(reference)}),
            &["ref"],
        ),
        tool(
            "resonance_list_items",
            "Spec items (requirements, decisions, designs, constraints), filtered by type, status and text.",
            json!({
                "type": s("Requirement, Decision, Design or Constraint."),
                "status": s("draft, active, superseded, retired, accepted, ..."),
                "text": s("Text to look for in keys, titles and text."),
            }),
            &[],
        ),
        tool(
            "resonance_create_item",
            "Create a spec item. Requirements need key and fields.text; decisions fields.title and fields.decision; designs fields.title; constraints fields.text. Optional fields: kind, priority, status, scope, context, consequences.",
            json!({
                "type": s("Requirement, Decision, Design or Constraint."),
                "key": s("A key: FR-042 for requirements; optional for the others."),
                "fields": {"type": "object", "additionalProperties": {"type": "string"}, "description": "Field values by name."},
            }),
            &["type"],
        ),
        tool(
            "resonance_edit_item",
            "Change a spec item's fields. Links to it become stale, including the derivedFrom link to the document it came from, which tells you to update that document.",
            json!({
                "ref": s(reference),
                "fields": {"type": "object", "additionalProperties": {"type": "string"}, "description": "Field values by name."},
            }),
            &["ref", "fields"],
        ),
        tool(
            "resonance_retire_item",
            "Retire a spec item; it is never deleted.",
            json!({"ref": s(reference)}),
            &["ref"],
        ),
        tool(
            "resonance_supersede",
            "Record that a new spec item supersedes an old one; the old one becomes superseded and its links stale.",
            json!({"old": s(reference), "new": s(reference)}),
            &["old", "new"],
        ),
        tool(
            "resonance_create_extension",
            "Create a node of a namespaced extension type such as acme:Service (FR-007). Link it with a namespaced link type such as acme:deploys.",
            json!({
                "type": s("namespace:Name"),
                "name": s("The node's name."),
                "text": s("Optional description."),
            }),
            &["type", "name"],
        ),
        tool(
            "resonance_recover",
            "Divergences between git, the files and the graph (classified C1-C9 from git evidence), the detached pool and recovery records. action: list (default), undo <recovery id> (puts back what a resolution displaced), dismiss <divergence id>, attach <delta id> (detached delta onto HEAD), drop <delta id> (exports, then removes). Get the developer's confirmation before anything that cannot be undone.",
            json!({
                "action": s("list, undo, dismiss, attach or drop."),
                "id": s("The recovery record, divergence or delta ID (a unique prefix is enough)."),
                "note": s("For dismiss: what was done."),
            }),
            &[],
        ),
        tool(
            "resonance_conventions",
            "The conventions for this working tree (they depend on the adoption stage, on whether this developer has opted in, and on the Resonance version), the state of the agent kit, and the orientation to give the developer at the end of onboarding. Read them at the start of a session.",
            json!({}),
            &[],
        ),
        tool(
            "resonance_check",
            "Consistency checks with findings (code, severity, message, fix). Without a range: what is staged for the next commit (run before every commit and fix every error). With a range such as origin/main..HEAD: the commits of a branch (run before opening or updating a pull request).",
            json!({"range": s("A git range, for example origin/main..HEAD.")}),
            &[],
        ),
        tool(
            "resonance_review",
            "A summary of the graph changes in a git range (intent added, changed and retired; links; annotations; identity operations; conflicts; compactions), with a Markdown rendering to put in pull request descriptions.",
            json!({"range": s("A git range, for example origin/main..HEAD.")}),
            &["range"],
        ),
        tool(
            "resonance_compact",
            "Fold the sealed deltas into a new snapshot. Resonance compacts the trunk on its own; call this only when the developer asks. With commit, the compaction gets its own commit.",
            json!({
                "commit": {"type": "boolean", "description": "Commit the compaction on its own."},
                "anywhere": {"type": "boolean", "description": "Compact even off the trunk (only when the developer insists)."},
            }),
            &[],
        ),
        tool(
            "resonance_changes",
            "The change log: people's recent changes to spec items (requirements, decisions, designs, constraints) that no agent has handled yet. Keep, in your own memory, the seq of the last entry you read; pass it as `since` at the start of work and before git operations, act on each entry, then remove it with resonance_changes_done.",
            json!({"since": i("Only entries after this sequence number.")}),
            &[],
        ),
        tool(
            "resonance_changes_show",
            "One change-log entry in full, with the item's fields before and after.",
            json!({"seq": i("The entry's sequence number.")}),
            &["seq"],
        ),
        tool(
            "resonance_changes_done",
            "Remove handled change-log entries. This deletes them from the log only; the changes stay in the graph's history. The log should be empty before commits, pushes and pulls.",
            json!({
                "seqs": {"type": "array", "items": {"type": "integer"}, "description": "The sequence numbers handled."},
                "note": s("Optional: what was done about them."),
            }),
            &["seqs"],
        ),
        tool(
            "resonance_value",
            "What Resonance did for this developer, locally: traces, context packs, anchors that followed edits, repairs and their cost.",
            json!({}),
            &[],
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
        "resonance_confirm" => (
            "confirm",
            json!({"ref": req(args, "link")?, "costTokens": args.get("costTokens"), "costSeconds": args.get("costSeconds")}),
        ),
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
            json!({"ref": req(args, "ref")?, "target": req(args, "target")?, "costTokens": args.get("costTokens"), "costSeconds": args.get("costSeconds")}),
        ),
        "resonance_recover" => (
            "recover",
            json!({"action": arg(args, "action"), "id": arg(args, "id"), "note": arg(args, "note")}),
        ),
        "resonance_value" => ("value", json!({})),
        "resonance_import_items" => (
            "import_items",
            json!({
                "document": req(args, "document")?,
                "items": args.get("items").cloned().unwrap_or(json!([])),
                "dryRun": args.get("dryRun").and_then(Value::as_bool).unwrap_or(false),
            }),
        ),
        "resonance_show_item" => ("spec_show", json!({"ref": req(args, "ref")?})),
        "resonance_remove_unit" => ("unit_remove", json!({"ref": req(args, "ref")?})),
        "resonance_compact" => (
            "compact",
            json!({
                "commit": args.get("commit").and_then(Value::as_bool).unwrap_or(false),
                "anywhere": args.get("anywhere").and_then(Value::as_bool).unwrap_or(false),
            }),
        ),
        "resonance_conventions" => ("conventions", json!({})),
        "resonance_check" => ("check", json!({"range": arg(args, "range")})),
        "resonance_review" => ("review", json!({"range": req(args, "range")?})),
        "resonance_changes" => (
            "changes",
            json!({"since": args.get("since").and_then(Value::as_u64)}),
        ),
        "resonance_changes_show" => (
            "changes_show",
            json!({"seq": args.get("seq").and_then(Value::as_u64).ok_or_else(|| Error::Usage("give the entry's seq".into()))?}),
        ),
        "resonance_changes_done" => (
            "changes_done",
            json!({
                "seqs": args.get("seqs").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).collect::<Vec<_>>()).unwrap_or_default(),
                "note": arg(args, "note"),
            }),
        ),
        "resonance_list_items" => (
            "spec_list",
            json!({"type": arg(args, "type"), "status": arg(args, "status"), "text": arg(args, "text")}),
        ),
        "resonance_create_item" => (
            "spec_add",
            json!({"type": req(args, "type")?, "key": arg(args, "key"), "fields": args.get("fields").cloned().unwrap_or(json!({}))}),
        ),
        "resonance_edit_item" => (
            "spec_edit",
            json!({"ref": req(args, "ref")?, "fields": args.get("fields").cloned().unwrap_or(json!({}))}),
        ),
        "resonance_retire_item" => ("spec_retire", json!({"ref": req(args, "ref")?})),
        "resonance_supersede" => (
            "supersede",
            json!({"old": req(args, "old")?, "new": req(args, "new")?}),
        ),
        "resonance_create_extension" => (
            "ext_add",
            json!({"type": req(args, "type")?, "name": req(args, "name")?, "text": arg(args, "text")}),
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
