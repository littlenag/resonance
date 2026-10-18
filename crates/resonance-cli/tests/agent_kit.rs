//! The agent kit (B7, mvp.md §5 "Agent kit (B7, S22)"): `agent install` writes the skill, the
//! harness hooks and the instructions section for the stage; the agent hook denies `git commit`
//! while checks fail or change-log entries are pending, and allows it with the developer's
//! override; `resonance_conventions` differs by stage and participation; `status` and `doctor`
//! report a missing or stale kit; the orientation is available through MCP.

mod common;

use std::fs;
use std::io::Write;
use std::process::Stdio;

use common::TestRepo;
use serde_json::{Value, json};

fn with_remote(repo: &TestRepo) -> tempfile::TempDir {
    let bare = tempfile::tempdir().unwrap();
    let out = std::process::Command::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(bare.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    repo.git(&["remote", "add", "origin", bare.path().to_str().unwrap()]);
    bare
}

fn shared_repo() -> (TestRepo, tempfile::TempDir) {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let bare = with_remote(&repo);
    repo.ok(&["share", "--yes"]);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "share the graph"]);
    repo.git(&["push", "-q", "-u", "origin", "main"]);
    (repo, bare)
}

/// Call one MCP tool as an agent; returns the initialize result and the tool's data.
fn mcp(repo: &TestRepo, name: &str, args: Value) -> (Value, Value) {
    let mut child = repo
        .command(&["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for l in [
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "clientInfo": {"name": "test-agent"}}}),
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": name, "arguments": args}}),
    ] {
        writeln!(stdin, "{l}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    let lines: Vec<Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let text = lines[1]["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(lines[1]["result"]["isError"], false, "{text}");
    let body: Value = serde_json::from_str(text).unwrap();
    (lines[0]["result"].clone(), body["data"].clone())
}

/// Handle every pending change-log entry.
fn clear_log(repo: &TestRepo) {
    let seqs: Vec<String> = repo
        .json(&["changes"])
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["seq"].to_string())
        .collect();
    if !seqs.is_empty() {
        let mut args = vec!["changes", "done"];
        args.extend(seqs.iter().map(String::as_str));
        repo.ok(&args);
    }
}

