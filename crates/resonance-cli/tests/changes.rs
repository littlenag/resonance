//! The change log (MVP-040, mvp.md §5 "S25 / TUI and change log"), read and cleared by agents
//! through MCP: entries for spec changes that arrive with a pull; none for agents' own changes
//! by default; one pending entry per item; `since`, `show` and `done`. People's TUI edits are
//! covered by the TUI's own tests.

mod common;

use common::TestRepo;
use serde_json::{Value, json};

fn entries(repo: &TestRepo) -> Vec<Value> {
    repo.tool("resonance_changes", json!({}))
        .as_array()
        .unwrap()
        .clone()
}

fn keys(list: &[Value]) -> Vec<String> {
    list.iter()
        .map(|e| e["item"]["key"].as_str().unwrap().to_string())
        .collect()
}

fn done(repo: &TestRepo, seqs: &[u64]) -> Value {
    repo.tool("resonance_changes_done", json!({"seqs": seqs}))
}

/// Log agents' changes too, so that MCP calls can stand in for a person's edits.
fn include_agents(repo: &TestRepo) {
    let cfg = repo.read(".resonance/config.toml");
    assert!(cfg.contains("includeAgents = false"), "{cfg}");
    repo.write(
        ".resonance/config.toml",
        &cfg.replace("includeAgents = false", "includeAgents = true"),
    );
}

#[test]
fn agents_own_changes_are_not_logged_by_default() {
    let repo = TestRepo::with_project();
    assert!(entries(&repo).is_empty(), "set-up logs nothing");
    repo.import_sample();
    repo.tool(
        "resonance_create_requirement",
        json!({"key": "FR-200", "text": "By an agent"}),
    );
    assert!(entries(&repo).is_empty(), "{:?}", entries(&repo));
    assert_eq!(repo.json(&["status"])["pendingChanges"], 0);
}

#[test]
fn entries_are_replaced_per_item_and_cleared_by_the_agent() {
    let repo = TestRepo::with_project();
    include_agents(&repo);
    repo.tool(
        "resonance_create_requirement",
        json!({"key": "FR-100", "text": "Exports are complete"}),
    );
    let first = entries(&repo);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["kind"], "created");
    assert_eq!(first[0]["origin"], "mcp");
    assert_eq!(first[0]["actor"], "agent:test-agent");
    assert!(first[0]["summary"].as_str().unwrap().len() <= 100);
    assert!(first[0].get("before").is_none(), "lists are brief");
    let seq1 = first[0]["seq"].as_u64().unwrap();

    // A second change to the same item replaces its entry, with a new sequence number.
    repo.tool(
        "resonance_edit_item",
        json!({"ref": "FR-100", "fields": {"priority": "high"}}),
    );
    let second = entries(&repo);
    assert_eq!(second.len(), 1, "{second:?}");
    let seq2 = second[0]["seq"].as_u64().unwrap();
    assert!(seq2 > seq1);
    assert_eq!(second[0]["kind"], "created", "still new to the agent");
    assert!(
        second[0]["fields"]
            .as_array()
            .unwrap()
            .contains(&json!("priority"))
    );

    repo.tool(
        "resonance_create_requirement",
        json!({"key": "FR-101", "text": "Restores are idempotent"}),
    );
    let since = repo.tool("resonance_changes", json!({"since": seq2}));
    assert_eq!(keys(since.as_array().unwrap()), ["FR-101"]);

    // `show` gives the full before and after.
    let show = repo.tool("resonance_changes_show", json!({"seq": seq2}));
    assert_eq!(show["after"]["priority"], "high");
    assert_eq!(show["after"]["text"], "Exports are complete");

    // Handling an edit: before is what the agent last saw.
    let d = repo.tool(
        "resonance_changes_done",
        json!({"seqs": [seq2], "note": "implemented"}),
    );
    assert_eq!(d["removed"], json!([seq2]));
    assert_eq!(d["remaining"], 1);
    repo.tool(
        "resonance_edit_item",
        json!({"ref": "FR-100", "fields": {"text": "Exports are complete and signed"}}),
    );
    let e = entries(&repo)
        .into_iter()
        .find(|e| e["item"]["key"] == "FR-100")
        .unwrap();
    assert_eq!(e["kind"], "edited");
    let seq3 = e["seq"].as_u64().unwrap();
    assert!(seq3 > seq2, "sequence numbers are never reused");
    let show = repo.tool("resonance_changes_show", json!({"seq": seq3}));
    assert_eq!(show["before"]["text"], "Exports are complete");
    assert_eq!(show["after"]["text"], "Exports are complete and signed");

    // Retiring an item nobody handled yet.
    done(&repo, &[seq3]);
    repo.tool("resonance_retire_item", json!({"ref": "FR-101"}));
    let r = entries(&repo)
        .into_iter()
        .find(|e| e["item"]["key"] == "FR-101")
        .unwrap();
    assert_eq!(
        r["kind"], "created",
        "created then retired before anyone handled it"
    );

    assert_eq!(repo.json(&["status"])["pendingChanges"], 1);
    assert_eq!(done(&repo, &[999])["unknown"], json!([999]));
}

#[test]
fn spec_changes_arriving_through_a_pull_are_logged() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let bare = tempfile::tempdir().unwrap();
    let out = std::process::Command::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(bare.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    repo.git(&["remote", "add", "origin", bare.path().to_str().unwrap()]);
    repo.ok(&["share", "--yes"]);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "share"]);
    repo.git(&["push", "-q", "-u", "origin", "main"]);

    let mate = TestRepo::clone_of(bare.path());
    mate.ok(&["init", "--no-agent"]);
    assert!(entries(&mate).is_empty(), "joining logs nothing");
    mate.tool(
        "resonance_create_requirement",
        json!({"key": "FR-300", "text": "From a teammate"}),
    );
    mate.write("MATE.md", "m\n");
    mate.git(&["add", "-A"]);
    mate.git(&["commit", "-q", "-m", "mate"]);
    mate.git(&["push", "-q", "origin", "main"]);

    let before: Vec<u64> = entries(&repo)
        .iter()
        .map(|e| e["seq"].as_u64().unwrap())
        .collect();
    if !before.is_empty() {
        done(&repo, &before);
    }
    repo.git(&["pull", "-q", "--no-rebase", "origin", "main"]);
    let e = entries(&repo);
    assert_eq!(keys(&e), ["FR-300"], "{e:?}");
    assert_eq!(e[0]["origin"], "git");
    assert_eq!(e[0]["actor"], "agent:test-agent");
}
