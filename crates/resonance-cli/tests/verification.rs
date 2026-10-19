//! Verification (FR-135, FR-004, MVP-017): passing tests mark `verifies` links verified,
//! failing tests record failing evidence, and a command that cannot start or runs zero tests
//! is recorded as `error`, never a pass.

mod common;

use common::TestRepo;
use serde_json::{Value, json};

fn verifies_status(repo: &TestRepo, key: &str) -> Value {
    repo.tool("resonance_trace", json!({"requirement": key}))["verifiedBy"][0]["link"]["status"]
        .clone()
}

fn declare_test(repo: &TestRepo, name: &str, run: &str) {
    repo.tool("resonance_declare_test", json!({"path": "tests/store_test.sh", "startLine": 3, "endLine": 5, "name": name, "run": run}));
}

#[test]
fn a_pass_verifies_and_records_evidence() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let report = repo.tool("resonance_test", json!({"ref": "FR-001"}));
    assert_eq!(report["passed"], 1);
    let run = &report["runs"][0];
    assert_eq!(run["result"], "pass");
    assert_eq!(run["exitCode"], 0);
    assert!(run["outputTail"].as_str().unwrap().contains("open ok"));
    assert_eq!(run["links"][0]["status"], "verified");
    let evidence = repo.tool(
        "resonance_show",
        json!({"ref": run["evidenceId"].as_str().unwrap()}),
    );
    let p = &evidence["properties"];
    assert_eq!(p["kind"], "test");
    assert_eq!(p["result"], "pass");
    assert!(p["commit"].as_str().unwrap().len() == 40);
    assert!(p["toolVersion"].as_str().unwrap().starts_with("resonance "));
    assert!(p["timestamp"].as_str().unwrap().ends_with('Z'));
    assert_eq!(verifies_status(&repo, "FR-001"), "verified");
}

#[test]
fn a_fail_leaves_the_link_asserted_with_failing_evidence() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.tool("resonance_test", json!({"ref": "FR-001"}));
    assert_eq!(verifies_status(&repo, "FR-001"), "verified");
    declare_test(&repo, "broken", "sh tests/store_test.sh nope");
    repo.link("broken", "verifies", "FR-002");
    let report = repo.tool("resonance_test", json!({"ref": "FR-002"}));
    assert_eq!(report["failed"], 1);
    assert_eq!(report["runs"][0]["result"], "fail");
    assert_eq!(report["runs"][0]["exitCode"], 1);
    assert!(
        report["runs"][0]["outputTail"]
            .as_str()
            .unwrap()
            .contains("unknown test nope")
    );
    let trace = repo.tool("resonance_trace", json!({"requirement": "FR-002"}));
    let link = &trace["verifiedBy"][0]["link"];
    assert_eq!(link["status"], "asserted");
    assert_eq!(link["latestEvidence"]["result"], "fail");

    // A verified link whose test starts failing goes back to asserted.
    repo.replace("tests/store_test.sh", "exit 0", "exit 3");
    let report = repo.tool("resonance_test", json!({"ref": "opens_db"}));
    assert_eq!(report["failed"], 1, "{report}");
    assert_eq!(verifies_status(&repo, "FR-001"), "asserted");
}

#[test]
fn errors_are_never_passes() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    declare_test(&repo, "missing_cmd", "definitely-not-a-command-xyz --flag");
    declare_test(&repo, "zero_tests", "echo 'running 0 tests'; exit 0");
    repo.link("missing_cmd", "verifies", "FR-002");
    repo.link("zero_tests", "verifies", "FR-003");
    for (key, why) in [("FR-002", "not found"), ("FR-003", "no test actually ran")] {
        let report = repo.tool("resonance_test", json!({"ref": key}));
        assert_eq!(report["errors"], 1, "{report}");
        assert_eq!(report["runs"][0]["result"], "error");
        assert!(
            report["runs"][0]["error"].as_str().unwrap().contains(why),
            "{report}"
        );
        assert_eq!(verifies_status(&repo, key), "asserted");
    }
}

