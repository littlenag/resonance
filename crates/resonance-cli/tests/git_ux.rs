//! Everyday git command sequences (FR-071, MVP-029, MVP-030).
//!
//! The map of which workflow lives in which test is docs/testing.md. Tests here
//! must pass both through a daemon and in direct mode.

mod common;

use common::{STORE, TestRepo};
use serde_json::json;

fn annotations_on(repo: &TestRepo, unit: &str) -> Vec<String> {
    let mut v: Vec<String> = repo.tool("resonance_why", json!({"ref": unit}))["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["text"].as_str().unwrap_or_default().to_string())
        .collect();
    v.sort();
    v
}

fn location(repo: &TestRepo, unit: &str) -> (String, String) {
    let show = repo.tool("resonance_show", json!({"ref": unit}));
    let l = &show["location"];
    (
        l["path"].as_str().unwrap().to_string(),
        l["state"].as_str().unwrap().to_string(),
    )
}

fn attached(repo: &TestRepo, unit: &str) {
    assert_eq!(
        location(repo, unit),
        ("src/store.rs".into(), "attached".into())
    );
}

/// A bare `origin`, plus a second clone that has joined the shared graph.
fn shared_pair() -> (TestRepo, TestRepo, tempfile::TempDir) {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "instruction", "keep the signature");
    let bare = tempfile::tempdir().unwrap();
    let out = std::process::Command::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(bare.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    repo.git(&["remote", "add", "origin", bare.path().to_str().unwrap()]);
    repo.ok(&["share", "--yes"]);
    repo.commit_all("share the graph");
    repo.git(&["push", "-q", "-u", "origin", "main"]);
    let mate = TestRepo::clone_of(bare.path());
    mate.ok(&["init", "--no-agent"]);
    (repo, mate, bare)
}

#[test]
fn pull_edit_commit_push_round_trip() {
    let (repo, mate, _bare) = shared_pair();
    mate.replace("src/store.rs", "conn.check();", "conn.check(); // pulled");
    mate.commit_all("mate edits open");
    mate.git(&["push", "-q", "origin", "main"]);

    repo.git(&["pull", "-q", "--ff-only", "origin", "main"]);
    assert!(
        repo.read("src/store.rs")
            .contains("conn.check(); // pulled")
    );
    assert_eq!(
        repo.git_status(),
        "",
        "a fast-forward pull of a covered file leaves no open delta"
    );
    attached(&repo, "open");
    assert_eq!(
        annotations_on(&repo, "open"),
        ["keep the signature"],
        "the pull keeps the shared annotation"
    );

    repo.annotate("open", "note", "after the pull");
    repo.write("AFTER.md", "a\n");
    repo.commit_all("local work after the pull");
    repo.git(&["push", "-q", "origin", "main"]);

    mate.git(&["pull", "-q", "--ff-only", "origin", "main"]);
    assert_eq!(mate.read("AFTER.md"), "a\n");
    assert_eq!(
        annotations_on(&mate, "open"),
        ["after the pull", "keep the signature"]
    );
    assert_eq!(repo.git_status(), "");
    assert_eq!(
        mate.git_status(),
        "",
        "the receiver of a fast-forward pull is clean too"
    );
}

#[test]
fn branch_edit_commit_merge_no_ff() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "on main");
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo.replace(
        "src/store.rs",
        "    conn.check();\n",
        "    conn.check();\n    conn.vacuum();\n",
    );
    repo.write("FEATURE.md", "feature work\n");
    repo.annotate("close", "note", "feature only");
    repo.commit_all("feature work");

    repo.git(&["switch", "-q", "main"]);
    let absent = repo.tool_err("resonance_why", json!({"ref": "close"}));
    assert!(
        absent.contains("nothing matches"),
        "main does not have the feature note yet: {absent}"
    );
    repo.git(&["merge", "-q", "--no-ff", "--no-edit", "feature"]);
    assert!(repo.read("src/store.rs").contains("conn.vacuum()"));
    assert_eq!(repo.read("FEATURE.md"), "feature work\n");
    attached(&repo, "open");
    assert_eq!(annotations_on(&repo, "open"), ["on main"]);
    assert_eq!(annotations_on(&repo, "close"), ["feature only"]);
    assert_eq!(repo.git_status(), "");
    let parents = repo.git(&["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(
        parents.split_whitespace().count(),
        3,
        "the merge is not a fast-forward: {parents}"
    );
}

