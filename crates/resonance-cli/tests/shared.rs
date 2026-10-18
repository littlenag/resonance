//! The shared graph and opting in (B6, mvp.md §5 "Shared graph"): sharing (S16), joining and
//! fresh clones (S12), merging branches that both changed the graph (S11), hold-outs (S17),
//! consistency checks and bypasses (S24), compaction (S19), FR-012b conflicts, and stepping
//! up and down (S20).

mod common;

use std::fs;
use std::path::Path;

use common::TestRepo;
use serde_json::Value;

/// A bare remote, plus `origin` configured in `repo`.
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
    repo.ok(&[
        "annotate",
        "open",
        "--kind",
        "instruction",
        "--text",
        "keep the signature",
    ]);
    let bare = with_remote(&repo);
    repo.ok(&["share", "--yes"]);
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "share the graph"]);
    repo.git(&["push", "-q", "-u", "origin", "main"]);
    (repo, bare)
}

fn annotations_on(repo: &TestRepo, unit: &str) -> Vec<String> {
    let mut v: Vec<String> = repo.json(&["why", unit])["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["text"].as_str().unwrap_or_default().to_string())
        .collect();
    v.sort();
    v
}

fn tracked(repo: &TestRepo) -> Vec<String> {
    repo.git(&["ls-files", "--", ".resonance"])
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn share_previews_then_leaves_new_files_to_commit() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.ok(&[
        "annotate",
        "open",
        "--kind",
        "note",
        "--text",
        "shared note",
    ]);
    let private = repo.json(&[
        "annotate",
        "close",
        "--kind",
        "note",
        "--text",
        "private note",
    ]);
    let private_id = private["id"].as_str().unwrap().to_string();

    let plan = repo.json(&["share", "--dry-run", "--keep-local", &private_id]);
    assert_eq!(plan["applied"], false);
    assert!(plan["annotations"].as_array().unwrap().len() >= 2, "{plan}");
    assert_eq!(repo.git_status(), "", "a preview changes nothing");
    repo.fails(&["share"], 1);

    let done = repo.json(&["share", "--yes", "--keep-local", &private_id]);
    assert_eq!(done["applied"], true);
    let status = repo.git_status();
    assert!(status.contains(".resonance/"), "{status}");
    repo.git(&["add", "-A"]);
    let staged = repo.git(&["diff", "--cached", "--name-only"]);
    for f in [
        ".resonance/graph.db",
        ".resonance/config.toml",
        ".resonance/README.md",
        ".resonance/.gitignore",
        ".resonance/.gitattributes",
    ] {
        assert!(staged.contains(f), "{f} in {staged}");
    }
    assert!(
        !staged.contains(".resonance/local/"),
        "local state is never committed"
    );
    let config = repo.read(".resonance/config.toml");
    assert!(config.contains("stage = 2"), "{config}");
    assert!(repo.path().join(".git/hooks/pre-commit").is_file());
    let st = repo.json(&["status"]);
    assert_eq!(st["stage"], "2");
    assert_eq!(st["participation"], "user");
    // The private annotation stays, but only here (FR-187, NFR-012).
    assert_eq!(annotations_on(&repo, "close"), ["private note"]);
    assert!(
        fs::read_dir(repo.path().join(".resonance/local/private"))
            .unwrap()
            .next()
            .is_some()
    );
}

#[test]
fn a_fresh_clone_joins_and_rebuilds_the_graph_from_git() {
    let (repo, bare) = shared_repo();
    let mate = TestRepo::clone_of(bare.path());
    let out = mate.ok(&["init", "--no-agent"]);
    assert!(out.contains("Joined"), "{out}");
    assert_eq!(annotations_on(&mate, "open"), ["keep the signature"]);
    assert_eq!(
        mate.json(&["trace", "FR-001"])["satisfiedBy"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(mate.json(&["status"])["participation"], "user");
    assert_eq!(mate.git_status(), "", "joining changes no tracked file");
    let _ = repo;
}

#[test]
fn branches_that_both_change_the_graph_merge_without_conflicts() {
    let (repo, bare) = shared_repo();
    let mate = TestRepo::clone_of(bare.path());
    mate.ok(&["init", "--no-agent"]);
    mate.git(&["checkout", "-q", "-b", "feature"]);
    mate.ok(&[
        "annotate",
        "close",
        "--kind",
        "note",
        "--text",
        "from the feature",
    ]);
    mate.write("FEATURE.md", "f\n");
    mate.git(&["add", "-A"]);
    mate.git(&["commit", "-q", "-m", "feature"]);
    assert!(
        mate.git(&["show", "--name-only", "--format=", "HEAD"])
            .contains(".resonance/deltas/"),
        "the pre-commit hook staged the open delta"
    );
    mate.git(&["push", "-q", "-u", "origin", "feature"]);

    repo.ok(&["annotate", "close", "--kind", "note", "--text", "from main"]);
    repo.write("MAIN.md", "m\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "main work"]);
    repo.git(&["fetch", "-q", "origin"]);
    let merge = repo.git_output(&["merge", "-q", "--no-edit", "origin/feature"]);
    assert!(
        merge.status.success(),
        "{}",
        String::from_utf8_lossy(&merge.stderr)
    );
    assert!(
        repo.git(&["diff", "--name-only", "--diff-filter=U"])
            .is_empty()
    );
    assert_eq!(
        annotations_on(&repo, "close"),
        ["from main", "from the feature"]
    );
    let check = repo.json(&["check", "--range", "origin/main..HEAD"]);
    assert_eq!(check["errors"], 0, "{check}");
}

#[test]
fn a_hold_outs_commits_are_ingested_and_never_blocked() {
    let (repo, bare) = shared_repo();
    let holdout = TestRepo::clone_of(bare.path());
    // Never runs Resonance: plain git only (FR-174).
    holdout.replace("src/store.rs", "conn.check();", "conn.verify();");
    holdout.commit_all("hold-out change");
    holdout.git(&["push", "-q", "origin", "main"]);
    repo.git(&["pull", "-q", "--no-rebase", "origin", "main"]);
    let value = repo.json(&["value"]);
    assert!(
        value["divergences"]["C2"].as_u64().unwrap_or(0) >= 1,
        "{value}"
    );
    let review = repo.json(&["review", "HEAD~1..HEAD"]);
    assert_eq!(
        review["filesOutsideResonance"],
        serde_json::json!(["src/store.rs"])
    );
    let check = repo.json(&["check", "--range", "HEAD~1..HEAD"]);
    assert_eq!(
        check["errors"], 0,
        "a teammate's commit is information, not an error: {check}"
    );
}

#[test]
fn inconsistent_commits_are_blocked_and_bypasses_caught_before_push() {
    let (repo, _bare) = shared_repo();
    // R003: local state staged by force is refused by the pre-commit hook.
    fs::write(repo.path().join(".resonance/local/note.txt"), "x").unwrap();
    repo.git(&["add", "-f", ".resonance/local/note.txt"]);
    let out = repo.git_output(&["commit", "-q", "-m", "oops"]);
    assert!(!out.status.success(), "the commit is blocked");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("R003"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    repo.git(&["restore", "--staged", ".resonance/local/note.txt"]);

    // S24: a bypassed commit is caught by pre-push and repaired with a follow-up commit.
    repo.replace("src/store.rs", "drop(store);", "drop(store); // closed");
    repo.ok(&["status"]);
    repo.git(&["add", "src/store.rs"]);
    repo.git(&["commit", "-q", "--no-verify", "-m", "bypass"]);
    let push = repo.git_output(&["push", "-q", "origin", "main"]);
    assert!(!push.status.success(), "the push is blocked");
    assert!(
        String::from_utf8_lossy(&push.stderr).contains("R002"),
        "{}",
        String::from_utf8_lossy(&push.stderr)
    );
    repo.git(&["add", ".resonance/deltas"]);
    repo.git(&[
        "commit",
        "-q",
        "-m",
        "the graph changes of the previous commit",
    ]);
    let push = repo.git_output(&["push", "-q", "origin", "main"]);
    assert!(
        push.status.success(),
        "{}",
        String::from_utf8_lossy(&push.stderr)
    );
}

#[test]
fn compaction_on_the_trunk_and_colliding_compactions() {
    let (repo, bare) = shared_repo();
    let mate = TestRepo::clone_of(bare.path());
    mate.ok(&["init", "--no-agent"]);
    // A branch in flight carries its own delta.
    mate.git(&["checkout", "-q", "-b", "topic"]);
    mate.ok(&["annotate", "open", "--kind", "note", "--text", "topic note"]);
    mate.write("T.md", "t\n");
    mate.git(&["add", "-A"]);
    mate.git(&["commit", "-q", "-m", "topic"]);
    mate.git(&["push", "-q", "-u", "origin", "topic"]);

    // The trunk accumulates deltas and compacts.
    for i in 0..2 {
        repo.ok(&[
            "annotate",
            "close",
            "--kind",
            "note",
            "--text",
            &format!("main {i}"),
        ]);
        repo.write(&format!("M{i}.md"), "m\n");
        repo.git(&["add", "-A"]);
        repo.git(&["commit", "-q", "-m", &format!("main {i}")]);
    }
    let c = repo.json(&["compact", "--commit"]);
    assert!(c["folded"].as_array().unwrap().len() >= 2, "{c}");
    let still = tracked(&repo);
    for id in c["folded"].as_array().unwrap() {
        let id = id.as_str().unwrap();
        assert!(
            !still.iter().any(|p| p.contains(id)),
            "{id} was folded: {still:?}"
        );
    }
    repo.git(&["push", "-q", "origin", "main"]);
    let off = mate.fails(&["compact"], 1);
    assert!(off.contains("trunk"), "{off}");

    // S19: the branch made before the compaction merges cleanly.
    repo.git(&["fetch", "-q", "origin"]);
    let merge = repo.git_output(&["merge", "-q", "--no-edit", "origin/topic"]);
    assert!(
        merge.status.success(),
        "{}",
        String::from_utf8_lossy(&merge.stderr)
    );
    assert!(annotations_on(&repo, "open").contains(&"topic note".to_string()));

    // Colliding compactions: both sides compact on main; the merge driver unions them.
    mate.git(&["checkout", "-q", "main"]);
    mate.git(&["pull", "-q", "--no-rebase", "origin", "main"]);
    mate.ok(&["annotate", "close", "--kind", "note", "--text", "mate"]);
    mate.write("X.md", "x\n");
    mate.git(&["add", "-A"]);
    mate.git(&["commit", "-q", "-m", "mate"]);
    mate.ok(&["compact", "--commit"]);
    repo.ok(&["annotate", "close", "--kind", "note", "--text", "mine"]);
    repo.write("Y.md", "y\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "mine"]);
    repo.ok(&["compact", "--commit"]);
    mate.git(&["push", "-q", "origin", "main"]);
    let pull = repo.git_output(&["pull", "-q", "--no-rebase", "--no-edit", "origin", "main"]);
    assert!(
        pull.status.success(),
        "{}",
        String::from_utf8_lossy(&pull.stderr)
    );
    let notes = annotations_on(&repo, "close");
    assert!(
        notes.contains(&"mate".to_string()) && notes.contains(&"mine".to_string()),
        "{notes:?}"
    );
}

#[test]
fn an_edit_made_while_resonance_was_down_is_restored_from_the_graph() {
    let (repo, _bare) = shared_repo();
    if common::direct_mode() {
        return; // A daemon's start-up reconcile is what finds such edits (FR-012b).
    }
    repo.ok(&["daemon", "--stop"]);
    repo.replace("src/store.rs", "conn.check();", "conn.unchecked();");
    let st = repo.json(&["recover"]);
    let c3 = st["divergences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["class"] == "C3")
        .cloned()
        .unwrap_or_else(|| panic!("{st}"));
    assert_eq!(c3["level"], "action-required");
    assert!(
        repo.read("src/store.rs").contains("conn.check();"),
        "restored from the graph"
    );
    let rec = c3["recovery"].as_str().unwrap().to_string();
    repo.ok(&["recover", "undo", &rec]);
    assert!(
        repo.read("src/store.rs").contains("conn.unchecked();"),
        "the edit is back"
    );
}

#[test]
fn leave_rejoin_and_unshare_lose_nothing() {
    let (repo, _bare) = shared_repo();
    repo.ok(&["leave"]);
    assert!(
        !fs::read_to_string(repo.path().join(".git/hooks/pre-commit"))
            .unwrap_or_default()
            .contains("resonance hook")
    );
    assert_eq!(repo.json(&["status"])["participation"], "offline");
    repo.ok(&["init", "--no-agent"]);
    assert_eq!(repo.json(&["status"])["participation"], "user");
    assert_eq!(annotations_on(&repo, "open"), ["keep the signature"]);

    let export = repo.path().join("..").join("unshare.jsonl");
    repo.ok(&["unshare", "--yes", "--out", export.to_str().unwrap()]);
    assert!(Path::new(&export).is_file());
    let staged = repo.git(&["diff", "--cached", "--name-status"]);
    assert!(staged.contains("D\t.resonance/graph.db"), "{staged}");
    repo.git(&["commit", "-q", "-m", "stop sharing"]);
    assert_eq!(repo.json(&["status"])["stage"], "1");
    assert_eq!(annotations_on(&repo, "open"), ["keep the signature"]);
    assert_eq!(repo.git_status(), "", "offline again: invisible");
    let _ = fs::remove_file(export);
}

#[test]
fn deltas_render_for_people() {
    let (repo, _bare) = shared_repo();
    repo.ok(&["annotate", "close", "--kind", "note", "--text", "readable"]);
    repo.write("R.md", "r\n");
    repo.git(&["add", "-A"]);
    repo.git(&["commit", "-q", "-m", "r"]);
    let file = repo
        .git(&["show", "--name-only", "--format=", "HEAD"])
        .lines()
        .find(|l| l.ends_with(".delta"))
        .unwrap()
        .to_string();
    let text = repo.ok(&["show-delta", &file]);
    assert!(
        text.contains("Resonance delta") && text.contains("readable"),
        "{text}"
    );
    let diff = repo.git(&["show", "HEAD", "--", &file]);
    assert!(
        diff.contains("Resonance delta"),
        "the textconv driver renders deltas in git: {diff}"
    );
    let review = repo.ok(&["review", "HEAD~1..HEAD", "--markdown"]);
    assert!(review.contains("### Resonance graph changes"), "{review}");
    assert!(review.contains("readable"), "{review}");
    let v: Value = repo.json(&["review", "HEAD~1..HEAD"]);
    assert_eq!(v["annotations"].as_array().unwrap().len(), 1);
}
