//! Store boundary (FR-110, FR-161): `resonance-core` depends on no SQLite crate and no
//! source-code parser. The conformance suite itself runs in resonance-store-sqlite's tests.

use std::path::Path;
use std::process::Command;

#[test]
fn resonance_core_depends_on_neither_sqlite_nor_a_parser() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "tree",
            "-p",
            "resonance-core",
            "--edges",
            "normal,build",
            "--prefix",
            "none",
            "--offline",
        ])
        .current_dir(&workspace)
        .output()
        .expect("cargo tree");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let tree = String::from_utf8_lossy(&out.stdout);
    for forbidden in [
        "rusqlite",
        "libsqlite3-sys",
        "resonance-store-sqlite",
        "tree-sitter",
    ] {
        assert!(
            !tree.lines().any(|l| l.starts_with(forbidden)),
            "resonance-core must not depend on {forbidden}:\n{tree}"
        );
    }
}

#[test]
fn no_sql_appears_in_the_core_crate() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../resonance-core/src");
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let text = std::fs::read_to_string(&p).unwrap();
            for needle in ["SELECT ", "INSERT INTO", "rusqlite", "CREATE TABLE"] {
                assert!(
                    !text.contains(needle),
                    "{} mentions {needle} (FR-161)",
                    p.display()
                );
            }
        }
    }
}
