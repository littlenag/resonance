//! S7: AC1 editing a requirement's text in Markdown and re-running any command makes its
//! links stale; AC2 `trace` lists everything affected. Also MVP-011 retire and re-activate,
//! FR-013 unreadable sources, and the CLI rules for imported requirements.

mod common;

use common::TestRepo;

fn statuses(repo: &TestRepo, key: &str) -> Vec<String> {
    let t = repo.json(&["trace", key]);
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
    repo.ok(&["test", "FR-001"]);
    assert_eq!(
        statuses(&repo, "FR-001"),
        ["satisfies asserted", "verifies verified"]
    );

    repo.replace(
        "spec/requirements.md",
        "The store opens a database at a path.",
        "The store opens a database at a path, creating it if missing.",
    );
    // Any command picks up the change; `status` shows the stale links on the repair list.
    let status = repo.json(&["status"]);
    let stale = status["repair"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "stale-link")
        .count();
    assert_eq!(stale, 2, "AC1: {status}");
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
    let req = repo.json(&["show", "FR-001"]);
    assert!(
        req["properties"]["text"]
            .as_str()
            .unwrap()
            .contains("creating it if missing")
    );

    // Confirming and re-testing return the links to health.
    let satisfies = repo.json(&["trace", "FR-001"])["satisfiedBy"][0]["link"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(repo.json(&["confirm", &satisfies])["status"], "asserted");
    repo.ok(&["test", "FR-001"]);
    assert_eq!(
        statuses(&repo, "FR-001"),
        ["satisfies asserted", "verifies verified"]
    );
}

#[test]
fn removed_requirements_are_retired_and_come_back() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.replace(
        "spec/requirements.md",
        "- **FR-002** The store closes cleanly, flushing pending writes. [Proposed]\n",
        "",
    );
    let req = repo.json(&["show", "FR-002"]);
    assert_eq!(req["node"]["status"], "retired", "never deleted (MVP-011)");
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
    repo.git(&["checkout", "--", "spec/requirements.md"]);
    assert_eq!(repo.json(&["show", "FR-002"])["node"]["status"], "active");
}

#[test]
fn an_unreadable_source_keeps_its_requirements_and_is_listed() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.write_bytes("spec/requirements.md", b"\xff\xfe not utf-8");
    let status = repo.json(&["status"]);
    assert_eq!(status["counts"]["requirements"], 4, "FR-013: kept");
    let item = status["repair"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "unreadable-intent-source")
        .cloned()
        .expect("listed");
    assert_eq!(item["target"], "spec/requirements.md");
}

#[test]
fn imported_requirements_are_edited_in_their_source_and_authored_ones_through_the_cli() {
    let repo = TestRepo::with_project();
    repo.ok(&["import", "spec/requirements.md"]);
    let err = repo.fails(&["req", "edit", "FR-001", "--text", "new"], 1);
    assert!(err.contains("spec/requirements.md:3"), "{err}");
    let added = repo.json(&[
        "req",
        "add",
        "FR-100",
        "--text",
        "Exports are engine-neutral.",
    ]);
    assert_eq!(added["origin"], "authored");
    repo.ok(&["unit", "add", "src/store.rs:11-14", "--name", "close"]);
    repo.ok(&["link", "close", "satisfies", "FR-100"]);
    repo.ok(&[
        "req",
        "edit",
        "FR-100",
        "--text",
        "Exports are engine-neutral JSON Lines.",
    ]);
    assert_eq!(statuses(&repo, "FR-100"), ["satisfies stale"]);
    assert_eq!(repo.json(&["req", "retire", "FR-100"])["status"], "retired");
    repo.fails(&["req", "add", "FR-100", "--text", "again"], 1);
    repo.fails(&["req", "add", "not-a-key", "--text", "x"], 2);
}
