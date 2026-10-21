//! S1 (offline form): AC1 no file modified, AC2 files present, hooks and the agent kit
//! installed, no requirements read and nothing parsed, AC3 builds unaffected, AC4 deleting
//! `.resonance/` leaves a working project, AC5 / NFR-041 `git status` unchanged. Also MVP-009,
//! FR-045/FR-046, MVP-016 and the operations-only CLI (FR-259).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use common::{SPEC, STORE, TestRepo};
use serde_json::json;

/// Every file outside `.git/` and `.resonance/`, with its bytes and modification time.
fn snapshot(root: &Path) -> BTreeMap<String, (Vec<u8>, SystemTime)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let p = entry.path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            if rel == ".git" || rel == ".resonance" {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else {
                let md = fs::metadata(&p).unwrap();
                out.insert(rel, (fs::read(&p).unwrap(), md.modified().unwrap()));
            }
        }
    }
    out
}

/// A stand-in for a build: deterministic output derived from the sources.
fn build(repo: &TestRepo) -> String {
    let out = std::process::Command::new("sh")
        .args(["-c", "cat src/*.rs spec/*.md | cksum && ls -R src spec"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn project() -> TestRepo {
    let repo = TestRepo::new();
    repo.write("spec/requirements.md", SPEC);
    repo.write("src/store.rs", STORE);
    repo.write(".gitignore", "secret.env\n");
    repo.commit_all("project");
    repo
}

#[test]
fn init_modifies_no_file_and_leaves_git_status_unchanged() {
    let repo = project();
    repo.write("notes.txt", "untracked work in progress\n");
    let status_before = repo.git_status();
    let files_before = snapshot(repo.path());
    let build_before = build(&repo);

    let out = repo.ok(&["init", "--no-agent"]);
    assert!(out.contains("offline mode"), "{out}");
    assert!(
        out.contains("What offline mode cannot do"),
        "explains offline mode (FR-180): {out}"
    );

    assert_eq!(
        snapshot(repo.path()),
        files_before,
        "AC1: no existing file modified"
    );
    assert_eq!(
        repo.git_status(),
        status_before,
        "AC5 / NFR-041: git status unchanged"
    );
    assert_eq!(build(&repo), build_before, "AC3: builds unaffected");
    assert!(repo.path().join(".resonance/local/state.db").is_file());
    assert!(repo.path().join(".resonance/config.toml").is_file());
    let exclude = fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap();
    assert!(exclude.lines().any(|l| l == "/.resonance/"), "FR-025");

    // The offline hooks are installed, and re-running init changes nothing visible.
    assert!(
        repo.path().join(".git/hooks/post-checkout").is_file(),
        "FR-069"
    );
    assert!(
        !repo.path().join(".git/hooks/pre-commit").exists(),
        "offline: no blocking hooks"
    );
    let again = repo.ok(&["init", "--no-agent"]);
    assert!(again.contains("already set up"), "{again}");
    let exclude2 = fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap();
    assert_eq!(exclude, exclude2);
    assert_eq!(repo.git_status(), status_before);
}

#[test]
fn the_graph_holds_files_but_no_requirements_and_parses_nothing() {
    let repo = project();
    repo.write("secret.env", "TOKEN=1\n");
    repo.write("target/debug/app", "binary output\n");
    repo.write("web/node_modules/lib/index.js", "module.exports = 1;\n");
    repo.write("src/.store.rs.swp", "vim swap\n");
    let out = repo.ok(&["init", "--no-agent"]);
    assert!(
        out.contains("No spec items yet") || out.contains("Requirements: 0"),
        "{out}"
    );
    let status = repo.json(&["status"]);
    let counts = &status["counts"];
    // README.md, .gitignore, spec/requirements.md, src/store.rs (FR-045, FR-046, .gitignore).
    assert_eq!(counts["files"], 4, "{status}");
    assert_eq!(
        counts["requirements"], 0,
        "AC2: init reads no requirements; the agent brings them in (S26)"
    );
    assert_eq!(
        counts["codeUnits"], 0,
        "AC2: nothing parsed; code units are declared"
    );
    let file = repo.tool("resonance_show", json!({"ref": "src/store.rs"}));
    assert_eq!(file["node"]["type"], "File");
    for ignored in [
        "secret.env",
        "target/debug/app",
        "web/node_modules/lib/index.js",
        "src/.store.rs.swp",
    ] {
        repo.tool_err("resonance_show", json!({"ref": ignored}));
    }
}

#[test]
fn deleting_the_resonance_directory_leaves_a_working_project() {
    let repo = project();
    let status_before = repo.git_status();
    let build_before = build(&repo);
    repo.ok(&["init", "--no-agent"]);
    repo.import_sample();
    fs::remove_dir_all(repo.path().join(".resonance")).unwrap();
    assert_eq!(repo.git_status(), status_before, "AC4");
    assert_eq!(build(&repo), build_before, "AC4");
    assert_eq!(repo.read("src/store.rs"), STORE);
    // And Resonance says plainly that it is not set up.
    let err = repo.fails(&["status"], 1);
    assert!(err.contains("resonance init"), "{err}");
}

#[test]
fn resonance_refuses_to_run_outside_a_git_repository() {
    let dir = tempfile::tempdir().unwrap();
    let out = std::process::Command::new(common::bin())
        .arg("status")
        .current_dir(dir.path())
        .env("GIT_CEILING_DIRECTORIES", dir.path().parent().unwrap())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("not inside a git repository"),
        "MVP-009: {err}"
    );
}

#[test]
fn large_and_binary_files_are_recorded_without_their_bytes() {
    let repo = project();
    repo.write_bytes("assets/logo.png", b"\x89PNG\r\n\x1a\n\x00\x00binary");
    repo.write(".resonance-test-big.txt", &"x".repeat(2048));
    repo.commit_all("assets");
    repo.ok(&["init", "--no-agent"]);
    let png = repo.tool("resonance_show", json!({"ref": "assets/logo.png"}));
    assert_eq!(png["properties"]["stored"], false, "MVP-016: binary");
    let config = repo
        .read(".resonance/config.toml")
        .replace("maxBlobBytes = 5242880", "maxBlobBytes = 1024");
    repo.write(".resonance/config.toml", &config);
    repo.write(".resonance-test-big.txt", &"y".repeat(2048));
    let big = repo.tool("resonance_show", json!({"ref": ".resonance-test-big.txt"}));
    assert_eq!(
        big["properties"]["stored"], false,
        "MVP-016: larger than ingest.maxBlobBytes"
    );
    assert_eq!(big["properties"]["size"], 2048);
    let err = repo.tool_err(
        "resonance_declare_unit",
        json!({"path": "assets/logo.png", "startLine": 1, "name": "logo"}),
    );
    assert!(err.contains("not UTF-8 text"), "{err}");
}

#[test]
fn commands_before_init_and_usage_errors_have_distinct_exit_codes() {
    let repo = project();
    let err = repo.fails(&["status"], 1);
    assert!(err.contains("resonance init"), "{err}");
    repo.ok(&["init", "--no-agent"]);
    repo.fails(&["check"], 2);
    repo.fails(&["no-such-command"], 2);
    repo.fails(&["recover", "undo", "nothing-by-this-id"], 1);
}

#[test]
fn the_cli_offers_operations_only() {
    // FR-259: graph work is MCP; the CLI sets up, runs, checks and repairs.
    let repo = project();
    let help = repo.ok(&["--help"]);
    for gone in [
        "import", "req", "decision", "unit", "link", "annotate", "trace", "why", "search", "show",
        "context", "changes", "review", "compact", "value", "agent ",
    ] {
        assert!(
            !help
                .lines()
                .any(|l| l.trim_start().starts_with(&format!("{gone} "))),
            "`{gone}` is not a CLI command: {help}"
        );
    }
    for kept in [
        "init", "status", "doctor", "tui", "daemon", "check", "recover", "share",
    ] {
        assert!(help.contains(kept), "{kept}: {help}");
    }
    repo.fails(&["import", "spec/requirements.md"], 2);
    repo.fails(&["trace", "FR-001"], 2);
}

/// Offline init installs the notification hooks only. A manager, or a hook that is not a
/// shell script, means `manual`: print the block and do not write the hook file (FR-177).
fn assert_hook_left_manual(repo: &TestRepo, manager_or_note: &str, absent: &str) {
    let out = repo.ok(&["init", "--no-agent"]);
    assert!(out.contains(manager_or_note), "{out}");
    assert!(
        out.lines()
            .any(|l| l.contains("post-checkout") && l.contains("manual")),
        "{out}"
    );
    assert!(
        !repo.path().join(absent).exists(),
        "init must not create {absent} while {manager_or_note} owns the hooks\n{out}"
    );
}

#[test]
fn a_hook_manager_is_left_for_its_owner() {
    // FR-177: husky keeps its hooks. Resonance prints the block to add.
    let repo = project();
    fs::create_dir_all(repo.path().join(".husky")).unwrap();
    fs::write(repo.path().join(".husky/pre-commit"), "#!/bin/sh\n").unwrap();
    assert_hook_left_manual(&repo, "husky", ".git/hooks/post-checkout");
}

#[test]
fn a_pre_commit_config_is_left_for_its_owner() {
    let repo = project();
    repo.write(".pre-commit-config.yaml", "repos: []\n");
    assert_hook_left_manual(&repo, "pre-commit", ".git/hooks/post-checkout");
}

#[test]
fn lefthook_yml_is_left_for_its_owner() {
    let repo = project();
    repo.write("lefthook.yml", "pre-commit:\n  commands: {}\n");
    assert_hook_left_manual(&repo, "lefthook", ".git/hooks/post-checkout");
}

#[test]
fn lefthook_yaml_is_left_for_its_owner() {
    let repo = project();
    repo.write("lefthook.yaml", "pre-commit:\n  commands: {}\n");
    assert_hook_left_manual(&repo, "lefthook", ".git/hooks/post-checkout");
}

#[test]
fn core_hooks_path_inside_the_work_tree_is_left_alone() {
    let repo = project();
    fs::create_dir_all(repo.path().join(".githooks")).unwrap();
    repo.git(&["config", "core.hooksPath", ".githooks"]);
    assert_hook_left_manual(&repo, "core.hooksPath", ".githooks/post-checkout");
    assert!(!repo.path().join(".git/hooks/post-checkout").exists());
}

#[test]
fn a_non_shell_hook_is_left_unchanged() {
    let repo = project();
    let hook = repo.path().join(".git/hooks/post-checkout");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    let body = "#!/usr/bin/env python3\nprint('keep')\n";
    fs::write(&hook, body).unwrap();
    let out = repo.ok(&["init", "--no-agent"]);
    assert!(
        out.lines()
            .any(|l| l.contains("post-checkout") && l.contains("manual")),
        "{out}"
    );
    assert!(out.contains("not a shell script"), "{out}");
    assert_eq!(fs::read_to_string(&hook).unwrap(), body);
}

#[test]
fn a_unit_in_a_file_past_max_blob_bytes_stays_attached() {
    // Declaring reads the file from disk, so a unit can be anchored in text that ingest
    // refuses to store. `resonance rebuild` re-checks anchors against stored blobs only
    // and is not asserted here.
    let repo = TestRepo::with_project();
    let config = repo
        .read(".resonance/config.toml")
        .replace("maxBlobBytes = 5242880", "maxBlobBytes = 64");
    repo.write(".resonance/config.toml", &config);
    let body = "pub fn wide() {\n    let value = 1;\n}\n".repeat(4);
    assert!(body.len() > 64, "{}", body.len());
    repo.write("src/wide.rs", &body);
    let file = repo.tool("resonance_show", json!({"ref": "src/wide.rs"}));
    assert_eq!(file["properties"]["stored"], false, "{file}");
    repo.declare("src/wide.rs:1-3", "wide");
    let show = repo.tool("resonance_show", json!({"ref": "wide"}));
    assert_eq!(show["location"]["path"], "src/wide.rs");
    assert_eq!(show["location"]["state"], "attached", "{show}");
}
