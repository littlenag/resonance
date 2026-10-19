//! S15: AC1 `init` modifies no tracked file and explains offline mode, AC2 every local feature
//! works, AC4 teammates see no trace, AC7 leaving is clean (`eject`). Also the agent kit that
//! `init` installs.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::TestRepo;
use serde_json::json;

#[test]
fn every_local_feature_works_and_leaves_no_trace() {
    let repo = TestRepo::with_project();
    let tracked_before = repo.git(&["ls-files", "-s"]);
    repo.declare_sample();
    repo.annotate("open", "warning", "callers depend on this");
    repo.tool(
        "resonance_create_decision",
        json!({"title": "Plain files", "decision": "Keep the store in one file"}),
    );
    repo.tool("resonance_test", json!({"ref": "FR-001"}));
    repo.tool("resonance_context", json!({"requirements": ["FR-001"]}));
    repo.tool("resonance_search", json!({"text": "store"}));

    // AC4: nothing visible to git or teammates.
    assert_eq!(repo.git_status(), "", "the working tree looks untouched");
    repo.write("src/feature.rs", "pub fn feature() {}\n");
    repo.git(&["add", "-A"]);
    let staged = repo.git(&["diff", "--cached", "--name-only"]);
    assert_eq!(
        staged.trim(),
        "src/feature.rs",
        "`git add -A` never stages .resonance/"
    );
    repo.git(&["commit", "-q", "-m", "feature"]);
    let log = repo.git(&["log", "--format=%B", "-1"]);
    assert_eq!(log.trim(), "feature");
    let files = repo.git(&["ls-files"]);
    assert!(!files.contains(".resonance"), "{files}");
    // Tracked files are byte-for-byte what they were, apart from the new one.
    let tracked_after = repo.git(&["ls-files", "-s"]);
    let unchanged: Vec<&str> = tracked_after
        .lines()
        .filter(|l| !l.ends_with("src/feature.rs"))
        .collect();
    assert_eq!(unchanged.join("\n"), tracked_before.trim_end());
}

#[test]
fn eject_exports_first_then_removes_everything_it_added() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "keep me");
    let exclude_path = repo.path().join(".git/info/exclude");
    let exclude_before_init = fs::read_to_string(&exclude_path)
        .unwrap()
        .replace("/.resonance/\n", "");

    // A dry run changes nothing.
    let plan = repo.ok(&["eject", "--dry-run"]);
    assert!(plan.contains("will:"), "{plan}");
    assert!(repo.path().join(".resonance").exists());

    // Without a terminal and without --yes, eject exports but does not remove.
    let out = repo.run(&["eject"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(repo.path().join(".resonance").exists());

    let out = repo.ok(&["--json", "eject", "--yes"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let export = std::path::PathBuf::from(v["export"].as_str().unwrap());
    assert!(
        export.starts_with(repo.home().join(".resonance/exports")),
        "{export:?}"
    );
    let lines: Vec<serde_json::Value> = fs::read_to_string(&export)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["format"], "resonance-export");
    let kinds: Vec<&str> = lines[1..]
        .iter()
        .map(|l| l["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"node.create") && kinds.contains(&"edge.add"));
    assert!(
        lines
            .iter()
            .any(|l| l["payload"]["props"]["text"] == "keep me"),
        "the annotation is in the export"
    );
    assert!(
        lines
            .iter()
            .any(|l| l["payload"]["props"]["anchor"]["quote"]
                .as_str()
                .is_some_and(|q| q.contains("pub fn open"))),
        "anchors (identity data) are in the export"
    );

    // AC7: .resonance/ and the exclusion are gone; nothing else changed.
    assert!(!repo.path().join(".resonance").exists());
    assert_eq!(
        fs::read_to_string(&exclude_path).unwrap(),
        exclude_before_init
    );
    assert_eq!(repo.git_status(), "");
}

#[test]
fn export_is_engine_neutral_json_lines() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let out = repo.ok(&["export"]);
    let mut lines = out.lines();
    let header: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(header["formatVersion"], 1);
    for line in lines {
        let op: serde_json::Value = serde_json::from_str(line).unwrap();
        for field in [
            "opId",
            "kind",
            "actor",
            "hlc",
            "changeSet",
            "schema",
            "payload",
        ] {
            assert!(
                op.get(field).is_some(),
                "§9.1 envelope field {field} missing: {line}"
            );
        }
        assert!(
            !line.contains("sqlite") && !line.contains("rowid"),
            "{line}"
        );
    }
    repo.ok(&["export", "--out", "../graph-export.jsonl"]);
    assert!(
        repo.path()
            .parent()
            .unwrap()
            .join("graph-export.jsonl")
            .is_file()
    );
}

fn fake_claude(dir: &std::path::Path, log: &std::path::Path) {
    let script = format!(
        "#!/bin/sh\necho \"$PWD|$*\" >> '{}'\necho 'Added stdio MCP server resonance'\n",
        log.display()
    );
    let path = dir.join("claude");
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn init_installs_the_agent_kit_and_registers_mcp_at_local_scope() {
    let mut repo = TestRepo::new();
    repo.write("src/lib.rs", "pub fn f() {}\n");
    repo.commit_all("code");
    let bin_dir = tempfile::tempdir().unwrap();
    let log = bin_dir.path().join("calls.log");
    fake_claude(bin_dir.path(), &log);
    repo.with_path(bin_dir.path().to_path_buf());

    let out = repo.ok(&["init"]);
    assert!(out.contains("Registered"), "{out}");
    assert!(out.contains("hook post-checkout: created"), "{out}");
    let call = fs::read_to_string(&log).unwrap();
    let (cwd, args) = call.trim().split_once('|').unwrap();
    assert_eq!(
        fs::canonicalize(cwd).unwrap(),
        fs::canonicalize(repo.path()).unwrap()
    );
    assert!(
        args.starts_with("mcp add --scope local resonance -- /"),
        "{args}"
    );
    assert!(args.ends_with(" mcp"), "{args}");
    assert!(repo.path().join("CLAUDE.local.md").is_file());
    assert!(
        repo.home()
            .join(".claude/skills/resonance/SKILL.md")
            .is_file()
    );
    assert_eq!(repo.git_status(), "", "FR-263: nothing tracked changes");

    // Running init again refreshes, changing nothing.
    let again = repo.ok(&["init"]);
    assert!(again.contains("already set up"), "{again}");
    assert_eq!(repo.git_status(), "");
}

#[test]
fn init_without_claude_prints_the_registration_command() {
    let mut repo = TestRepo::new();
    let empty = tempfile::tempdir().unwrap();
    repo.with_path(empty.path().to_path_buf());
    // Keep only system directories on PATH so that a real `claude` is not found.
    let mut cmd = repo.command(&["init"]);
    cmd.env("PATH", format!("{}:/usr/bin:/bin", empty.path().display()));
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("was not found") && text.contains("claude mcp add --scope local resonance"),
        "{text}"
    );
}

#[test]
fn generic_agents_get_a_section_and_the_mcp_command() {
    let repo = TestRepo::new();
    let out = repo.ok(&["init", "--agent", "generic"]);
    assert!(out.contains("AGENTS.md") && out.contains(" mcp"), "{out}");
    assert!(out.contains("<!-- >>> resonance >>> -->"), "{out}");
    assert!(
        !repo.path().join("AGENTS.md").exists(),
        "offline, printed only"
    );
}
