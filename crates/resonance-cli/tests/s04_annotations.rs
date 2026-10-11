//! S4: AC1 annotations attach to code units, AC2 they survive edits elsewhere, edits inside
//! the region, file renames and moves (a region rewritten beyond recognition is orphaned and
//! listed, never silently re-attached), AC3 orphaned with last location and restored when the
//! text reappears (MVP-005), AC4 annotations appear in `why` and in context packs.

mod common;

use common::{STORE, TestRepo};
use serde_json::Value;

fn annotated() -> TestRepo {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.ok(&[
        "annotate",
        "open",
        "--kind",
        "instruction",
        "--text",
        "do not change the open() signature",
    ]);
    repo
}

fn annotation_on_open(repo: &TestRepo) -> Value {
    let why = repo.json(&["why", "open"]);
    let list = why["annotations"].as_array().unwrap();
    assert_eq!(list.len(), 1, "{why}");
    list[0].clone()
}

fn location(repo: &TestRepo, unit: &str) -> (String, u64, u64, String) {
    let show = repo.json(&["show", unit]);
    let l = &show["location"];
    (
        l["path"].as_str().unwrap().to_string(),
        l["startLine"].as_u64().unwrap(),
        l["endLine"].as_u64().unwrap(),
        l["state"].as_str().unwrap().to_string(),
    )
}

#[test]
fn an_annotation_is_attached_to_the_unit_not_a_line() {
    let repo = annotated();
    let a = annotation_on_open(&repo);
    assert_eq!(a["kind"], "instruction");
    assert_eq!(a["on"]["label"], "src/store.rs::open", "AC1");
    assert_eq!(a["on"]["type"], "CodeUnit");
}

