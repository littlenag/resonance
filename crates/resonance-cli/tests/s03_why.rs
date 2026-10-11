//! S3 AC1: one `why` call returns the requirements a code unit satisfies, the decisions that
//! constrain it, the tests that cover it, and the annotations on it and its containing units.

mod common;

use common::TestRepo;

#[test]
fn why_returns_requirements_decisions_tests_and_annotations() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let decision = repo.json(&[
        "decision",
        "add",
        "--title",
        "SQLite for the store",
        "--decision",
        "Use SQLite behind the store API",
        "--context",
        "embedded, robust",
    ]);
    let decision_id = decision["id"].as_str().unwrap();
    repo.ok(&["link", decision_id, "constrains", "open"]);
    repo.ok(&[
        "test",
        "add",
        "tests/store_test.sh:4",
        "--name",
        "open_case",
        "--run",
        "sh tests/store_test.sh open",
    ]);
    repo.ok(&["link", "open_case", "verifies", "open"]);
    // A unit inside `open`, and annotations on `open` and on the file.
    repo.ok(&[
        "unit",
        "add",
        "src/store.rs:5-6",
        "--name",
        "connect_and_migrate",
        "--kind",
        "block",
    ]);
    repo.ok(&[
        "annotate",
        "open",
        "--kind",
        "instruction",
        "--text",
        "do not change the open() signature",
    ]);
    repo.ok(&[
        "annotate",
        "src/store.rs",
        "--kind",
        "note",
        "--text",
        "store module",
    ]);

    let why = repo.json(&["why", "open"]);
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
    let inner = repo.json(&["why", "connect_and_migrate"]);
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

    let text = repo.ok(&["why", "open"]);
    for needle in [
        "Satisfies:",
        "FR-001",
        "Constrained by:",
        "Covered by tests:",
        "do not change",
    ] {
        assert!(text.contains(needle), "{needle} missing from:\n{text}");
    }
}

#[test]
fn units_are_addressed_by_path_name_or_id_and_duplicates_get_numbers() {
    let repo = TestRepo::with_project();
    repo.ok(&[
        "unit",
        "add",
        "src/store.rs:4-9",
        "--name",
        "fn",
        "--kind",
        "function",
    ]);
    repo.ok(&[
        "unit",
        "add",
        "src/store.rs:11-14",
        "--name",
        "fn",
        "--kind",
        "function",
    ]);
    let second = repo.json(&["show", "src/store.rs::fn#2"]);
    assert_eq!(second["location"]["startLine"], 11);
    let err = repo.fails(&["show", "fn"], 1);
    assert!(
        err.contains("ambiguous") && err.contains("src/store.rs::fn#2"),
        "{err}"
    );
    let id = second["node"]["id"].as_str().unwrap();
    let by_prefix = repo.json(&["show", &id[..12]]);
    assert_eq!(by_prefix["node"]["id"], id);
    // A path relative to the current directory works too.
    let out = repo.run(&["-C", "src", "--json", "show", "store.rs::fn"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
