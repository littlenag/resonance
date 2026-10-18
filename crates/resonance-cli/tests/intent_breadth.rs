//! Intent breadth (B4, mvp.md §5 "Intent breadth"): designs, constraints, supersede,
//! derivedFrom and realizes, extension types, the importers (Spec Kit, Kiro, EARS, ADR),
//! source tags and `intent project`.

mod common;

use common::TestRepo;
use serde_json::Value;

fn keys(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["key"].as_str().map(String::from))
        .collect()
}

#[test]
fn designs_constraints_and_their_links() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let d = repo.json(&[
        "design",
        "add",
        "--key",
        "DES-1",
        "--title",
        "Storage layer",
        "--text",
        "One file per store",
    ]);
    assert_eq!(d["type"], "Design");
    assert_eq!(d["status"], "active");
    let c = repo.json(&[
        "constraint",
        "add",
        "--key",
        "CON-1",
        "--text",
        "Opening takes under 10 ms",
        "--scope",
        "src/store.rs",
    ]);
    assert_eq!(c["props"]["scope"], "src/store.rs");

    repo.ok(&["link", "open", "realizes", "DES-1"]);
    repo.ok(&["link", "CON-1", "constrains", "open"]);
    repo.ok(&[
        "req",
        "add",
        "FR-010",
        "--text",
        "Opening is atomic",
        "--priority",
        "high",
        "--status",
        "draft",
    ]);
    repo.ok(&["link", "FR-010", "derivedFrom", "FR-001"]);
    let why = repo.json(&["show", "open"]);
    let text = why.to_string();
    assert!(
        text.contains("realizes") && text.contains("constrained by"),
        "{why}"
    );
    let r = repo.json(&["spec", "show", "FR-010"]);
    assert_eq!(r["priority"], "high");
    assert_eq!(r["status"], "draft");

    // A design cannot satisfy a requirement; the error says what can.
    let err = repo.fails(&["link", "DES-1", "satisfies", "FR-001"], 1);
    assert!(err.contains("satisfies a requirement"), "{err}");

    // Supersede: the old one is superseded and its links go stale (S7).
    repo.ok(&[
        "req",
        "add",
        "FR-011",
        "--text",
        "The store opens a database, or creates one",
    ]);
    repo.ok(&["test", "FR-001"]);
    repo.ok(&["req", "supersede", "FR-001", "--by", "FR-011"]);
    let old = repo.json(&["spec", "show", "FR-001"]);
    assert_eq!(old["status"], "superseded");
    let trace = repo.json(&["trace", "FR-001"]);
    assert_eq!(trace["verifiedBy"][0]["link"]["status"], "stale", "{trace}");

    // Edit and retire: retired, never deleted (FR-271).
    repo.ok(&["design", "edit", "DES-1", "--text", "Two files per store"]);
    repo.ok(&["design", "retire", "DES-1"]);
    let list = repo.json(&["spec", "list", "--type", "Design"]);
    assert_eq!(list[0]["status"], "retired");
    let active = repo.json(&["spec", "list", "--status", "active"]);
    assert!(!keys(&active).contains(&"DES-1".to_string()));
}

#[test]
fn imported_items_are_edited_in_their_source() {
    let repo = TestRepo::with_project();
    repo.ok(&["import", "spec/requirements.md"]);
    let err = repo.fails(&["req", "edit", "FR-001", "--priority", "high"], 1);
    assert!(err.contains("spec/requirements.md:3"), "{err}");
}

#[test]
fn extension_types_survive_export_restore_and_rebuild() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    let svc = repo.json(&[
        "ext",
        "add",
        "acme:Service",
        "--name",
        "billing",
        "--prop",
        "tier=gold",
    ]);
    let id = svc["node"]["id"].as_str().unwrap().to_string();
    assert_eq!(svc["properties"]["extType"], "acme:Service");
    repo.ok(&["link", "open", "acme:runsIn", &id]);
    let bad = repo.fails(&["ext", "add", "Service", "--name", "x"], 2);
    assert!(bad.contains("namespace:Name"), "{bad}");

    repo.write("NOTES.md", "x\n");
    repo.commit_all("seal");
    repo.ok(&["rebuild"]);
    let show = repo.json(&["show", &id]);
    assert_eq!(show["properties"]["tier"], "gold");
    assert!(show.to_string().contains("acme:runsIn"), "{show}");

    let export = repo.path().join("..").join("ext-export.jsonl");
    repo.ok(&["export", "--out", export.to_str().unwrap()]);
    let other = TestRepo::with_project();
    other.ok(&["restore", export.to_str().unwrap()]);
    let found = other.json(&["search", "billing"]);
    assert_eq!(found[0]["type"], "Extension", "{found}");
    let _ = std::fs::remove_file(export);
}