#[test]
fn a_test_that_runs_too_long_is_stopped_and_recorded_as_an_error() {
    let repo = TestRepo::with_project();
    repo.import_sample();
    declare_test(&repo, "slow", "sleep 30");
    repo.link("slow", "verifies", "FR-001");
    let cfg = repo
        .read(".resonance/config.toml")
        .replace("timeoutSeconds = 600", "timeoutSeconds = 1");
    repo.write(".resonance/config.toml", &cfg);
    let started = std::time::Instant::now();
    let report = repo.tool("resonance_test", json!({"ref": "slow"}));
    assert!(started.elapsed().as_secs() < 15, "stopped by the timeout");
    assert_eq!(report["runs"][0]["result"], "error");
    assert!(
        report["runs"][0]["error"]
            .as_str()
            .unwrap()
            .contains("timeout")
    );
}

#[test]
fn tests_run_by_requirement_test_or_link_and_fall_back_to_the_configured_command() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let link = repo.tool("resonance_trace", json!({"requirement": "FR-001"}))["verifiedBy"][0]["link"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        repo.tool("resonance_test", json!({"ref": &link}))["passed"],
        1
    );
    assert_eq!(
        repo.tool("resonance_test", json!({"ref": "opens_db"}))["passed"],
        1
    );
    assert_eq!(
        repo.tool("resonance_test", json!({"ref": "open"}))["runs"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "no test verifies the unit itself"
    );

    // test.command with substitutions, for tests declared without --run.
    let config = repo.read(".resonance/config.toml").replace(
        "command = \"\"",
        "command = \"sh tests/store_test.sh {name} # {path} {address}\"",
    );
    repo.write(".resonance/config.toml", &config);
    repo.tool(
        "resonance_declare_test",
        json!({"path": "tests/store_test.sh", "startLine": 4, "endLine": 4, "name": "open"}),
    );
    repo.link("tests/store_test.sh::open", "verifies", "FR-003");
    let report = repo.tool("resonance_test", json!({"ref": "FR-003"}));
    assert_eq!(
        report["runs"][0]["command"],
        "sh tests/store_test.sh open # tests/store_test.sh tests/store_test.sh::open"
    );
    assert_eq!(report["passed"], 1);
}

#[test]
fn declaring_a_test_needs_a_command() {
    let repo = TestRepo::with_project();
    let err = repo.tool_err(
        "resonance_declare_test",
        json!({"path": "tests/store_test.sh", "startLine": 3, "endLine": 5, "name": "t"}),
    );
    assert!(err.contains("run"), "{err}");
}

#[test]
fn a_verified_link_goes_stale_when_the_test_changes() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.tool("resonance_test", json!({"ref": "FR-001"}));
    assert_eq!(verifies_status(&repo, "FR-001"), "verified");
    // An edit inside the test's region changes its content hash (FR-002, MVP-012).
    repo.replace(
        "tests/store_test.sh",
        "echo \"open ok\"",
        "echo \"open ok!\"",
    );
    assert_eq!(verifies_status(&repo, "FR-001"), "stale");
    // An asserted link whose code changes stays asserted (MVP-012).
    assert_eq!(
        repo.tool("resonance_trace", json!({"requirement": "FR-001"}))["satisfiedBy"][0]["link"]["status"],
        "asserted"
    );
    repo.replace("src/store.rs", "conn.check();", "conn.check(); // checked");
    assert_eq!(
        repo.tool("resonance_trace", json!({"requirement": "FR-001"}))["satisfiedBy"][0]["link"]["status"],
        "asserted"
    );
    // Re-running the test verifies it again.
    repo.tool("resonance_test", json!({"ref": "FR-001"}));
    assert_eq!(verifies_status(&repo, "FR-001"), "verified");
}
