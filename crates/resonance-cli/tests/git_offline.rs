//! Git-aware offline mode and recovery (B3, mvp.md §5 "Branch-aware offline deltas" and
//! "S18 / recovery"): offline deltas follow branches and rewrites (MVP-030), divergences are
//! classified from git evidence (MVP-031), and what git discards can be put back (MVP-032).

mod common;

use common::TestRepo;
use serde_json::{Value, json};

fn annotations_on(repo: &TestRepo, unit: &str) -> Vec<String> {
    repo.tool("resonance_why", json!({"ref": unit}))["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["text"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn committed_sample() -> TestRepo {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.write("NOTES.md", "notes\n");
    repo.commit_all("the sample's graph changes belong to this commit");
    repo
}

#[test]
fn offline_graph_changes_follow_branches() {
    let repo = committed_sample();
    repo.git(&["checkout", "-q", "-b", "feature"]);
    repo.annotate("open", "note", "feature only");
    repo.write("FEATURE.md", "f\n");
    repo.commit_all("feature work");
    assert_eq!(annotations_on(&repo, "open"), ["feature only"]);

    repo.git(&["checkout", "-q", "main"]);
    assert!(
        annotations_on(&repo, "open").is_empty(),
        "main never had it"
    );
    repo.git(&["checkout", "-q", "feature"]);
    assert_eq!(annotations_on(&repo, "open"), ["feature only"]);
    assert_eq!(repo.git_status(), "");
}

#[test]
fn uncommitted_graph_changes_travel_with_checkouts() {
    let repo = committed_sample();
    repo.annotate("open", "note", "not committed yet");
    repo.git(&["checkout", "-q", "-b", "other"]);
    assert_eq!(annotations_on(&repo, "open"), ["not committed yet"]);
    repo.git(&["checkout", "-q", "main"]);
    assert_eq!(annotations_on(&repo, "open"), ["not committed yet"]);
}

#[test]
fn amended_and_rebased_commits_keep_their_graph_changes() {
    let repo = committed_sample();
    repo.annotate("open", "note", "amended");
    repo.write("A.md", "a\n");
    repo.commit_all("a");
    repo.ok(&["status"]);
    repo.write("A.md", "a2\n");
    repo.git(&["commit", "-q", "-a", "--amend", "-m", "a, amended"]);
    assert_eq!(annotations_on(&repo, "open"), ["amended"]);

    // A branch commit rebased onto a newer main keeps its delta (re-associated by patch ID).
    repo.git(&["checkout", "-q", "-b", "topic", "HEAD~1"]);
    repo.annotate("close", "note", "topic");
    repo.write("B.md", "b\n");
    repo.commit_all("b");
    repo.ok(&["status"]);
    repo.git(&["rebase", "-q", "main"]);
    assert_eq!(annotations_on(&repo, "close"), ["topic"]);
    assert_eq!(
        annotations_on(&repo, "open"),
        ["amended"],
        "main's history too"
    );
}

#[test]
fn a_dropped_commit_leaves_its_delta_in_the_detached_pool() {
    let repo = committed_sample();
    repo.annotate("open", "note", "will be detached");
    repo.write("C.md", "c\n");
    repo.commit_all("c");
    repo.ok(&["status"]);
    repo.git(&["reset", "-q", "--hard", "HEAD~1"]);
    let status = repo.json(&["status"]);
    let detached = status["history"]["detached"].as_array().unwrap().clone();
    assert_eq!(detached.len(), 1, "{}", status["history"]);
    assert!(annotations_on(&repo, "open").is_empty());

    let list = repo.json(&["recover"]);
    assert_eq!(list["detached"].as_array().unwrap().len(), 1);
    let id = detached[0].as_str().unwrap();
    repo.ok(&["recover", "attach", id]);
    assert_eq!(annotations_on(&repo, "open"), ["will be detached"]);
    assert!(
        repo.json(&["status"])["history"]["detached"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_edit_discarded_by_git_can_be_put_back() {
    let repo = committed_sample();
    repo.replace("src/store.rs", "conn.check();", "conn.check_everything();");
    repo.ok(&["status"]);
    repo.git(&["checkout", "--", "src/store.rs"]);

    let list = repo.json(&["recover"]);
    let divs: Vec<Value> = list["divergences"].as_array().unwrap().clone();
    let c4 = divs
        .iter()
        .find(|d| d["class"] == "C4")
        .unwrap_or_else(|| panic!("{list}"));
    assert_eq!(c4["level"], "attention");
    assert_eq!(c4["paths"][0], "src/store.rs");
    let rec = c4["recovery"].as_str().unwrap().to_string();
    assert!(!repo.read("src/store.rs").contains("check_everything"));

    repo.ok(&["recover", "undo", &rec]);
    assert!(
        repo.read("src/store.rs").contains("check_everything"),
        "the edit is back"
    );
    let after = repo.json(&["recover"]);
    assert!(
        !after["divergences"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["class"] == "C4"),
        "undone divergences are resolved: {after}"
    );
}

#[test]
fn a_pull_with_teammates_commits_is_classified() {
    let repo = committed_sample();
    let mate = TestRepo::clone_of(repo.path());
    mate.replace("src/store.rs", "drop(store);", "drop(store); // done");
    mate.commit_all("mate's change");
    repo.git(&[
        "pull",
        "-q",
        "--no-rebase",
        mate.path().to_str().unwrap(),
        "main",
    ]);
    let value = repo.tool("resonance_value", json!({}));
    let d = &value["divergences"];
    assert!(d["C1"].as_u64().unwrap_or(0) >= 1, "{value}");
    assert!(d["C2"].as_u64().unwrap_or(0) >= 1, "{value}");
}

#[test]
fn value_counts_what_resonance_did() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.tool("resonance_trace", json!({"requirement": "FR-001"}));
    repo.tool("resonance_why", json!({"ref": "open"}));
    repo.tool("resonance_context", json!({"requirements": ["FR-001"]}));
    let v = repo.tool("resonance_value", json!({}));
    assert_eq!(v["traces"], 1);
    assert_eq!(v["whys"], 1);
    assert_eq!(v["contextPacks"], 1);
    assert!(
        v["upgrade"].is_string(),
        "offline: how sharing improves things"
    );
}

#[test]
fn edits_made_while_no_daemon_ran_are_classified_at_startup() {
    let repo = TestRepo::with_project_daemon();
    repo.declare_sample();
    repo.ok(&["daemon", "stop"]);
    repo.write("src/new.rs", "pub fn new() {}\n");
    repo.ok(&["status"]);
    let v = repo.tool("resonance_value", json!({}));
    assert!(v["divergences"]["C3"].as_u64().unwrap_or(0) >= 1, "{v}");
}
