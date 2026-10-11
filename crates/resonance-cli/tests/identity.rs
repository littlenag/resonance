//! Identity (B2, MVP-024): file moves by content similarity (FR-036), regions moved into
//! files that did not change, snippet anchors through edits on their own line, and declared
//! splits and merges (FR-033, §9.4).

mod common;

use common::{STORE, TestRepo};
use resonance_core::store::{GraphRead, GraphStore};
use resonance_store_sqlite::SqliteStore;
use serde_json::Value;

fn op_kinds(repo: &TestRepo) -> Vec<String> {
    let mut store = SqliteStore::open(&repo.path().join(".resonance/graph.db")).unwrap();
    store.migrate().unwrap();
    store
        .ops_after(0, 1_000_000)
        .unwrap()
        .into_iter()
        .map(|r| r.op.kind)
        .collect()
}

fn location(repo: &TestRepo, unit: &str) -> (String, u64, String) {
    let l = repo.json(&["show", unit])["location"].clone();
    (
        l["path"].as_str().unwrap().to_string(),
        l["startLine"].as_u64().unwrap(),
        l["state"].as_str().unwrap().to_string(),
    )
}

#[test]
fn a_file_renamed_and_edited_in_one_step_keeps_its_anchors_and_links() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.ok(&[
        "annotate",
        "src/store.rs",
        "--kind",
        "note",
        "--text",
        "the storage module",
    ]);
    repo.ok(&["link", "src/store.rs", "satisfies", "FR-003"]);
    // Rename and edit at once: exact search alone could not follow `open`, whose lines change.
    let edited = STORE
        .replace("use std::io;\n", "use std::io;\nuse std::fs;\n")
        .replace("conn.check();", "conn.check_all();");
    repo.remove("src/store.rs");
    repo.write("src/storage/sqlite.rs", &edited);

    let (path, start, state) = location(&repo, "open");
    assert_eq!(
        (path.as_str(), start, state.as_str()),
        ("src/storage/sqlite.rs", 5, "attached")
    );
    let quote_line = repo.json(&["why", "open"])["unit"]["address"].clone();
    assert_eq!(quote_line, "src/storage/sqlite.rs::open");
    // File-level links and annotations follow the file (FR-036).
    let file = repo.json(&["why", "src/storage/sqlite.rs"]);
    assert_eq!(file["annotations"][0]["text"], "the storage module");
    assert_eq!(file["requirements"][0]["requirement"]["key"], "FR-003");
    assert!(op_kinds(&repo).contains(&"file.move".to_string()));
    assert!(
        repo.json(&["status"])["repair"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_unrelated_new_file_is_not_mistaken_for_a_move() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.ok(&[
        "annotate",
        "src/store.rs",
        "--kind",
        "note",
        "--text",
        "the storage module",
    ]);
    repo.remove("src/store.rs");
    repo.write("src/other.rs", "pub fn unrelated() -> u32 {\n    42\n}\n");
    let repair = repo.json(&["status"])["repair"].clone();
    let kinds: Vec<&str> = repair
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"orphaned-annotation"), "{repair}");
    assert!(!op_kinds(&repo).contains(&"file.move".to_string()));
}

#[test]
fn a_region_moved_into_a_file_that_did_not_change_is_found() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let open_fn =
        &STORE[STORE.find("/// Opens the store.").unwrap()..STORE.find("pub fn close").unwrap()];
    // First the function is copied into another file (both copies exist for a while)...
    repo.write("src/open.rs", &format!("use super::*;\n\n{open_fn}"));
    assert_eq!(location(&repo, "open").0, "src/store.rs");
    // ...then removed from the original. src/open.rs does not change in this step.
    repo.write("src/store.rs", &STORE.replace(open_fn, ""));
    let (path, start, state) = location(&repo, "open");
    assert_eq!(
        (path.as_str(), start, state.as_str()),
        ("src/open.rs", 4, "attached")
    );
    assert!(op_kinds(&repo).contains(&"identity.move".to_string()));
}

#[test]
fn a_snippet_anchor_keeps_its_exact_columns() {
    let repo = TestRepo::with_project();
    repo.ok(&["import", "spec/requirements.md"]);
    repo.ok(&[
        "unit",
        "add",
        "src/store.rs",
        "--match",
        "connect(path)",
        "--name",
        "connect_call",
    ]);
    repo.replace(
        "src/store.rs",
        "let conn = connect(path);",
        "let connection = connect(&path)?;",
    );
    let export: Vec<Value> = repo
        .ok(&["export"])
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let quote = export
        .iter()
        .find(|o| o["payload"]["props"]["name"] == "connect_call")
        .map(|o| {
            o["payload"]["props"]["anchor"]["quote"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .unwrap();
    assert_eq!(quote, "connect(&path)");
}

#[test]
fn split_copies_links_and_annotations_to_the_new_units() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.ok(&[
        "annotate",
        "open",
        "--kind",
        "instruction",
        "--text",
        "keep the signature",
    ]);
    // `open` is split into two declared units.
    repo.ok(&[
        "unit",
        "add",
        "src/store.rs:4-6",
        "--name",
        "open_connect",
        "--kind",
        "function",
    ]);
    repo.ok(&[
        "unit",
        "add",
        "src/store.rs:7-9",
        "--name",
        "open_finish",
        "--kind",
        "function",
    ]);
    let change = repo.json(&[
        "unit",
        "split",
        "open",
        "--into",
        "open_connect",
        "open_finish",
    ]);
    assert_eq!(change["linksMoved"], 4, "{change}");
    assert_eq!(change["retired"][0]["removed"], true);
    for part in ["open_connect", "open_finish"] {
        let why = repo.json(&["why", part]);
        assert_eq!(
            why["requirements"][0]["requirement"]["key"], "FR-001",
            "{part}"
        );
        assert_eq!(why["requirements"][0]["link"]["status"], "asserted");
        assert_eq!(
            why["annotations"][0]["text"], "keep the signature",
            "{part}"
        );
    }
    let trace = repo.json(&["trace", "FR-001"]);
    let labels: Vec<&str> = trace["satisfiedBy"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["link"]["other"]["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        labels,
        ["src/store.rs::open_connect", "src/store.rs::open_finish"]
    );
    assert!(op_kinds(&repo).contains(&"identity.split".to_string()));
    // The old unit is retired, not orphaned: nothing to repair.
    assert!(
        repo.json(&["status"])["repair"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn merge_moves_links_to_the_remaining_unit() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.ok(&[
        "unit",
        "add",
        "src/store.rs:4-14",
        "--name",
        "store_fns",
        "--kind",
        "block",
    ]);
    let change = repo.json(&["unit", "merge", "open", "close", "--into", "store_fns"]);
    assert_eq!(change["retired"].as_array().unwrap().len(), 2);
    let why = repo.json(&["why", "store_fns"]);
    let keys: Vec<&str> = why["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["requirement"]["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["FR-001", "FR-002"]);
    assert!(op_kinds(&repo).contains(&"identity.merge".to_string()));
    let err = repo.fails(&["unit", "merge", "store_fns", "--into", "store_fns"], 2);
    assert!(err.contains("listed twice"), "{err}");
}
