//! Intent breadth (B4) and the agent's import of spec documents (S26, MVP-042), through MCP as
//! agents do it: designs, constraints, supersede, derivedFrom and realizes, extension types,
//! and `resonance_import_items` with its dry run, validation and links back to the document.

mod common;

use common::TestRepo;
use serde_json::{Value, json};

fn keys(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["key"].as_str().map(String::from))
        .collect()
}

#[test]
fn designs_constraints_and_their_links() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let d = repo.tool(
        "resonance_create_item",
        json!({"type": "Design", "key": "DES-1", "fields": {"title": "Storage layer", "text": "One file per store"}}),
    );
    assert_eq!(d["type"], "Design");
    assert_eq!(d["status"], "active");
    let c = repo.tool(
        "resonance_create_item",
        json!({"type": "Constraint", "key": "CON-1", "fields": {"text": "Opening takes under 10 ms", "scope": "src/store.rs"}}),
    );
    assert_eq!(c["props"]["scope"], "src/store.rs");

    repo.link("open", "realizes", "DES-1");
    repo.link("CON-1", "constrains", "open");
    repo.tool(
        "resonance_create_item",
        json!({"type": "Requirement", "key": "FR-010", "fields": {"text": "Opening is atomic", "kind": "non-functional", "status": "draft"}}),
    );
    repo.link("FR-010", "derivedFrom", "FR-001");
    let why = repo.tool("resonance_show", json!({"ref": "open"}));
    let text = why.to_string();
    assert!(
        text.contains("realizes") && text.contains("constrained by"),
        "{why}"
    );
    let r = repo.tool("resonance_show_item", json!({"ref": "FR-010"}));
    assert_eq!(r["kind"], "non-functional");
    assert_eq!(r["status"], "draft");

    // A design cannot satisfy a requirement; the error says what can.
    let err = repo.tool_err(
        "resonance_link",
        json!({"source": "DES-1", "type": "satisfies", "target": "FR-001", "confidence": 0.9}),
    );
    assert!(err.contains("satisfies a requirement"), "{err}");

    // Supersede: the old one is superseded and its links go stale (S7).
    repo.tool(
        "resonance_create_requirement",
        json!({"key": "FR-011", "text": "The store opens a database, or creates one"}),
    );
    repo.tool("resonance_test", json!({"ref": "FR-001"}));
    repo.tool(
        "resonance_supersede",
        json!({"old": "FR-001", "new": "FR-011"}),
    );
    let old = repo.tool("resonance_show_item", json!({"ref": "FR-001"}));
    assert_eq!(old["status"], "superseded");
    let trace = repo.tool("resonance_trace", json!({"requirement": "FR-001"}));
    assert_eq!(trace["verifiedBy"][0]["link"]["status"], "stale", "{trace}");

    // Edit and retire: retired, never deleted (FR-271).
    repo.tool(
        "resonance_edit_item",
        json!({"ref": "DES-1", "fields": {"text": "Two files per store"}}),
    );
    repo.tool("resonance_retire_item", json!({"ref": "DES-1"}));
    let list = repo.tool("resonance_list_items", json!({"type": "Design"}));
    assert_eq!(list[0]["status"], "retired");
    let active = repo.tool("resonance_list_items", json!({"status": "active"}));
    assert!(!keys(&active).contains(&"DES-1".to_string()));
}

#[test]
fn extension_types_survive_export_restore_and_rebuild() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let svc = repo.tool(
        "resonance_create_extension",
        json!({"type": "acme:Service", "name": "billing", "text": "Bills customers"}),
    );
    let id = svc["node"]["id"].as_str().unwrap().to_string();
    assert_eq!(svc["properties"]["extType"], "acme:Service");
    repo.link("open", "acme:runsIn", &id);
    let bad = repo.tool_err(
        "resonance_create_extension",
        json!({"type": "Service", "name": "x"}),
    );
    assert!(bad.contains("namespace:Name"), "{bad}");

    repo.write("NOTES.md", "x\n");
    repo.commit_all("seal");
    repo.ok(&["rebuild"]);
    let show = repo.tool("resonance_show", json!({"ref": &id}));
    assert_eq!(show["properties"]["name"], "billing");
    assert!(show.to_string().contains("acme:runsIn"), "{show}");

    let export = repo.path().join("..").join("ext-export.jsonl");
    repo.ok(&["export", "--out", export.to_str().unwrap()]);
    let other = TestRepo::with_project();
    other.ok(&["restore", export.to_str().unwrap()]);
    let found = other.tool("resonance_search", json!({"text": "billing"}));
    assert_eq!(found[0]["type"], "Extension", "{found}");
    let _ = std::fs::remove_file(export);
}

const ADR: &str =
    "# 1. Use SQLite\n\n## Status\n\nAccepted\n\n## Decision\n\nUse SQLite behind a store API.\n";

fn adr_item(decision: &str) -> Value {
    json!({"type": "Decision", "key": "ADR-0001",
        "fields": {"title": "Use SQLite", "decision": decision, "status": "accepted"},
        "region": {"match": "Use SQLite behind a store API."}})
}