#[test]
fn it_survives_edits_elsewhere_inside_renames_and_moves() {
    let repo = annotated();

    // Edits elsewhere in the file.
    repo.replace(
        "src/store.rs",
        "use std::io;\n",
        "use std::io;\nuse std::fs;\nuse std::path::Path;\n",
    );
    assert_eq!(
        location(&repo, "open"),
        ("src/store.rs".into(), 6, 11, "attached".into())
    );

    // Edits inside the region that keep at least half its lines.
    repo.replace(
        "src/store.rs",
        "    conn.check();\n",
        "    conn.check();\n    conn.vacuum();\n",
    );
    assert_eq!(
        location(&repo, "open"),
        ("src/store.rs".into(), 6, 12, "attached".into())
    );
    assert_eq!(annotation_on_open(&repo)["linkStatus"], "asserted");

    // A file rename: the unit's address follows the file.
    repo.rename("src/store.rs", "src/db/store.rs");
    assert_eq!(
        location(&repo, "open"),
        ("src/db/store.rs".into(), 6, 12, "attached".into())
    );
    assert_eq!(
        annotation_on_open(&repo)["on"]["label"],
        "src/db/store.rs::open"
    );

    // The region moves to another file.
    let text = repo.read("src/db/store.rs");
    let start = text.find("/// Opens the store.").unwrap();
    let end = text.find("pub fn close").unwrap();
    repo.write(
        "src/db/open.rs",
        &format!("use super::*;\n\n{}", &text[start..end]),
    );
    repo.write(
        "src/db/store.rs",
        &format!("{}{}", &text[..start], &text[end..]),
    );
    let (path, _, _, state) = location(&repo, "open");
    assert_eq!(
        (path.as_str(), state.as_str()),
        ("src/db/open.rs", "attached")
    );
    let a = annotation_on_open(&repo);
    assert_eq!(a["on"]["label"], "src/db/open.rs::open");
    assert_eq!(a["linkStatus"], "asserted");
    assert!(
        repo.json(&["status"])["repair"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_region_rewritten_beyond_recognition_is_orphaned_and_listed() {
    let repo = annotated();
    let open_fn =
        &STORE[STORE.find("pub fn open").unwrap()..STORE.find("\n\npub fn close").unwrap()];
    repo.replace(
        "src/store.rs",
        open_fn,
        "pub async fn open_pool(cfg: Config) -> Result<Pool, Error> {\n    let pool = Pool::builder().max_size(cfg.size).build().await?;\n    Ok(pool)\n}",
    );
    let (_, start, end, state) = location(&repo, "open");
    assert_eq!(state, "orphaned", "never silently re-attached");
    assert_eq!(
        (start, end),
        (4, 9),
        "keeps its last known location (FR-034)"
    );
    let repair = repo.json(&["status"])["repair"].clone();
    let kinds: Vec<&str> = repair
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"orphaned-unit"), "{repair}");
    assert!(kinds.contains(&"orphaned-annotation"), "{repair}");
    let unit_item = repair
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "orphaned-unit")
        .unwrap();
    assert!(unit_item["quote"].as_str().unwrap().contains("pub fn open"));
    assert_eq!(unit_item["location"]["path"], "src/store.rs");

    // The agent re-anchors it to the new function; the annotation comes back.
    repo.ok(&["unit", "edit", "open", "src/store.rs:4-7"]);
    let a = annotation_on_open(&repo);
    assert_eq!(a["linkStatus"], "asserted");
    assert!(
        repo.json(&["status"])["repair"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn orphaned_when_the_anchor_disappears_and_restored_when_it_reappears() {
    let repo = annotated();
    repo.git(&["checkout", "-q", "-b", "rewrite"]);
    let open_fn =
        &STORE[STORE.find("/// Opens the store.").unwrap()..STORE.find("pub fn close").unwrap()];
    repo.replace("src/store.rs", open_fn, "");
    repo.commit_all("drop open");
    let (path, start, end, state) = location(&repo, "open");
    assert_eq!(
        (path.as_str(), start, end, state.as_str()),
        ("src/store.rs", 4, 9, "orphaned"),
        "AC3"
    );
    assert_eq!(annotation_on_open(&repo)["linkStatus"], "orphaned");
    let trace = repo.json(&["trace", "FR-001"]);
    assert_eq!(trace["satisfiedBy"][0]["link"]["status"], "orphaned");

    // Switching back brings the text back with its context: the anchor is restored (MVP-005).
    repo.git(&["checkout", "-q", "main"]);
    assert_eq!(
        location(&repo, "open"),
        ("src/store.rs".into(), 4, 9, "attached".into())
    );
    assert_eq!(annotation_on_open(&repo)["linkStatus"], "asserted");
    let trace = repo.json(&["trace", "FR-001"]);
    assert_eq!(
        trace["satisfiedBy"][0]["link"]["status"], "asserted",
        "links return to their earlier status"
    );
}

#[test]
fn annotations_reach_agents_through_why_and_context_packs() {
    let repo = annotated();
    repo.ok(&[
        "annotate",
        "close",
        "--kind",
        "call-out",
        "--text",
        "flush order matters here",
        "--task",
        "fix-shutdown",
    ]);
    let pack = repo.json(&["context", "FR-001", "FR-002", "--task", "fix-shutdown"]);
    let texts: Vec<&str> = pack["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["text"].as_str().unwrap())
        .collect();
    assert!(
        texts.contains(&"do not change the open() signature"),
        "AC4: {pack}"
    );
    assert_eq!(pack["callOuts"][0]["text"], "flush order matters here");
    assert_eq!(pack["callOuts"][0]["task"], "fix-shutdown");
    let units: Vec<&str> = pack["codeUnits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["address"].as_str().unwrap())
        .collect();
    assert_eq!(
        units,
        ["src/store.rs::open", "src/store.rs::close"],
        "in file order"
    );
    assert!(
        pack["codeUnits"][0].get("quote").is_none(),
        "locations, not full text (FR-133)"
    );
    assert_eq!(pack["tests"][0]["address"], "tests/store_test.sh::opens_db");
    let err = repo.fails(
        &[
            "annotate", "open", "--kind", "note", "--text", "x", "--task", "t",
        ],
        2,
    );
    assert!(err.contains("call-out"), "{err}");
}

#[test]
fn orphaned_annotations_can_be_reattached() {
    let repo = annotated();
    repo.ok(&["unit", "remove", "open"]);
    let a = annotation_on_open_by_search(&repo);
    let id = a["id"].as_str().unwrap().to_string();
    let reattached = repo.json(&["reattach", &id, "close"]);
    assert_eq!(reattached["status"], "asserted");
    let why = repo.json(&["why", "close"]);
    assert_eq!(
        why["annotations"][0]["text"],
        "do not change the open() signature"
    );
}

fn annotation_on_open_by_search(repo: &TestRepo) -> Value {
    let hits = repo.json(&["search", "open() signature", "--type", "Annotation"]);
    hits[0].clone()
}
