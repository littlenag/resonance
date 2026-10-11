//! S1 (offline form): AC1 no file modified, AC2 files and imported requirements present and
//! nothing parsed, AC3 builds unaffected, AC4 deleting `.resonance/` leaves a working project,
//! AC5 / NFR-041 `git status` unchanged. Also MVP-009, FR-045/FR-046 and MVP-016.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use common::{SPEC, STORE, TestRepo};

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
    assert!(repo.path().join(".resonance/graph.db").is_file());
    assert!(repo.path().join(".resonance/config.toml").is_file());
    let exclude = fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap();
    assert!(exclude.lines().any(|l| l == "/.resonance/"), "FR-025");

    // Re-running init changes nothing.
    let again = repo.ok(&["init", "--no-agent"]);
    assert!(again.contains("already set up"), "{again}");
    let exclude2 = fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap();
    assert_eq!(exclude, exclude2);
    assert_eq!(repo.git_status(), status_before);
}

#[test]
fn the_graph_holds_files_and_imported_requirements_but_parses_no_code() {
    let repo = project();
    repo.write("secret.env", "TOKEN=1\n");
    repo.write("target/debug/app", "binary output\n");
    repo.write("web/node_modules/lib/index.js", "module.exports = 1;\n");
    repo.write("src/.store.rs.swp", "vim swap\n");
    repo.ok(&["init", "--no-agent"]);
    repo.ok(&["import", "spec/requirements.md"]);
    let status = repo.json(&["status"]);
    let counts = &status["counts"];
    // README.md, .gitignore, spec/requirements.md, src/store.rs (FR-045, FR-046, .gitignore).
    assert_eq!(counts["files"], 4, "{status}");
    assert_eq!(
        counts["requirements"], 4,
        "AC2: FR-001..FR-003 and NFR-001 imported"
    );
    assert_eq!(
        counts["codeUnits"], 0,
        "AC2: nothing parsed; code units are declared"
    );
    let file = repo.json(&["show", "src/store.rs"]);
    assert_eq!(file["node"]["type"], "File");
    for ignored in [
        "secret.env",
        "target/debug/app",
        "web/node_modules/lib/index.js",
        "src/.store.rs.swp",
    ] {
        let out = repo.run(&["show", ignored]);
        assert!(!out.status.success(), "{ignored} must not be ingested");
    }
}

#[test]
fn deleting_the_resonance_directory_leaves_a_working_project() {
    let repo = project();
    let status_before = repo.git_status();
    let build_before = build(&repo);
    repo.ok(&["init", "--no-agent"]);
    repo.ok(&["import", "spec/requirements.md"]);
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
    let png = repo.json(&["show", "assets/logo.png"]);
    assert_eq!(png["properties"]["stored"], false, "MVP-016: binary");
    let config = repo
        .read(".resonance/config.toml")
        .replace("maxBlobBytes = 5242880", "maxBlobBytes = 1024");
    repo.write(".resonance/config.toml", &config);
    repo.write(".resonance-test-big.txt", &"y".repeat(2048));
    let big = repo.json(&["show", ".resonance-test-big.txt"]);
    assert_eq!(
        big["properties"]["stored"], false,
        "MVP-016: larger than ingest.maxBlobBytes"
    );
    assert_eq!(big["properties"]["size"], 2048);
    let err = repo.fails(&["unit", "add", "assets/logo.png:1-1", "--name", "logo"], 1);
    assert!(err.contains("not UTF-8 text"), "{err}");
}

#[test]
fn commands_before_init_and_usage_errors_have_distinct_exit_codes() {
    let repo = project();
    let err = repo.fails(&["trace", "FR-001"], 1);
    assert!(err.contains("resonance init"), "{err}");
    repo.ok(&["init", "--no-agent"]);
    repo.fails(&["link", "only-one-arg"], 2);
    repo.fails(&["no-such-command"], 2);
    repo.fails(&["--json", "show", "nothing-by-this-name"], 1);
}
