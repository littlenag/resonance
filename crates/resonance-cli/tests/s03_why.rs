//! S3 AC1: one `why` call returns the requirements a code unit satisfies, the decisions that
//! constrain it, the tests that cover it, and the annotations on it and its containing units.

mod common;

use common::TestRepo;
use serde_json::json;

#[test]
fn why_returns_requirements_decisions_tests_and_annotations() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let decision = repo.tool("resonance_create_decision", json!({"title": "SQLite for the store", "decision": "Use SQLite behind the store API", "context": "embedded, robust"}));
    let decision_id = decision["id"].as_str().unwrap();
    repo.link(decision_id, "constrains", "open");
    repo.tool("resonance_declare_test", json!({"path": "tests/store_test.sh", "startLine": 4, "endLine": 4, "name": "open_case", "run": "sh tests/store_test.sh open"}));
    repo.link("open_case", "verifies", "open");
    // A unit inside `open`, and annotations on `open` and on the file.
    repo.tool("resonance_declare_unit", json!({"path": "src/store.rs", "startLine": 5, "endLine": 6, "name": "connect_and_migrate", "kind": "block"}));
    repo.annotate("open", "instruction", "do not change the open() signature");
    repo.annotate("src/store.rs", "note", "store module");

    let why = repo.tool("resonance_why", json!({"ref": "open"}));
    assert_eq!(why["unit"]["address"], "src/store.rs::open");
    assert_eq!(why["requirements"][0]["requirement"]["key"], "FR-001");
    assert_eq!(why["requirements"][0]["link"]["status"], "asserted");
    assert_eq!(
        why["decisions"][0]["decision"]["title"],
        "SQLite for the store"
    );
    assert_eq!(
        why["tests"][0]["test"]["address"],
        "tests/store_test.sh::open_case"
    );
    let annotations: Vec<&str> = why["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        annotations,
        ["do not change the open() signature", "store module"]
    );

    // The inner unit inherits annotations and decisions from what contains it.
    let inner = repo.tool("resonance_why", json!({"ref": "connect_and_migrate"}));
    let inherited: Vec<(&str, bool)> = inner["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a["text"].as_str().unwrap(),
                a["inherited"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        inherited,
        [
            ("do not change the open() signature", true),
            ("store module", true)
        ]
    );
    assert_eq!(inner["decisions"][0]["via"]["label"], "src/store.rs::open");
    let enclosed: Vec<&str> = inner["enclosedBy"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["label"].as_str().unwrap())
        .collect();
    assert_eq!(enclosed, ["src/store.rs::open", "src/store.rs"]);

    let text = repo
        .tool("resonance_why", json!({"ref": "open"}))
        .to_string();
    for needle in [
        "satisfies",
        "FR-001",
        "constrained by",
        "verified by",
        "do not change",
    ] {
        assert!(text.contains(needle), "{needle} missing from:\n{text}");
    }
}

#[test]
fn units_are_addressed_by_path_name_or_id_and_duplicates_get_numbers() {
    let repo = TestRepo::with_project();
    repo.tool("resonance_declare_unit", json!({"path": "src/store.rs", "startLine": 4, "endLine": 9, "name": "fn", "kind": "function"}));
    repo.tool("resonance_declare_unit", json!({"path": "src/store.rs", "startLine": 11, "endLine": 14, "name": "fn", "kind": "function"}));
    let second = repo.tool("resonance_show", json!({"ref": "src/store.rs::fn#2"}));
    assert_eq!(second["location"]["startLine"], 11);
    let err = repo.tool_err("resonance_show", json!({"ref": "fn"}));
    assert!(
        err.contains("ambiguous") && err.contains("src/store.rs::fn#2"),
        "{err}"
    );
    let id = second["node"]["id"].as_str().unwrap();
    let by_prefix = repo.tool("resonance_show", json!({"ref": &id[..12]}));
    assert_eq!(by_prefix["node"]["id"], id);
    // A unit address works too.
    let first = repo.tool("resonance_show", json!({"ref": "src/store.rs::fn"}));
    assert_eq!(first["node"]["type"], "CodeUnit");
}