#[test]
fn fast_forward_merge_keeps_the_annotation() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "comes along");
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo.write("FEATURE.md", "f\n");
    repo.commit_all("feature");
    repo.git(&["switch", "-q", "main"]);
    repo.git(&["merge", "-q", "--ff-only", "feature"]);
    assert_eq!(repo.read("FEATURE.md"), "f\n");
    assert_eq!(annotations_on(&repo, "open"), ["comes along"]);
    attached(&repo, "open");
}

#[test]
fn commit_a_records_a_tracked_edit() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "stays");
    // Offline graph changes are not git files. The note is sealed onto a real commit.
    repo.write("NOTE.md", "n\n");
    repo.commit_all("the note");
    repo.replace("src/store.rs", "conn.check();", "conn.check(); // via -a");
    repo.git(&["commit", "-q", "-a", "-m", "commit -a"]);
    assert!(repo.read("src/store.rs").contains("via -a"));
    attached(&repo, "open");
    assert_eq!(annotations_on(&repo, "open"), ["stays"]);
    assert_eq!(repo.git_status(), "");
}

#[test]
fn a_partial_add_commits_one_file_and_keeps_the_other() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "stays");
    repo.write("A.md", "a\n");
    repo.write("B.md", "b\n");
    repo.git(&["add", "A.md"]);
    repo.git(&["commit", "-q", "-m", "only A"]);
    let status = repo.git_status();
    assert!(status.contains("B.md"), "{status}");
    assert!(!status.contains("A.md"), "{status}");
    assert_eq!(annotations_on(&repo, "open"), ["stays"]);
    attached(&repo, "open");
}

#[test]
fn a_message_only_amend_keeps_the_delta() {
    // No Resonance command between the commit and the amend: catch-up sees both
    // reflog entries at once (MVP-030).
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "amended message");
    repo.write("A.md", "a\n");
    repo.commit_all("first message");
    repo.git(&["commit", "-q", "--amend", "-m", "retitled"]);
    assert_eq!(repo.git(&["log", "-1", "--format=%s"]).trim(), "retitled");
    assert_eq!(annotations_on(&repo, "open"), ["amended message"]);
    attached(&repo, "open");
}

#[test]
fn stash_and_pop_round_trip_an_anchored_edit() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "stays");
    repo.replace("src/store.rs", "conn.check();", "conn.check_stashed();");
    repo.ok(&["status"]);
    repo.git(&["stash", "push", "-q", "-m", "wip", "--", "src/store.rs"]);
    assert!(
        !repo.read("src/store.rs").contains("check_stashed"),
        "the stash took the edit"
    );
    repo.git(&["stash", "pop", "-q"]);
    assert!(repo.read("src/store.rs").contains("check_stashed"));
    attached(&repo, "open");
    assert_eq!(annotations_on(&repo, "open"), ["stays"]);
}

#[test]
fn git_restore_discards_an_edit_and_undo_puts_it_back() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.replace("src/store.rs", "conn.check();", "conn.check_restored();");
    repo.ok(&["status"]);
    repo.git(&["restore", "--", "src/store.rs"]);
    assert!(!repo.read("src/store.rs").contains("check_restored"));

    let list = repo.json(&["recover"]);
    let c4 = list["divergences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["class"] == "C4")
        .cloned()
        .unwrap_or_else(|| panic!("{list}"));
    let rec = c4["recovery"].as_str().unwrap();
    repo.ok(&["recover", "undo", rec]);
    assert!(repo.read("src/store.rs").contains("check_restored"));
    attached(&repo, "open");
}

#[test]
fn revert_of_a_code_commit_keeps_the_anchor() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "survives the revert");
    repo.write("NOTE.md", "n\n");
    repo.commit_all("the note");
    repo.replace(
        "src/store.rs",
        "    conn.check();\n",
        "    conn.check();\n    conn.vacuum();\n",
    );
    repo.commit_all("add vacuum");
    repo.git(&["revert", "--no-edit", "HEAD"]);
    assert!(!repo.read("src/store.rs").contains("conn.vacuum()"));
    attached(&repo, "open");
    assert_eq!(annotations_on(&repo, "open"), ["survives the revert"]);
}

