//! The change log (MVP-040, mvp.md §5 "S25 / TUI and change log"): entries for people's
//! changes from the CLI, a source document and a pull; none for MCP or `RESONANCE_ACTOR`
//! agents by default; one pending entry per item; `--since`, `show` and `done`.

mod common;

use std::io::Write;
use std::process::Stdio;

use common::TestRepo;
use serde_json::{Value, json};

fn entries(repo: &TestRepo) -> Vec<Value> {
    repo.json(&["changes"]).as_array().unwrap().clone()
}

fn keys(list: &[Value]) -> Vec<String> {
    list.iter()
        .map(|e| e["item"]["key"].as_str().unwrap().to_string())
        .collect()
}

/// Run MCP tool calls through `resonance mcp`, as an agent.
fn mcp(repo: &TestRepo, calls: &[(&str, Value)]) -> Vec<Value> {
    let mut child = repo
        .command(&["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = vec![json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "clientInfo": {"name": "test-agent"}}})];
    for (i, (name, args)) in calls.iter().enumerate() {
        lines.push(
            json!({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call",
            "params": {"name": name, "arguments": args}}),
        );
    }
    for l in lines {
        writeln!(stdin, "{l}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .skip(1)
        .collect()
}

#[test]
fn people_s_cli_changes_are_logged_and_replaced_per_item() {
    let repo = TestRepo::with_project();
    assert!(entries(&repo).is_empty(), "set-up logs nothing");
    repo.ok(&["req", "add", "FR-100", "--text", "Exports are complete"]);
    let first = entries(&repo);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["kind"], "created");
    assert_eq!(first[0]["origin"], "cli");
    assert_eq!(first[0]["actor"], "human:dev@example.com");
    assert!(first[0]["summary"].as_str().unwrap().len() <= 100);
    assert!(first[0].get("before").is_none(), "lists are brief");
    let seq1 = first[0]["seq"].as_u64().unwrap();

    // A second change to the same item replaces its entry, with a new sequence number.
    repo.ok(&["req", "edit", "FR-100", "--priority", "high"]);
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

    repo.ok(&["req", "add", "FR-101", "--text", "Restores are idempotent"]);
    let since = repo.json(&["changes", "--since", &seq2.to_string()]);
    assert_eq!(keys(since.as_array().unwrap()), ["FR-101"]);

    // `show` gives the full before and after.
    let show = repo.json(&["changes", "show", &seq2.to_string()]);
    assert_eq!(show["after"]["priority"], "high");
    assert_eq!(show["after"]["text"], "Exports are complete");

    // Handling an edit: before is what the agent last saw.
    let done = repo.json(&[
        "changes",
        "done",
        &seq2.to_string(),
        "--note",
        "implemented",
    ]);
    assert_eq!(done["removed"], json!([seq2]));
    assert_eq!(done["remaining"], 1);
    repo.ok(&[
        "req",
        "edit",
        "FR-100",
        "--text",
        "Exports are complete and signed",
    ]);
    let e = entries(&repo)
        .into_iter()
        .find(|e| e["item"]["key"] == "FR-100")
        .unwrap();
    assert_eq!(e["kind"], "edited");
    assert!(
        e["seq"].as_u64().unwrap() > seq2,
        "sequence numbers are never reused"
    );
    let show = repo.json(&["changes", "show", &e["seq"].to_string()]);
    assert_eq!(show["before"]["text"], "Exports are complete");
    assert_eq!(show["after"]["text"], "Exports are complete and signed");
    let text = repo.ok(&["changes", "show", &e["seq"].to_string()]);
    assert!(text.contains("- Exports are complete\n"), "{text}");

    // Retiring.
    repo.ok(&["changes", "done", &e["seq"].to_string()]);
    repo.ok(&["req", "retire", "FR-101"]);
    let r = entries(&repo)
        .into_iter()
        .find(|e| e["item"]["key"] == "FR-101")
        .unwrap();
    assert_eq!(
        r["kind"], "created",
        "created then retired before anyone handled it"
    );

    let st = repo.json(&["status"]);
    assert_eq!(st["pendingChanges"], 1);
    assert!(
        repo.ok(&["changes", "done", "999"])
            .contains("Not pending: 999")
    );
}

#[test]
fn agent_changes_are_not_logged_unless_configured() {
    let repo = TestRepo::with_project();
    let mut cmd = repo.command(&["req", "add", "FR-200", "--text", "By an agent"]);
    cmd.env("RESONANCE_ACTOR", "agent:claude-code");
    assert!(cmd.output().unwrap().status.success());
    let out = mcp(
        &repo,
        &[(
            "resonance_create_requirement",
            json!({"key": "FR-201", "text": "Also by an agent"}),
        )],
    );
    assert_eq!(out[0]["result"]["isError"], false, "{out:?}");
    assert!(entries(&repo).is_empty(), "{:?}", entries(&repo));
    let show = repo.json(&["show", "FR-200"]);
    assert!(show.to_string().contains("FR-200"));

    // With changes.includeAgents, they are.
    let cfg = repo.read(".resonance/config.toml");
    assert!(cfg.contains("includeAgents = false"), "{cfg}");
    repo.write(
        ".resonance/config.toml",
        &cfg.replace("includeAgents = false", "includeAgents = true"),
    );
    let mut cmd = repo.command(&["req", "edit", "FR-200", "--text", "Edited by an agent"]);
    cmd.env("RESONANCE_ACTOR", "agent:claude-code");
    assert!(cmd.output().unwrap().status.success());
    let e = entries(&repo);
    assert_eq!(keys(&e), ["FR-200"]);
    assert_eq!(e[0]["actor"], "agent:claude-code");

    // Agents read and clear the log through MCP.
    let seq = e[0]["seq"].as_u64().unwrap();
    let out = mcp(
        &repo,
        &[
            ("resonance_changes", json!({})),
            (
                "resonance_changes_done",
                json!({"seqs": [seq], "note": "seen"}),
            ),
        ],
    );
    let text = out[0]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("FR-200"), "{text}");
    assert!(entries(&repo).is_empty());
}

#[test]
fn source_document_edits_are_logged_as_imports() {
    let repo = TestRepo::with_project();
    repo.ok(&["import", "spec/requirements.md"]);
    let initial = entries(&repo);
    assert!(
        keys(&initial).contains(&"FR-001".to_string()),
        "{initial:?}"
    );
    let all: Vec<String> = initial.iter().map(|e| e["seq"].to_string()).collect();
    let mut args = vec!["changes", "done"];
    args.extend(all.iter().map(String::as_str));
    repo.ok(&args);

    repo.replace(
        "spec/requirements.md",
        "The store opens a database at a path.",
        "The store opens a database at a given path.",
    );
    let e = entries(&repo);
    assert_eq!(keys(&e), ["FR-001"]);
    assert_eq!(e[0]["origin"], "import");
    assert_eq!(e[0]["kind"], "edited");
    assert_eq!(e[0]["fields"], json!(["text"]));
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
    mate.ok(&["req", "add", "FR-300", "--text", "From a teammate"]);
    mate.write("MATE.md", "m\n");
    mate.git(&["add", "-A"]);
    mate.git(&["commit", "-q", "-m", "mate"]);
    mate.git(&["push", "-q", "origin", "main"]);

    let before: Vec<String> = entries(&repo)
        .iter()
        .map(|e| e["seq"].to_string())
        .collect();
    if !before.is_empty() {
        let mut args = vec!["changes", "done"];
        args.extend(before.iter().map(String::as_str));
        repo.ok(&args);
    }
    repo.git(&["pull", "-q", "--no-rebase", "origin", "main"]);
    let e = entries(&repo);
    assert_eq!(keys(&e), ["FR-300"], "{e:?}");
    assert_eq!(e[0]["origin"], "git");
    assert_eq!(e[0]["actor"], "human:mate@example.com");
}
