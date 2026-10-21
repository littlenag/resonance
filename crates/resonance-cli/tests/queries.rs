//! Odd queries: an empty graph, a retired requirement, duplicate unit names, a context pack
//! that is not a requirement, and an unknown status. The happy paths live in the scenario tests.

mod common;

use common::TestRepo;
use serde_json::{Value, json};

fn labels(hits: &Value) -> Vec<String> {
    hits.as_array()
        .unwrap()
        .iter()
        .map(|h| h["label"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn an_empty_graph_searches_repairs_and_rejects_an_empty_context_pack() {
    let repo = TestRepo::with_project();
    assert_eq!(
        repo.tool(
            "resonance_search",
            json!({"text": "store", "type": "Requirement"})
        )
        .as_array()
        .unwrap()
        .len(),
        0
    );
    assert_eq!(
        repo.tool("resonance_search", json!({"text": "zzzz-no-such"}))
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        repo.tool("resonance_repair_list", json!({}))
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let err = repo.tool_err("resonance_context", json!({"requirements": []}));
    assert!(err.contains("at least one requirement"), "{err}");
    let unknown = repo.tool_err(
        "resonance_search",
        json!({"text": "store", "status": "nope"}),
    );
    assert!(unknown.contains("unknown status"), "{unknown}");
}

#[test]
fn retired_requirements_match_their_status_and_duplicate_names_both_match() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    // FR-002 has a satisfies link, so retiring it is also a repair-list row.
    // Retirement is a status: an unfiltered search still returns the item.
    repo.tool("resonance_retire_item", json!({"ref": "FR-002"}));

    let plain = labels(&repo.tool(
        "resonance_search",
        json!({"text": "store", "type": "Requirement"}),
    ));
    assert!(plain.contains(&"FR-001".to_string()), "{plain:?}");
    assert!(plain.contains(&"FR-002".to_string()), "{plain:?}");
    assert!(plain.contains(&"FR-003".to_string()), "{plain:?}");

    let active = labels(&repo.tool(
        "resonance_search",
        json!({"text": "store", "type": "Requirement", "status": "active"}),
    ));
    assert!(active.contains(&"FR-001".to_string()), "{active:?}");
    assert!(active.contains(&"FR-003".to_string()), "{active:?}");
    assert!(!active.contains(&"FR-002".to_string()), "{active:?}");

    let retired = labels(&repo.tool(
        "resonance_search",
        json!({"text": "closes", "type": "Requirement", "status": "retired"}),
    ));
    assert_eq!(retired, ["FR-002"]);

    let upper = labels(&repo.tool(
        "resonance_search",
        json!({"text": "STORE", "type": "Requirement", "limit": 1}),
    ));
    assert_eq!(upper.len(), 1, "{upper:?}");

    repo.write(
        "src/twins.rs",
        "pub fn twin() {\n    let a = 1;\n}\n\npub fn twin() {\n    let b = 2;\n}\n",
    );
    repo.tool(
        "resonance_declare_unit",
        json!({"path": "src/twins.rs", "startLine": 1, "endLine": 3, "name": "twin", "kind": "function"}),
    );
    repo.tool(
        "resonance_declare_unit",
        json!({"path": "src/twins.rs", "startLine": 5, "endLine": 7, "name": "twin", "kind": "function"}),
    );
    let mut twins = labels(&repo.tool(
        "resonance_search",
        json!({"text": "twin", "type": "CodeUnit"}),
    ));
    twins.sort();
    assert_eq!(twins.len(), 2, "{twins:?}");
    assert!(twins.iter().any(|l| l.ends_with("::twin")), "{twins:?}");
    assert!(twins.iter().any(|l| l.ends_with("::twin#2")), "{twins:?}");

    let err = repo.tool_err("resonance_context", json!({"requirements": ["open"]}));
    assert!(
        err.contains("starts from requirements") || err.contains("CodeUnit"),
        "{err}"
    );

    let pack = repo.tool(
        "resonance_context",
        json!({"requirements": ["FR-001"], "task": "review the open path"}),
    );
    assert_eq!(pack["task"], "review the open path");
    assert_eq!(pack["requirements"][0]["key"], "FR-001");

    let repair = repo.tool("resonance_repair_list", json!({}));
    let kinds: Vec<&str> = repair
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap())
        .collect();
    assert!(
        kinds.contains(&"retired-requirement-with-links"),
        "{kinds:?}"
    );
}