#[test]
fn cherry_pick_of_a_code_commit_follows_the_anchor() {
    // The picked commit changes code only. Carrying an offline delta that was
    // sealed on that commit is not what this test checks; see docs/testing.md.
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "already on main");
    repo.write("NOTE.md", "n\n");
    repo.commit_all("the note");
    repo.git(&["switch", "-q", "-c", "tweak"]);
    repo.replace("src/store.rs", "conn.check();", "conn.check_picked();");
    repo.commit_all("tweak the check");
    repo.git(&["switch", "-q", "main"]);
    repo.git(&["cherry-pick", "tweak"]);
    assert!(repo.read("src/store.rs").contains("check_picked"));
    attached(&repo, "open");
    assert_eq!(annotations_on(&repo, "open"), ["already on main"]);
}

#[test]
fn pull_rebase_replays_a_local_commit() {
    let (repo, mate, _bare) = shared_pair();
    // A commit that contains a delta does not rebase in direct mode: post-checkout
    // writes that delta path as an untracked file, and the pick then refuses to
    // overwrite it. This test rebases a code-only commit. See docs/testing.md.
    mate.write("MATE.md", "from the branch\n");
    mate.git(&["add", "MATE.md"]);
    mate.git(&[
        "commit",
        "-q",
        "--no-verify",
        "-m",
        "mate, before the rebase",
    ]);

    repo.write("MAIN.md", "from main\n");
    repo.commit_all("main moves");
    repo.git(&["push", "-q", "origin", "main"]);

    let pull = mate.git_output(&["pull", "-q", "--rebase", "origin", "main"]);
    assert!(
        pull.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&pull.stdout),
        String::from_utf8_lossy(&pull.stderr)
    );
    assert_eq!(mate.read("MAIN.md"), "from main\n");
    assert_eq!(mate.read("MATE.md"), "from the branch\n");
    assert!(
        annotations_on(&mate, "open").contains(&"keep the signature".to_string()),
        "main's annotation is in the rebased history"
    );
    attached(&mate, "open");
}

#[test]
fn a_resolved_source_conflict_keeps_the_anchor() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "keep me");
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo.replace("src/store.rs", "conn.check();", "conn.check_feature();");
    repo.commit_all("feature edits the same line");
    repo.git(&["switch", "-q", "main"]);
    repo.replace("src/store.rs", "conn.check();", "conn.check_main();");
    repo.commit_all("main edits the same line");

    let merge = repo.git_output(&["merge", "--no-edit", "feature"]);
    assert!(
        !merge.status.success(),
        "the same line must conflict\n{}",
        String::from_utf8_lossy(&merge.stdout)
    );
    assert!(
        repo.read("src/store.rs").contains("<<<<<<<"),
        "conflict markers"
    );
    repo.write(
        "src/store.rs",
        &STORE.replace("conn.check();", "conn.check_both();"),
    );
    repo.git(&["add", "src/store.rs"]);
    repo.git(&["commit", "-q", "--no-edit"]);
    assert!(repo.read("src/store.rs").contains("check_both"));
    attached(&repo, "open");
    assert_eq!(annotations_on(&repo, "open"), ["keep me"]);
    assert_eq!(repo.git_status(), "");
}

#[test]
fn detached_head_round_trip() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "only on the tip");
    repo.write("TIP.md", "tip\n");
    repo.commit_all("the tip");
    assert_eq!(annotations_on(&repo, "open"), ["only on the tip"]);

    repo.git(&["switch", "-q", "--detach", "HEAD~1"]);
    let absent = repo.tool_err("resonance_why", json!({"ref": "open"}));
    assert!(
        absent.contains("nothing matches"),
        "the parent commit does not have the note: {absent}"
    );
    assert!(!repo.path().join("TIP.md").exists());
    repo.git(&["switch", "-q", "main"]);
    assert_eq!(annotations_on(&repo, "open"), ["only on the tip"]);
    assert_eq!(repo.read("TIP.md"), "tip\n");
    attached(&repo, "open");
}

