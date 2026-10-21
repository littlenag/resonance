//! Storage maturity (B5, mvp.md §5 "Fold and storage"): delta files, the deterministic fold,
//! `rebuild`, export and `restore`, idempotent deltas, the layout migration (MVP-025),
//! backups (FR-184) and the guard against `.resonance/` changing under the graph (C6).

mod common;

use std::fs;

use common::TestRepo;
use serde_json::{Value, json};

/// The `.delta` files in `.resonance/deltas/`, sorted.
fn delta_files(repo: &TestRepo) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = fs::read_dir(repo.path().join(".resonance/deltas"))
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    v.retain(|p| p.extension().is_some_and(|e| e == "delta"));
    v.sort();
    v
}

/// A summary of the authored graph that does not depend on operation IDs.
fn fingerprint(repo: &TestRepo) -> Value {
    let trace = repo.tool("resonance_trace", json!({"requirement": "FR-001"}));
    let why = repo.tool("resonance_why", json!({"ref": "open"}));
    let status = repo.json(&["status"]);
    serde_json::json!({
        "satisfiedBy": trace["satisfiedBy"].as_array().map(|a| a.len()),
        "verifiedBy": trace["verifiedBy"].as_array().map(|a| a.len()),
        "annotations": why["annotations"],
        "counts": status["counts"],
    })
}

#[test]
fn every_change_is_written_to_the_open_delta_in_canonical_form() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let files = delta_files(&repo);
    assert_eq!(files.len(), 1, "one open delta: {files:?}");
    let text = fs::read_to_string(&files[0]).unwrap();
    let mut lines = text.lines();
    let header: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(header["resonance"], "delta");
    assert_eq!(header["format"], 1);
    let rest: Vec<&str> = lines.collect();
    let last: Value = serde_json::from_str(rest.last().unwrap()).unwrap();
    assert!(
        last["manifest"].is_object(),
        "the manifest is the last line"
    );
    let ops: Vec<Value> = rest[..rest.len() - 1]
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        ops.iter().any(|o| o["kind"] == "edge.add"),
        "links are operations"
    );
    assert!(
        !ops.iter().any(|o| o["kind"] == "file.put"),
        "file content travels in git, not in deltas (FR-064)"
    );
    // Canonical: keys sorted, no whitespace (FR-153).
    for l in &rest {
        let v: Value = serde_json::from_str(l).unwrap();
        let mut canon = String::new();
        canonical(&v, &mut canon);
        assert_eq!(*l, canon);
    }
    assert_eq!(
        repo.git_status(),
        "",
        "offline deltas stay invisible to git"
    );
}

fn canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*k).clone()).to_string());
                out.push(':');
                canonical(&m[*k], out);
            }
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canonical(x, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

#[test]
fn rebuild_reproduces_the_same_authored_graph() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "note", "keep");
    repo.tool("resonance_test", json!({"ref": "FR-001"}));
    repo.write("NOTES.md", "x\n");
    repo.commit_all("seal the graph changes into a commit");
    repo.replace("src/store.rs", "conn.check();", "conn.check_all();");
    let before = fingerprint(&repo);
    let out = repo.json(&["rebuild"]);
    assert_eq!(out["history"]["refolded"], true, "{out}");
    assert_eq!(fingerprint(&repo), before);
    // Twice is the same as once (FR-150).
    repo.ok(&["rebuild"]);
    assert_eq!(fingerprint(&repo), before);
}

#[test]
fn a_delta_that_arrives_twice_is_harmless() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.write("NOTES.md", "x\n");
    repo.commit_all("seal");
    repo.ok(&["status"]);
    let before = fingerprint(&repo);
    let sealed = delta_files(&repo);
    assert!(!sealed.is_empty());
    // A copy under another delta name (as after a cherry-pick and a merge): same operations.
    let copy = sealed[0].with_file_name("01ZZZZZZZZZZZZZZZZZZZZZZZZ.delta");
    fs::copy(&sealed[0], &copy).unwrap();
    repo.ok(&["rebuild"]);
    assert_eq!(
        fingerprint(&repo),
        before,
        "operations are idempotent by ID (FR-193)"
    );
}

#[test]
fn export_and_restore_round_trip() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "warning", "careful");
    let before = fingerprint(&repo);
    let export = repo.path().join("..").join("export.jsonl");
    repo.ok(&["export", "--out", export.to_str().unwrap()]);

    let other = TestRepo::with_project();
    other.import_sample();
    let out = other.json(&["restore", export.to_str().unwrap()]);
    assert!(out["applied"].as_u64().unwrap() > 0, "{out}");
    assert!(
        out["skipped"].as_u64().unwrap() > 0,
        "requirements already existed: {out}"
    );
    assert_eq!(fingerprint(&other)["annotations"], before["annotations"]);
    assert_eq!(fingerprint(&other)["satisfiedBy"], before["satisfiedBy"]);
    // Restoring again adds nothing.
    let again = other.json(&["restore", export.to_str().unwrap()]);
    assert_eq!(again["applied"], 0, "{again}");
    let _ = fs::remove_file(export);
}

