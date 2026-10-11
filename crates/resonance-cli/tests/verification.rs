//! Verification (FR-135, FR-004, MVP-017): passing tests mark `verifies` links verified,
//! failing tests record failing evidence, and a command that cannot start or runs zero tests
//! is recorded as `error`, never a pass.

mod common;

use common::TestRepo;
use serde_json::Value;

fn verifies_status(repo: &TestRepo, key: &str) -> Value {
    repo.json(&["trace", key])["verifiedBy"][0]["link"]["status"].clone()
}

fn declare_test(repo: &TestRepo, name: &str, run: &str) {
    repo.ok(&[
        "test",
        "add",
        "tests/store_test.sh:3-5",
        "--name",
        name,
        "--run",
        run,
    ]);
}

#[test]
fn a_pass_verifies_and_records_evidence() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let report = repo.json(&["test", "FR-001"]);
    assert_eq!(report["passed"], 1);
    let run = &report["runs"][0];
    assert_eq!(run["result"], "pass");
    assert_eq!(run["exitCode"], 0);
    assert!(run["outputTail"].as_str().unwrap().contains("open ok"));
    assert_eq!(run["links"][0]["status"], "verified");
    let evidence = repo.json(&["show", run["evidenceId"].as_str().unwrap()]);
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
    repo.ok(&["test", "FR-001"]);
    assert_eq!(verifies_status(&repo, "FR-001"), "verified");
    declare_test(&repo, "broken", "sh tests/store_test.sh nope");
    repo.ok(&["link", "broken", "verifies", "FR-002"]);
    let out = repo.run(&["--json", "test", "FR-002"]);
    assert_eq!(out.status.code(), Some(1), "scripts can tell");
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["failed"], 1);
    assert_eq!(report["runs"][0]["result"], "fail");
    assert_eq!(report["runs"][0]["exitCode"], 1);
    assert!(
        report["runs"][0]["outputTail"]
            .as_str()
            .unwrap()
            .contains("unknown test nope")
    );
    let trace = repo.json(&["trace", "FR-002"]);
    let link = &trace["verifiedBy"][0]["link"];
    assert_eq!(link["status"], "asserted");
    assert_eq!(link["latestEvidence"]["result"], "fail");

    // A verified link whose test starts failing goes back to asserted.
    repo.replace("tests/store_test.sh", "exit 0", "exit 3");
    let out = repo.run(&["test", "opens_db"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(verifies_status(&repo, "FR-001"), "asserted");
}

#[test]
fn errors_are_never_passes() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    declare_test(&repo, "missing_cmd", "definitely-not-a-command-xyz --flag");
    declare_test(&repo, "zero_tests", "echo 'running 0 tests'; exit 0");
    repo.ok(&["link", "missing_cmd", "verifies", "FR-002"]);
    repo.ok(&["link", "zero_tests", "verifies", "FR-003"]);
    for (key, why) in [("FR-002", "not found"), ("FR-003", "no test actually ran")] {
        let out = repo.run(&["--json", "test", key]);
        assert_eq!(out.status.code(), Some(1));
        let report: Value = serde_json::from_slice(&out.stdout).unwrap();
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
    repo.ok(&["import", "spec/requirements.md"]);
    declare_test(&repo, "slow", "sleep 30");
    repo.ok(&["link", "slow", "verifies", "FR-001"]);
    let started = std::time::Instant::now();
    let out = repo
        .command(&["--json", "test", "slow"])
        .env("RESONANCE_TEST_TIMEOUT_SECONDS", "1")
        .output()
        .unwrap();
    assert!(started.elapsed().as_secs() < 15, "stopped by the timeout");
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
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
    let link = repo.json(&["trace", "FR-001"])["verifiedBy"][0]["link"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(repo.json(&["test", &link])["passed"], 1);
    assert_eq!(repo.json(&["test", "opens_db"])["passed"], 1);
    assert_eq!(
        repo.json(&["test", "open"])["runs"]
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
    repo.ok(&["test", "add", "tests/store_test.sh:4", "--name", "open"]);
    repo.ok(&["link", "tests/store_test.sh::open", "verifies", "FR-003"]);
    let report = repo.json(&["test", "FR-003"]);
    assert_eq!(
        report["runs"][0]["command"],
        "sh tests/store_test.sh open # tests/store_test.sh tests/store_test.sh::open"
    );
    assert_eq!(report["passed"], 1);
}

#[test]
fn declaring_a_test_needs_a_command() {
    let repo = TestRepo::with_project();
    let err = repo.fails(
        &["test", "add", "tests/store_test.sh:3-5", "--name", "t"],
        2,
    );
    assert!(err.contains("--run is required"), "{err}");
    let err = repo.fails(&["test"], 2);
    assert!(err.contains("test add"), "{err}");
}