#[test]
fn a_linked_worktree_keeps_its_own_graph() {
    // FR-070: discovery uses `git rev-parse --show-toplevel`, so a linked worktree is its
    // own root, open delta, and daemon.
    let mut repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "only on main");
    let parent = tempfile::tempdir().unwrap();
    let wt = parent.path().join("wt");
    repo.git(&["worktree", "add", "-q", wt.to_str().unwrap(), "HEAD"]);

    repo.set_work_dir(Some(wt.clone()));
    repo.ok(&["init", "--no-agent"]);
    repo.declare("src/store.rs:4-9", "open");
    repo.annotate("open", "note", "only in the worktree");
    assert_eq!(annotations_on(&repo, "open"), ["only in the worktree"]);
    assert!(wt.join(".resonance").is_dir());
    repo.ok(&["daemon", "stop"]);
    repo.set_work_dir(None);

    assert_eq!(annotations_on(&repo, "open"), ["only on main"]);
    assert_eq!(repo.git_status(), "");
    let toplevel = repo
        .git_command(&["-C", wt.to_str().unwrap(), "rev-parse", "--show-toplevel"])
        .output()
        .unwrap();
    assert!(toplevel.status.success());
    // macOS canonicalizes `/var` to `/private/var`.
    assert_eq!(
        std::fs::canonicalize(String::from_utf8_lossy(&toplevel.stdout).trim()).unwrap(),
        std::fs::canonicalize(&wt).unwrap()
    );
}

#[test]
fn a_file_mode_change_keeps_the_anchor() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "mode");
    use std::os::unix::fs::PermissionsExt;
    let path = repo.path().join("src/store.rs");
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    repo.commit_all("make the store executable");
    let listed = repo.git(&["ls-files", "-s", "src/store.rs"]);
    assert!(listed.starts_with("100755"), "{listed}");
    attached(&repo, "open");
    assert_eq!(annotations_on(&repo, "open"), ["mode"]);
}

#[test]
fn a_symlink_is_not_its_own_file_and_resolves_to_its_target() {
    // The walk skips symlinks, so the graph has no file node for the link. A path a person
    // types is canonicalized, so declaring through the link anchors the target file.
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "the real file");
    std::os::unix::fs::symlink("src/store.rs", repo.path().join("STORE_LINK.rs")).unwrap();
    repo.git(&["add", "STORE_LINK.rs"]);
    repo.commit_all("add a symlink");
    let hits = repo.tool(
        "resonance_search",
        json!({"text": "STORE_LINK", "type": "File"}),
    );
    assert_eq!(hits.as_array().unwrap().len(), 0, "{hits}");
    let declared = repo.tool(
        "resonance_declare_unit",
        json!({"path": "STORE_LINK.rs", "startLine": 4, "endLine": 9, "name": "via_link"}),
    );
    assert_eq!(declared["location"]["path"], "src/store.rs", "{declared}");
    assert_eq!(declared["location"]["state"], "attached");
    attached(&repo, "open");
    assert_eq!(annotations_on(&repo, "open"), ["the real file"]);
}

#[test]
fn autosquash_of_code_commits_keeps_the_anchor() {
    // The commits being replayed are code only (`--no-verify`), so they carry no delta.
    // Squashing a commit that sealed a delta is a separate, still-open case.
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "before the fixup");
    repo.write("NOTE.md", "sealed\n");
    repo.commit_all("seal the annotation");

    repo.write("A.md", "feature\n");
    repo.git(&["add", "A.md"]);
    repo.git(&["commit", "-q", "--no-verify", "-m", "feature"]);
    repo.write("B.md", "fixup\n");
    repo.git(&["add", "B.md"]);
    repo.git(&["commit", "-q", "--no-verify", "--fixup", "HEAD"]);

    let mut cmd = repo.git_command(&[
        "-c",
        "sequence.editor=true",
        "rebase",
        "-q",
        "-i",
        "--autosquash",
        "HEAD~2",
    ]);
    cmd.env("GIT_SEQUENCE_EDITOR", "true");
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(repo.read("A.md"), "feature\n");
    assert_eq!(repo.read("B.md"), "fixup\n");
    assert_eq!(repo.read("NOTE.md"), "sealed\n");
    attached(&repo, "open");
    assert_eq!(annotations_on(&repo, "open"), ["before the fixup"]);
    let subjects = repo.git(&["log", "--format=%s", "HEAD~2..HEAD"]);
    assert!(
        !subjects.lines().any(|l| l.starts_with("fixup!")),
        "autosquash folded the fixup commit: {subjects}"
    );
}

