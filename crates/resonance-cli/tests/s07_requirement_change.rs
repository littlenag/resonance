//! S7: AC1 editing a requirement's text makes its links stale; AC2 `resonance_trace` lists
//! everything affected. Also the link between an item and its document (FR-124): either side
//! changing makes it stale, and the agent's import brings the item back in step.

mod common;

use common::TestRepo;
use serde_json::json;

fn statuses(repo: &TestRepo, key: &str) -> Vec<String> {
    let t = repo.tool("resonance_trace", json!({"requirement": key}));
    t["satisfiedBy"]
        .as_array()
        .unwrap()
        .iter()
        .chain(t["verifiedBy"].as_array().unwrap())
        .map(|e| {
            format!(
                "{} {}",
                e["link"]["type"].as_str().unwrap(),
                e["link"]["status"].as_str().unwrap()
            )
        })
        .collect()
}

#[test]
fn editing_requirement_text_makes_every_link_stale() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.tool("resonance_test", json!({"ref": "FR-001"}));
    assert_eq!(
        statuses(&repo, "FR-001"),
        ["satisfies asserted", "verifies verified"]
    );

    repo.tool(
        "resonance_edit_item",
        json!({"ref": "FR-001", "fields": {"text": "The store opens a database at a path, creating it if missing."}}),
    );
    let status = repo.json(&["status"]);
    let stale: Vec<&serde_json::Value> = status["repair"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "stale-link")
        .collect();
    // The code, the test, and the link to the document it came from.
    assert_eq!(stale.len(), 3, "AC1: {status}");
    assert_eq!(
        statuses(&repo, "FR-001"),
        ["satisfies stale", "verifies stale"],
        "AC2: trace lists everything affected"
    );
    assert_eq!(
        statuses(&repo, "FR-002"),
        ["satisfies asserted"],
        "other requirements unaffected"
    );

    // Confirming and re-testing return the links to health.
    let satisfies = repo.tool("resonance_trace", json!({"requirement": "FR-001"}))["satisfiedBy"]
        [0]["link"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        repo.tool("resonance_confirm", json!({"link": &satisfies}))["link"]["status"],
        "asserted"
    );
    repo.tool("resonance_test", json!({"ref": "FR-001"}));
    assert_eq!(
        statuses(&repo, "FR-001"),
        ["satisfies asserted", "verifies verified"]
    );
}

fn source_link_status(repo: &TestRepo, key: &str) -> String {
    let show = repo.tool("resonance_show", json!({"ref": key}));
    show["links"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["type"] == "derivedFrom")
        .map(|l| l["status"].as_str().unwrap().to_string())
        .unwrap_or_else(|| panic!("no source link: {show}"))
}

#[test]
fn a_document_edit_marks_the_item_and_the_agent_applies_it() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    assert_eq!(source_link_status(&repo, "FR-001"), "asserted");
    repo.replace(
        "spec/requirements.md",
        "The store opens a database at a path.",
        "The store opens a database at a path, creating it if missing.",
    );
    // The graph is the source of truth: the item is unchanged, but its source link is stale.
    assert_eq!(source_link_status(&repo, "FR-001"), "stale");
    let item = repo.tool("resonance_show_item", json!({"ref": "FR-001"}));
    assert_eq!(item["text"], "The store opens a database at a path.");
    assert_eq!(
        statuses(&repo, "FR-001"),
        ["satisfies asserted", "verifies asserted"]
    );

    // The agent shows the developer the difference, then applies it.
    let item = json!({"type": "Requirement", "key": "FR-001",
        "fields": {"text": "The store opens a database at a path, creating it if missing."},
        "region": {"startLine": 3}});
    let plan = repo.tool(
        "resonance_import_items",
        json!({"document": "spec/requirements.md", "items": [item.clone()], "dryRun": true}),
    );
    assert_eq!(plan["items"][0]["action"], "update", "{plan}");
    assert_eq!(plan["items"][0]["fields"], json!(["text"]));
    repo.tool(
        "resonance_import_items",
        json!({"document": "spec/requirements.md", "items": [item]}),
    );
    assert_eq!(source_link_status(&repo, "FR-001"), "asserted");
    assert_eq!(
        statuses(&repo, "FR-001"),
        ["satisfies stale", "verifies stale"]
    );
}

#[test]
fn retired_requirements_with_links_are_listed() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let retired = repo.tool("resonance_retire_item", json!({"ref": "FR-002"}));
    assert_eq!(retired["item"]["status"], "retired", "never deleted");
    let repair = repo.json(&["status"]);
    let kinds: Vec<String> = repair["repair"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap().to_string())
        .collect();
    assert!(
        kinds.contains(&"retired-requirement-with-links".to_string()),
        "{kinds:?}"
    );
}

#[test]
fn every_item_is_edited_through_the_graph() {
    let repo = TestRepo::with_project();
    repo.import_sample();
    // An item that came from a document is edited in the graph like any other (FR-272).
    repo.tool(
        "resonance_edit_item",
        json!({"ref": "FR-001", "fields": {"priority": "high"}}),
    );
    assert_eq!(
        repo.tool("resonance_show_item", json!({"ref": "FR-001"}))["priority"],
        "high"
    );
    repo.tool(
        "resonance_create_requirement",
        json!({"key": "FR-100", "text": "Exports are engine-neutral."}),
    );
    repo.tool(
        "resonance_declare_unit",
        json!({"path": "src/store.rs", "startLine": 11, "endLine": 14, "name": "close"}),
    );
    repo.link("close", "satisfies", "FR-100");
    repo.tool(
        "resonance_edit_item",
        json!({"ref": "FR-100", "fields": {"text": "Exports are engine-neutral JSON Lines."}}),
    );
    assert_eq!(statuses(&repo, "FR-100"), ["satisfies stale"]);
    assert_eq!(
        repo.tool("resonance_retire_item", json!({"ref": "FR-100"}))["item"]["status"],
        "retired"
    );
    repo.tool_err(
        "resonance_create_requirement",
        json!({"key": "FR-100", "text": "again"}),
    );
    repo.tool_err(
        "resonance_create_requirement",
        json!({"key": "not-a-key", "text": "x"}),
    );
}