#[test]
fn spec_kit_kiro_ears_and_adr_sources_import() {
    let repo = TestRepo::with_project();
    repo.write(
        "specs/001-photos/spec.md",
        "# Feature\n\n### Functional Requirements\n\n- **FR-101**: System MUST let users create albums.\n- **FR-102**: System MUST sort albums by date.\n",
    );
    repo.write(
        ".kiro/specs/user-auth/requirements.md",
        "# Requirements\n\n### Requirement 1\n\n**User Story:** As a user, I want to sign in.\n\n#### Acceptance Criteria\n\n1. WHEN credentials are valid THEN the system SHALL sign the user in\n",
    );
    repo.write(
        ".kiro/specs/user-auth/design.md",
        "# Design\n\n## Overview\n\nA session service.\n\n## Components\n\nLogin form, session store.\n",
    );
    repo.write(
        "docs/ears.md",
        "- REQ-1: When the user saves, the editor shall write the file.\n- When the disk is full, the system shall warn the user.\n",
    );
    repo.write(
        "docs/adr/0001-use-sqlite.md",
        "# 1. Use SQLite\n\n## Status\n\nAccepted\n\n## Context\n\nWe need an embedded store.\n\n## Decision\n\nUse SQLite behind a store API.\n\n## Consequences\n\nOne file per repository.\n",
    );
    repo.write("docs/adr/README.md", "# Decisions\n");
    repo.commit_all("specs");

    let out = repo.json(&[
        "import",
        "specs/001-photos/spec.md",
        ".kiro/specs/user-auth/requirements.md",
        ".kiro/specs/user-auth/design.md",
        "docs/adr",
    ]);
    let added: Vec<String> = out["intent"]["added"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap().to_string())
        .collect();
    for k in [
        "FR-101",
        "FR-102",
        "USERAUTH-1",
        "USERAUTH-DESIGN-1",
        "ADR-0001",
    ] {
        assert!(added.contains(&k.to_string()), "{k} in {added:?}");
    }
    let ears = repo.json(&["import", "docs/ears.md", "--format", "ears"]);
    assert_eq!(ears["intent"]["added"], serde_json::json!(["REQ-1"]));
    assert_eq!(
        ears["intent"]["unidentified"].as_array().unwrap().len(),
        1,
        "{ears}"
    );

    let adr = repo.json(&["spec", "show", "ADR-0001"]);
    assert_eq!(adr["type"], "Decision");
    assert_eq!(adr["title"], "Use SQLite");
    assert_eq!(adr["status"], "accepted");
    assert!(
        adr["props"]["context"]
            .as_str()
            .unwrap()
            .contains("embedded")
    );
    let design = repo.json(&["spec", "show", "USERAUTH-DESIGN-2"]);
    assert_eq!(design["title"], "Components");

    // Editing a source updates the item; removing one retires it.
    repo.replace(
        "docs/adr/0001-use-sqlite.md",
        "Use SQLite behind a store API.",
        "Use SQLite.",
    );
    assert_eq!(
        repo.json(&["spec", "show", "ADR-0001"])["text"],
        "Use SQLite."
    );
    repo.remove("docs/adr/0001-use-sqlite.md");
    assert_eq!(
        repo.json(&["spec", "show", "ADR-0001"])["status"],
        "retired"
    );
}

#[test]
fn source_tags_create_and_remove_links_when_enabled() {
    let repo = TestRepo::with_project();
    repo.declare_sample();
    repo.replace(
        "src/store.rs",
        "    conn.migrate();",
        "    conn.migrate(); // @satisfies FR-003",
    );
    assert_eq!(
        repo.json(&["trace", "FR-003"])["satisfiedBy"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "tags are off by default"
    );
    let cfg = repo
        .read(".resonance/config.toml")
        .replace("sourceTags = false", "sourceTags = true");
    repo.write(".resonance/config.toml", &cfg);
    let trace = repo.json(&["trace", "FR-003"]);
    let by = trace["satisfiedBy"].as_array().unwrap();
    assert_eq!(by.len(), 1, "{trace}");
    assert_eq!(
        by[0]["unit"]["address"], "src/store.rs::open",
        "the innermost unit: {trace}"
    );
    assert_eq!(by[0]["link"]["provenance"], "imported");

    repo.replace("src/store.rs", " // @satisfies FR-003", "");
    assert!(
        repo.json(&["trace", "FR-003"])["satisfiedBy"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn intent_project_writes_markdown_that_then_owns_the_items() {
    let repo = TestRepo::with_project();
    repo.ok(&["req", "add", "FR-200", "--text", "Exports are complete"]);
    repo.ok(&["req", "add", "FR-201", "--text", "Restores are idempotent"]);
    let plan = repo.json(&["intent", "project", "docs/intent", "--dry-run"]);
    assert_eq!(plan["written"], false);
    assert!(!repo.path().join("docs/intent/requirements.md").exists());
    repo.fails(&["intent", "project", "docs/intent"], 1);

    let done = repo.json(&["intent", "project", "docs/intent", "--yes"]);
    assert_eq!(done["keys"], serde_json::json!(["FR-200", "FR-201"]));
    let text = repo.read("docs/intent/requirements.md");
    assert!(text.contains("- **FR-200** Exports are complete"));
    let item = repo.json(&["spec", "show", "FR-200"]);
    assert_eq!(item["origin"], "import");
    // The document owns it now: edits there flow in.
    repo.replace(
        "docs/intent/requirements.md",
        "Exports are complete",
        "Exports are complete and signed",
    );
    assert_eq!(
        repo.json(&["spec", "show", "FR-200"])["text"],
        "Exports are complete and signed"
    );
}
