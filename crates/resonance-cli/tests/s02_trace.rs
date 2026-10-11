//! S2: AC1 `trace` returns satisfying code, verifying tests, each link's status and latest
//! evidence in one query; AC2 requirements with no verified test in one query (`unverified`).

mod common;

use common::TestRepo;
use serde_json::Value;

fn labels(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|e| e["link"]["other"]["label"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn trace_shows_code_tests_statuses_and_evidence() {
    let repo = TestRepo::with_project();
    repo.declare_sample();

    let before = repo.json(&["trace", "FR-001"]);
    assert_eq!(before["requirement"]["key"], "FR-001");
    assert_eq!(before["requirement"]["specStatus"], "Decided");
    assert_eq!(labels(&before["satisfiedBy"]), ["src/store.rs::open"]);
    assert_eq!(
        labels(&before["verifiedBy"]),
        ["tests/store_test.sh::opens_db"]
    );
    assert_eq!(before["satisfiedBy"][0]["link"]["status"], "asserted");
    assert_eq!(before["verifiedBy"][0]["link"]["status"], "asserted");
    assert!(
        before["verifiedBy"][0]["link"]
            .get("latestEvidence")
            .is_none()
    );
    assert_eq!(before["satisfiedBy"][0]["unit"]["location"]["startLine"], 4);

    repo.ok(&["test", "FR-001"]);
    let after = repo.json(&["trace", "FR-001"]);
    let verify = &after["verifiedBy"][0]["link"];
    assert_eq!(verify["status"], "verified");
    assert_eq!(verify["latestEvidence"]["result"], "pass");
    assert_eq!(verify["latestEvidence"]["exitCode"], 0);
    assert!(verify["latestEvidence"]["commit"].as_str().unwrap().len() >= 40);

    let text = repo.ok(&["trace", "FR-001"]);
    assert!(
        text.contains("Satisfied by:") && text.contains("Verified by:"),
        "{text}"
    );
    assert!(
        text.contains("[verified] tests/store_test.sh::opens_db"),
        "{text}"
    );
}

#[test]
fn unverified_lists_requirements_without_a_verified_test() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let keys = |v: Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|u| u["requirement"]["key"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        keys(repo.json(&["unverified"])),
        ["FR-001", "FR-002", "FR-003", "NFR-001"]
    );
    repo.ok(&["test", "FR-001"]);
    assert_eq!(
        keys(repo.json(&["unverified"])),
        ["FR-002", "FR-003", "NFR-001"]
    );
    // The same answer as a status filter (FR-005).
    let verified = repo.json(&["search", "--type", "Requirement", "--status", "verified"]);
    assert_eq!(verified.as_array().unwrap().len(), 1);
    assert_eq!(verified[0]["label"], "FR-001");
}

#[test]
fn trace_accepts_only_requirements() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let err = repo.fails(&["trace", "open"], 1);
    assert!(err.contains("resonance why"), "{err}");
}