#[test]
fn pull_rebase_replays_a_commit_that_carries_a_delta() {
    let (repo, mate, _bare) = shared_pair();
    repo.annotate("open", "note", "rebased");
    repo.write("LOCAL.md", "local\n");
    repo.commit_all("local delta");
    mate.write("MATE.md", "from the other clone\n");
    mate.commit_all("mate moves main");
    mate.git(&["push", "-q", "origin", "main"]);
    let pull = repo.git_output(&["pull", "-q", "--rebase", "origin", "main"]);
    assert!(
        pull.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&pull.stdout),
        String::from_utf8_lossy(&pull.stderr)
    );
    assert_eq!(repo.read("MATE.md"), "from the other clone\n");
    assert_eq!(repo.read("LOCAL.md"), "local\n");
    assert!(annotations_on(&repo, "open").contains(&"rebased".to_string()));
    attached(&repo, "open");
}

#[test]
fn cherry_pick_of_a_delta_commit_is_visible_on_both_branches() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.write("NOTE.md", "n\n");
    repo.commit_all("base");
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo.annotate("open", "note", "picked");
    repo.write("PICK.md", "p\n");
    repo.commit_all("feature note");
    repo.git(&["switch", "-q", "main"]);
    repo.git(&["cherry-pick", "feature"]);
    assert_eq!(repo.read("PICK.md"), "p\n");
    assert_eq!(annotations_on(&repo, "open"), ["picked"]);
    repo.git(&["switch", "-q", "feature"]);
    assert_eq!(annotations_on(&repo, "open"), ["picked"]);
    attached(&repo, "open");
}

#[test]
fn squash_merge_keeps_an_offline_delta() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.write("NOTE.md", "n\n");
    repo.commit_all("base");
    repo.git(&["switch", "-q", "-c", "feature"]);
    repo.annotate("close", "note", "squashed");
    repo.write("SQ.md", "s\n");
    repo.commit_all("feature note");
    repo.git(&["switch", "-q", "main"]);
    repo.git(&["merge", "--squash", "feature"]);
    repo.git(&["commit", "-q", "-m", "squash feature"]);
    assert_eq!(repo.read("SQ.md"), "s\n");
    assert_eq!(annotations_on(&repo, "close"), ["squashed"]);
    attached(&repo, "close");
}

#[test]
fn reset_soft_and_mixed_keep_the_delta_uncommitted() {
    for mode in ["--soft", "--mixed"] {
        let repo = TestRepo::with_project();
        repo.declare_sample();
        repo.write("NOTE.md", "n\n");
        repo.commit_all("base");
        repo.annotate("open", "note", mode);
        repo.write("RESET.md", "r\n");
        repo.commit_all("to reset");
        repo.git(&["reset", "-q", mode, "HEAD~1"]);
        assert_eq!(annotations_on(&repo, "open"), [mode], "{mode}");
        let detached = repo.json(&["status"])["history"]["detached"]
            .as_array()
            .unwrap()
            .len();
        assert_eq!(detached, 0, "{mode} must not detach the delta");
        attached(&repo, "open");
    }
}

#[test]
fn a_deleted_delta_file_is_recovered_from_the_journal() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.write("NOTE.md", "n\n");
    repo.commit_all("base");
    repo.annotate("open", "note", "journaled");
    repo.write("J.md", "j\n");
    repo.commit_all("seal the note");
    let dir = repo.path().join(".resonance/deltas");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "delta") {
            std::fs::remove_file(path).unwrap();
        }
    }
    repo.write("K.md", "k\n");
    repo.git(&["add", "K.md"]);
    repo.git(&["commit", "-q", "--no-verify", "-m", "move HEAD"]);
    repo.ok(&["status"]);
    assert_eq!(annotations_on(&repo, "open"), ["journaled"]);
    attached(&repo, "open");
}