#[test]
fn the_agent_previews_then_brings_items_in_linked_to_their_regions() {
    let repo = TestRepo::with_project();
    repo.write("docs/adr/0001-use-sqlite.md", ADR);
    repo.commit_all("adr");
    let reqs = json!([
        {"type": "Requirement", "key": "FR-001", "fields": {"text": "The store opens a database at a path."}, "region": {"startLine": 3}},
        {"type": "Requirement", "key": "FR-002", "fields": {"text": "The store closes cleanly, flushing pending writes.", "status": "draft"}, "region": {"startLine": 4}},
    ]);

    // The preview writes nothing.
    let plan = repo.tool(
        "resonance_import_items",
        json!({"document": "spec/requirements.md", "items": reqs, "dryRun": true}),
    );
    assert_eq!(plan["applied"], false);
    assert_eq!(plan["items"][0]["action"], "create");
    assert_eq!(plan["items"][0]["lines"], json!([3, 3]));
    assert_eq!(repo.json(&["status"])["counts"]["requirements"], 0);

    // One bad item: nothing is applied, and the plan says why.
    let mut bad = reqs.as_array().unwrap().clone();
    bad.push(json!({"type": "Requirement", "key": "FR-003", "fields": {"wat": "x", "text": "t"}, "region": {"startLine": 5}}));
    bad.push(json!({"type": "Requirement", "key": "FR-009", "fields": {"text": "t"}, "region": {"match": "no such text"}}));
    let plan = repo.tool(
        "resonance_import_items",
        json!({"document": "spec/requirements.md", "items": bad}),
    );
    assert_eq!(plan["applied"], false);
    assert_eq!(plan["errors"], 2, "{plan}");
    assert!(
        plan["items"][2]["error"].as_str().unwrap().contains("wat"),
        "{plan}"
    );
    assert_eq!(repo.json(&["status"])["counts"]["requirements"], 0);

    // Applying: items, their `spec` units and derivedFrom links.
    let done = repo.tool(
        "resonance_import_items",
        json!({"document": "spec/requirements.md", "items": reqs}),
    );
    assert_eq!(done["applied"], true, "{done}");
    assert_eq!(done["items"][0]["unit"], "spec/requirements.md::FR-001");
    let item = repo.tool("resonance_show_item", json!({"ref": "FR-002"}));
    assert_eq!(item["status"], "draft");
    assert_eq!(item["source"]["path"], "spec/requirements.md");
    assert_eq!(item["source"]["line"], 4);
    let units = repo.tool(
        "resonance_list_units",
        json!({"path": "spec/requirements.md"}),
    );
    assert_eq!(units.as_array().unwrap().len(), 2, "{units}");
    assert_eq!(units[0]["kind"], "spec");

    // Decisions keep their keys, with deterministic IDs (FR-186); a match finds the region.
    repo.tool(
        "resonance_import_items",
        json!({"document": "docs/adr/0001-use-sqlite.md", "items": [adr_item("Use SQLite behind a store API.")]}),
    );
    let adr = repo.tool("resonance_show_item", json!({"ref": "ADR-0001"}));
    assert_eq!(adr["id"], "dec:ADR-0001");
    assert_eq!(adr["status"], "accepted");
    assert_eq!(adr["source"]["line"], 9);

    // Calling again: unchanged items stay so; a changed one is updated.
    let again = repo.tool(
        "resonance_import_items",
        json!({"document": "docs/adr/0001-use-sqlite.md", "items": [adr_item("Use SQLite behind a store API, in WAL mode.")], "dryRun": true}),
    );
    assert_eq!(again["items"][0]["action"], "update");
    assert_eq!(again["items"][0]["fields"], json!(["decision"]));
    let same = repo.tool(
        "resonance_import_items",
        json!({"document": "spec/requirements.md", "items": reqs, "dryRun": true}),
    );
    assert_eq!(same["items"][0]["action"], "unchanged", "{same}");

    // A key used by an item of another type is refused.
    let clash = repo.tool(
        "resonance_import_items",
        json!({"document": "spec/requirements.md", "dryRun": true, "items": [
            {"type": "Design", "key": "FR-001", "fields": {"title": "x"}, "region": {"startLine": 3}}]}),
    );
    assert!(
        clash["items"][0]["error"]
            .as_str()
            .unwrap()
            .contains("already a Requirement"),
        "{clash}"
    );

    // Nothing here was written by Resonance into the documents.
    assert_eq!(repo.git_status(), "");
}

#[test]
fn a_blank_update_is_rejected_before_anything_is_written() {
    let repo = TestRepo::with_project();
    repo.import_sample();
    let before = repo.json(&["status"])["counts"]["requirements"].clone();
    let items = json!([
        {"type": "Requirement", "key": "FR-050", "fields": {"text": "A new requirement."},
            "region": {"startLine": 3, "endLine": 3}},
        {"type": "Requirement", "key": "FR-001", "fields": {"text": "  "},
            "region": {"startLine": 3, "endLine": 3}},
    ]);
    let dry = repo.tool(
        "resonance_import_items",
        json!({"document": "spec/requirements.md", "items": items, "dryRun": true}),
    );
    assert_eq!(dry["applied"], false, "{dry}");
    assert!(dry["errors"].as_u64().unwrap() >= 1, "{dry}");
    assert!(
        dry["items"][1]["error"]
            .as_str()
            .unwrap()
            .contains("must not be empty"),
        "{dry}"
    );
    let applied = repo.tool(
        "resonance_import_items",
        json!({"document": "spec/requirements.md", "items": items}),
    );
    assert_eq!(applied["applied"], false, "{applied}");
    assert_eq!(repo.json(&["status"])["counts"]["requirements"], before);
    let missing = repo.tool_err("resonance_show_item", json!({"ref": "FR-050"}));
    assert!(
        missing.contains("FR-050") || missing.contains("not"),
        "{missing}"
    );
}
