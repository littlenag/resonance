//! S8 (adapted): a file save followed by any command reflects the new text and re-located
//! anchors. S13 (adapted): edits made between commands are always reflected in the next
//! answer. Also the operations recorded by MVP-013 steps (identity.move, identity.retarget).

mod common;

use common::{STORE, TestRepo};
use resonance_core::store::{GraphRead, GraphStore};
use resonance_store_sqlite::SqliteStore;
use serde_json::json;

/// The kinds of every operation in the log, in order.
fn op_kinds(repo: &TestRepo) -> Vec<String> {
    let mut store = SqliteStore::open(&repo.path().join(".resonance/local/state.db")).unwrap();
    store.migrate().unwrap();
    store
        .ops_after(0, 1_000_000)
        .unwrap()
        .into_iter()
        .map(|r| r.op.kind)
        .collect()
}

#[test]
fn a_save_is_reflected_by_the_next_command() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let before = repo.tool("resonance_show", json!({"ref": "src/store.rs"}));
    let blob = before["properties"]["blob"].as_str().unwrap().to_string();

    // Two quick saves of the same size in the same instant must both be seen.
    repo.replace("src/store.rs", "conn.check();", "conn.check1();");
    let first = repo.tool("resonance_show", json!({"ref": "src/store.rs"}));
    repo.replace("src/store.rs", "conn.check1();", "conn.check2();");
    let second = repo.tool("resonance_show", json!({"ref": "src/store.rs"}));
    assert_ne!(first["properties"]["blob"], blob);
    assert_ne!(
        second["properties"]["blob"], first["properties"]["blob"],
        "racy timestamps are re-hashed"
    );

    let open = repo.tool("resonance_show", json!({"ref": "open"}));
    assert_eq!(open["location"]["startLine"], 4);
    // The graph's own copy of the unit's text follows the latest save.
    let id = open["node"]["id"].as_str().unwrap();
    let store = SqliteStore::open(&repo.path().join(".resonance/local/state.db")).unwrap();
    let node = store.get_node(id).unwrap().unwrap();
    let quote = node.props["anchor"]["quote"].as_str().unwrap();
    assert!(quote.contains("conn.check2();"), "{quote}");
}

#[test]
fn edits_between_commands_are_always_reflected() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    for i in 0..5 {
        let header = format!("// revision {i}\n").repeat(i + 1);
        repo.write("src/store.rs", &format!("{header}{STORE}"));
        let open = repo.tool("resonance_show", json!({"ref": "open"}));
        assert_eq!(
            open["location"]["startLine"],
            4 + i as u64 + 1,
            "revision {i}"
        );
    }
}

#[test]
fn moves_and_approximate_matches_are_recorded_as_identity_operations() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    assert!(!op_kinds(&repo).iter().any(|k| k.starts_with("identity.")));

    // Step 4: reformat, recorded as identity.retarget with its confidence.
    repo.write("src/store.rs", &STORE.replace("\n    ", "\n  "));
    let open = repo.tool("resonance_show", json!({"ref": "open"}));
    assert_eq!(open["location"]["state"], "attached");
    let confidence = open["location"]["confidence"].as_f64().unwrap();
    assert!((0.9..1.0).contains(&confidence), "{confidence}");
    assert!(op_kinds(&repo).contains(&"identity.retarget".to_string()));

    // Step 3: a move to another file, recorded as identity.move.
    let text = repo.read("src/store.rs");
    let start = text.find("/// Opens the store.").unwrap();
    let end = text.find("pub fn close").unwrap();
    repo.write("src/open.rs", &text[start..end]);
    repo.write(
        "src/store.rs",
        &format!("{}{}", &text[..start], &text[end..]),
    );
    assert_eq!(
        repo.tool("resonance_show", json!({"ref": "open"}))["location"]["path"],
        "src/open.rs"
    );
    assert!(op_kinds(&repo).contains(&"identity.move".to_string()));

    // Every operation carries the §9.1 envelope.
    let mut store = SqliteStore::open(&repo.path().join(".resonance/local/state.db")).unwrap();
    store.migrate().unwrap();
    let ops = store.ops_after(0, 1_000_000).unwrap();
    let mv = ops.iter().find(|r| r.op.kind == "identity.move").unwrap();
    assert_eq!(mv.op.actor.id, "resonance");
    assert!(!mv.op.change_set.is_empty() && !mv.op.hlc.is_empty());
    assert_eq!(mv.op.payload["to"], "src/open.rs:2-7");
    let hlcs: Vec<&str> = ops.iter().map(|r| r.op.hlc.as_str()).collect();
    let mut sorted = hlcs.clone();
    sorted.sort();
    assert_eq!(hlcs, sorted, "the clock never goes backwards");
}

#[test]
fn a_low_confidence_match_goes_on_the_repair_list_until_confirmed() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    // Most of the function's lines change a little: diff mapping and exact search fail, and
    // the approximate match scores between 0.8 and 0.9 (MVP-013 step 4).
    let edited = STORE
        .replace("conn.migrate();", "conn.run_migrations();")
        .replace("conn.check();", "conn.check_all();")
        .replace("let conn = connect(path);", "let conn = connect_to(path);")
        .replace("Store { conn }", "Store::new(conn)")
        .replace("pub fn open(path: &str)", "pub fn open(path: &Path)");
    repo.write("src/store.rs", &edited);
    let open = repo.tool("resonance_show", json!({"ref": "open"}));
    assert_eq!(open["location"]["state"], "attached");
    let confidence = open["location"]["confidence"].as_f64().unwrap();
    assert!((0.8..0.9).contains(&confidence), "{confidence}");
    let status = repo.json(&["status"]);
    let item = status["repair"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "low-confidence-anchor")
        .cloned()
        .expect("FR-038: matches below 0.9 go on the repair list");
    assert!(item["summary"].as_str().unwrap().contains("confidence"));
    assert_eq!(item["location"]["startLine"], 4);

    // The agent checks the region and confirms it.
    let out = repo.tool("resonance_edit_unit", json!({"ref": "open"}));
    assert_eq!(out["location"]["confidence"], 1.0, "{out}");
    let after = repo.json(&["status"]);
    assert!(
        !after["repair"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "low-confidence-anchor")
    );
    assert_eq!(
        repo.tool("resonance_show", json!({"ref": "open"}))["location"]["confidence"],
        1.0
    );
}

#[test]
fn rebuild_derived_and_full_reconcile_keep_everything() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.tool(
        "resonance_declare_unit",
        json!({"path": "src/store.rs", "startLine": 5, "endLine": 6, "name": "inner"}),
    );
    let before = repo.tool("resonance_why", json!({"ref": "inner"}));
    repo.ok(&["rebuild-derived"]);
    repo.ok(&["reconcile", "--full"]);
    let after = repo.tool("resonance_why", json!({"ref": "inner"}));
    assert_eq!(before["enclosedBy"], after["enclosedBy"]);
    assert_eq!(after["enclosedBy"][0]["label"], "src/store.rs::open");
    assert_eq!(
        repo.tool("resonance_trace", json!({"requirement": "FR-001"}))["satisfiedBy"][0]["link"]["status"],
        "asserted"
    );
}