#[test]
fn restore_into_a_live_graph_adds_what_is_missing_and_keeps_the_rest() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.annotate("open", "warning", "from the export");
    repo.tool(
        "resonance_create_decision",
        json!({"title": "Exported", "decision": "from the source graph"}),
    );
    let export = repo.home().join("export-live.jsonl");
    repo.ok(&["export", "--out", export.to_str().unwrap()]);

    let other = TestRepo::with_project();
    other.import_sample();
    other.declare("src/store.rs:11-14", "local_only");
    other.tool(
        "resonance_create_decision",
        json!({"title": "Kept", "decision": "stays on the target"}),
    );
    let out = other.json(&["restore", export.to_str().unwrap()]);
    assert!(out["applied"].as_u64().unwrap() > 0, "{out}");
    assert!(out["skipped"].as_u64().unwrap() > 0, "{out}");

    let kept = other.tool("resonance_show", json!({"ref": "Kept"}));
    assert_eq!(kept["properties"]["decision"], "stays on the target");
    let local = other.tool("resonance_show", json!({"ref": "local_only"}));
    assert_eq!(local["node"]["type"], "CodeUnit");
    assert_eq!(local["location"]["state"], "attached");
    let why = other.tool("resonance_why", json!({"ref": "open"}));
    let notes: Vec<&str> = why["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["text"].as_str().unwrap())
        .collect();
    assert!(notes.contains(&"from the export"), "{notes:?}");
    let exported = other.tool("resonance_show", json!({"ref": "Exported"}));
    assert_eq!(exported["properties"]["decision"], "from the source graph");

    let again = other.json(&["restore", export.to_str().unwrap()]);
    assert_eq!(again["applied"], 0, "{again}");
    assert_eq!(
        other.tool("resonance_show", json!({"ref": "Kept"}))["properties"]["decision"],
        "stays on the target"
    );
    let _ = fs::remove_file(export);
}

#[test]
fn a_repository_in_the_earlier_layout_is_migrated() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let before = fingerprint(&repo);
    repo.ok(&["daemon", "stop"]);
    // Recreate the earlier layout: the live store at .resonance/graph.db, no local/.
    let res = repo.path().join(".resonance");
    fs::rename(res.join("local/state.db"), res.join("graph.db")).unwrap();
    for f in ["state.db-wal", "state.db-shm"] {
        let p = res.join("local").join(f);
        if p.exists() {
            fs::rename(&p, res.join(f.replace("state", "graph"))).unwrap();
        }
    }
    fs::remove_dir_all(res.join("local")).unwrap();
    fs::remove_dir_all(res.join("deltas")).unwrap();

    let out = repo.run(&["--json", "status"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let note = String::from_utf8_lossy(&out.stderr);
    assert!(
        note.contains("new layout"),
        "the migration is reported once: {note}"
    );
    assert!(res.join("local/state.db").is_file());
    assert_eq!(fingerprint(&repo), before, "nothing was lost");
    let backups = repo.home().join(".resonance/backups");
    assert!(
        fs::read_dir(backups).unwrap().next().is_some(),
        "a backup was taken first"
    );
    let again = repo.run(&["status"]);
    assert!(!String::from_utf8_lossy(&again.stderr).contains("new layout"));
}

#[test]
fn a_missing_resonance_directory_is_restored_from_a_backup() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let before = fingerprint(&repo);
    let out = repo.json(&["backup"]);
    let dest = out["backup"].as_str().unwrap().to_string();
    assert!(
        dest.starts_with(repo.home().to_str().unwrap()),
        "outside the repository"
    );
    repo.ok(&["daemon", "stop"]);
    fs::remove_dir_all(repo.path().join(".resonance")).unwrap();

    let err = repo.fails(&["status"], 1);
    assert!(err.contains("backup --restore"), "{err}");
    repo.ok(&["backup", "--restore"]);
    assert_eq!(fingerprint(&repo), before);
    let listed = repo.json(&["backup", "--list"]);
    assert_eq!(listed.as_array().unwrap().len(), 1);
}

#[test]
fn deltas_disappearing_without_a_git_operation_are_held_not_applied() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.write("NOTES.md", "x\n");
    repo.commit_all("seal");
    repo.ok(&["status"]);
    let before = fingerprint(&repo);
    for f in delta_files(&repo) {
        fs::remove_file(f).unwrap();
    }
    let status = repo.json(&["status"]);
    assert!(
        status["history"]["held"].is_object(),
        "{}",
        status["history"]
    );
    assert_eq!(
        fingerprint(&repo)["counts"],
        before["counts"],
        "the graph is kept"
    );
    let text = repo.ok(&["status"]);
    assert!(text.contains("changed unexpectedly"), "{text}");
}