/// Run the harness hook as Claude Code would, with the agent's shell command.
fn gate(repo: &TestRepo, command: &str) -> String {
    let mut child = repo
        .command(&["agent-hook", "pre-git"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
        "tool_input": {"command": command}});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn install_writes_the_offline_kit_locally() {
    let repo = TestRepo::with_project();
    assert_eq!(repo.json(&["status"])["agentKit"]["installed"], false);
    assert!(repo.ok(&["status"]).contains("Agent kit: not installed"));
    assert!(repo.ok(&["doctor"]).contains("Agent kit: not installed"));
    repo.write(
        ".claude/settings.local.json",
        "{\"permissions\": {\"allow\": [\"Bash(ls:*)\"]}}\n",
    );

    let dry = repo.json(&["agent", "install", "--dry-run", "--no-mcp"]);
    assert_eq!(dry["dryRun"], true);
    assert!(
        !repo.path().join("CLAUDE.local.md").exists(),
        "a dry run writes nothing"
    );
    repo.fails(&["agent", "install", "--no-mcp"], 1);

    let out = repo.json(&["agent", "install", "--yes", "--no-mcp"]);
    assert_eq!(out["harness"], "claude-code");
    assert_eq!(out["stage"], 1);
    let skill = fs::read_to_string(repo.home().join(".claude/skills/resonance/SKILL.md")).unwrap();
    assert!(skill.starts_with("---\nname: resonance"), "{skill}");
    assert!(skill.contains("Never edit anything under `.resonance/`"));

    let settings: Value = serde_json::from_str(&repo.read(".claude/settings.local.json")).unwrap();
    assert_eq!(
        settings["permissions"]["allow"][0], "Bash(ls:*)",
        "kept: {settings}"
    );
    assert_eq!(settings["env"]["RESONANCE_ACTOR"], "agent:claude-code");
    let hook = &settings["hooks"]["PreToolUse"][0];
    assert_eq!(hook["matcher"], "Bash");
    assert!(
        hook["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .ends_with(" agent-hook pre-git"),
        "{settings}"
    );
    let section = repo.read("CLAUDE.local.md");
    assert!(section.contains("<!-- >>> resonance >>> -->"), "{section}");
    assert!(
        !repo.path().join("CLAUDE.md").exists(),
        "offline, nothing shared (FR-132)"
    );
    assert_eq!(repo.git_status(), "", "local files are excluded from git");
    assert!(
        repo.path()
            .join(".resonance/local/agent-kit.json")
            .is_file()
    );

    // Idempotent.
    let again = repo.json(&["agent", "install", "--yes", "--no-mcp"]);
    assert!(
        again["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["action"] == "unchanged"),
        "{again}"
    );
    let settings: Value = serde_json::from_str(&repo.read(".claude/settings.local.json")).unwrap();
    assert_eq!(settings["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);

    let st = repo.json(&["agent", "status"]);
    assert_eq!(st["kit"]["installed"], true);
    assert_eq!(st["kit"]["stale"], false);
    assert!(repo.ok(&["doctor"]).contains("Agent kit: "));

    // Stale: installed by another version.
    let path = repo.path().join(".resonance/local/agent-kit.json");
    let text = fs::read_to_string(&path).unwrap();
    let mut rec: Value = serde_json::from_str(&text).unwrap();
    rec["version"] = json!("0.0.1");
    fs::write(&path, rec.to_string()).unwrap();
    assert_eq!(repo.json(&["status"])["agentKit"]["stale"], true);
    let check = repo.json(&["check", "--staged"]);
    assert!(check.to_string().contains("R010"), "{check}");
    assert!(repo.ok(&["doctor"]).contains("stale"));
    repo.ok(&["agent", "update"]);
    assert_eq!(repo.json(&["status"])["agentKit"]["stale"], false);

    let err = repo.fails(&["agent", "install", "--yes", "--shared"], 1);
    assert!(err.contains("stage 2"), "{err}");
}

#[test]
fn generic_agents_get_a_section_and_the_mcp_command() {
    let repo = TestRepo::with_project();
    let out = repo.ok(&["agent", "install", "--generic", "--yes"]);
    assert!(out.contains("AGENTS.md") && out.contains(" mcp"), "{out}");
    assert!(out.contains("<!-- >>> resonance >>> -->"), "{out}");
    assert!(
        !repo.path().join("AGENTS.md").exists(),
        "offline, printed only"
    );
}

#[test]
fn conventions_differ_by_stage_and_participation() {
    let repo = TestRepo::with_project();
    let (init, conv) = mcp(&repo, "resonance_conventions", json!({}));
    let offline = init["instructions"].as_str().unwrap();
    assert!(offline.contains("Stay invisible"), "{offline}");
    assert_eq!(conv["stage"], 1);
    assert!(
        conv["orientation"].as_array().unwrap().len() >= 3,
        "FR-225: {conv}"
    );
    assert!(
        conv["orientation"].to_string().contains("Offline cannot"),
        "{conv}"
    );

    let (repo, bare) = {
        drop(repo);
        shared_repo()
    };
    let (init, conv) = mcp(&repo, "resonance_conventions", json!({}));
    let shared = init["instructions"].as_str().unwrap();
    assert!(shared.contains("Before every commit"), "{shared}");
    assert!(!shared.contains("Stay invisible"));
    assert_eq!(conv["optedIn"], true);

    // A teammate trying Resonance offline over the shared graph has not opted in.
    let mate = TestRepo::clone_of(bare.path());
    mate.ok(&["init", "--offline", "--no-agent"]);
    let (init, conv) = mcp(&mate, "resonance_conventions", json!({}));
    assert!(
        init["instructions"]
            .as_str()
            .unwrap()
            .contains("has not opted in"),
        "{conv}"
    );
    assert_eq!(conv["optedIn"], false);
}

#[test]
fn the_agent_gate_holds_commits_until_the_log_is_empty_or_overridden() {
    let (repo, _bare) = shared_repo();
    repo.ok(&["agent", "install", "--yes", "--no-mcp"]);
    clear_log(&repo);
    assert_eq!(gate(&repo, "git status && git log"), "", "not gated");

    // A person changed the spec: the log has an entry.
    repo.ok(&["req", "add", "FR-050", "--text", "Exports are signed"]);
    repo.write("NOTES.md", "n\n");
    repo.git(&["add", "NOTES.md"]);
    let out = gate(&repo, "git add -A && git commit -m notes");
    let d: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    assert_eq!(d["hookSpecificOutput"]["permissionDecision"], "deny");
    let reason = d["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(
        reason.contains("R009") && reason.contains("resonance changes"),
        "{reason}"
    );
    assert!(gate(&repo, "git pull").contains("deny"));
    assert_eq!(
        gate(&repo, "git merge feature"),
        "",
        "merges wait only on divergences"
    );

    // The developer says to proceed: allowed, and counted.
    assert_eq!(gate(&repo, "RESONANCE_OVERRIDE=1 git commit -m notes"), "");

    // Handling the entry clears the gate.
    clear_log(&repo);
    assert_eq!(gate(&repo, "git commit -m notes"), "");
    let v = repo.json(&["value"]);
    assert_eq!(v["agent"]["denials"], 2, "{v}");
    assert_eq!(v["agent"]["overrides"], 1, "{v}");
    assert_eq!(v["agent"]["commitsChecked"], 2, "{v}");
    assert_eq!(
        v["agent"]["firstPass"], 1,
        "the commit after the override passed at once: {v}"
    );
}

#[test]
fn agent_cli_changes_carry_the_agent_identity() {
    let repo = TestRepo::with_project();
    let mut cmd = repo.command(&["--json", "req", "add", "FR-060", "--text", "By the agent"]);
    cmd.env("RESONANCE_ACTOR", "agent:claude-code");
    assert!(cmd.output().unwrap().status.success());
    let show = repo.json(&["changes"]);
    assert!(
        show.as_array().unwrap().is_empty(),
        "agents are not logged by default"
    );
}

#[test]
fn leaving_removes_the_kit_but_keeps_other_settings() {
    let (repo, _bare) = shared_repo();
    repo.write(
        ".claude/settings.local.json",
        "{\"permissions\": {\"allow\": [\"Bash(ls:*)\"]}}\n",
    );
    repo.ok(&["agent", "install", "--yes", "--no-mcp"]);
    let rec: Value = serde_json::from_str(&repo.read(".resonance/local/agent-kit.json")).unwrap();
    assert_eq!(rec["stage"], 2);
    assert_eq!(rec["participation"], "user");
    let left = repo.json(&["leave"]);
    assert!(!left["kit"].as_array().unwrap().is_empty(), "{left}");
    let settings: Value = serde_json::from_str(&repo.read(".claude/settings.local.json")).unwrap();
    assert!(
        settings.get("hooks").is_none() && settings.get("env").is_none(),
        "{settings}"
    );
    assert_eq!(settings["permissions"]["allow"][0], "Bash(ls:*)");
    assert!(!repo.path().join("CLAUDE.local.md").exists());
    assert!(!repo.path().join(".resonance/local/agent-kit.json").exists());
}
