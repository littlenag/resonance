//! Table-driven anchor-tracking scenarios: insert above, edit inside, delete half, reformat,
//! move, duplicate, delete, restore (MVP-013, mvp.md §5 "Anchor tracking").
//!
//! Each scenario declares a unit on a region of a file, applies an edit, re-locates the
//! anchor, and checks which MVP-013 step placed it and what it now quotes. The operations
//! these steps record are checked end to end in the CLI crate's tests.

use resonance_core::anchors::locate::{CONFIDENT, Relocation, Step, relocate, restore};
use resonance_core::anchors::{Anchor, AnchorState, Method, lines_to_bytes};

const ORIGINAL: &str = "\
use std::io;

/// Opens the store.
pub fn open(path: &str) -> Store {
    let conn = connect(path);
    conn.migrate();
    conn.check();
    Store { conn }
}

pub fn close(store: Store) {
    store.flush();
    drop(store);
}
";

/// The `open` function: lines 4-9.
fn open_anchor(content: &str) -> Anchor {
    let (s, e) = lines_to_bytes(content, 4, 9).unwrap();
    Anchor::at("src/store.rs", content, s, e, Method::Declared)
}

struct Scenario {
    name: &'static str,
    /// New content of the same file, or `None` if it was deleted.
    new: Option<String>,
    /// Other changed files, `(path, content)`.
    others: Vec<(&'static str, String)>,
    expect_step: Step,
    /// Text the anchor must quote afterwards (checked when attached).
    expect_quote: Option<String>,
    expect_path: &'static str,
    /// Whether the result must be confident (not on the repair list).
    expect_confident: bool,
}

fn scenarios() -> Vec<Scenario> {
    let open_fn = &ORIGINAL
        [ORIGINAL.find("pub fn open").unwrap()..ORIGINAL.find("\n\npub fn close").unwrap()];
    let without_open = ORIGINAL.replace(&format!("{open_fn}\n\n"), "");
    vec![
        Scenario {
            name: "insert above",
            new: Some(ORIGINAL.replace("use std::io;\n", "use std::io;\nuse std::fs;\nuse std::path::Path;\n")),
            others: vec![],
            expect_step: Step::Diff,
            expect_quote: Some(open_fn.to_string()),
            expect_path: "src/store.rs",
            expect_confident: true,
        },
        Scenario {
            name: "edit elsewhere in the file",
            new: Some(ORIGINAL.replace("store.flush();", "store.flush_all();")),
            others: vec![],
            expect_step: Step::Diff,
            expect_quote: Some(open_fn.to_string()),
            expect_path: "src/store.rs",
            expect_confident: true,
        },
        Scenario {
            name: "edit inside keeps at least half the lines",
            new: Some(ORIGINAL.replace("    conn.check();\n", "    conn.check();\n    conn.vacuum();\n")),
            others: vec![],
            expect_step: Step::Diff,
            expect_quote: Some(open_fn.replace("    conn.check();\n", "    conn.check();\n    conn.vacuum();\n")),
            expect_path: "src/store.rs",
            expect_confident: true,
        },
        Scenario {
            name: "delete half the lines",
            new: Some(ORIGINAL.replace("    conn.migrate();\n    conn.check();\n", "")),
            others: vec![],
            expect_step: Step::Diff,
            expect_quote: Some(open_fn.replace("    conn.migrate();\n    conn.check();\n", "")),
            expect_path: "src/store.rs",
            expect_confident: true,
        },
        Scenario {
            name: "reformat (re-indent every line)",
            new: Some(ORIGINAL.replace("\n    ", "\n  ")),
            others: vec![],
            expect_step: Step::Approximate,
            expect_quote: Some(open_fn.replace("\n    ", "\n  ")),
            expect_path: "src/store.rs",
            expect_confident: true,
        },
        Scenario {
            name: "move to another file",
            new: Some(without_open.clone()),
            others: vec![(
                "src/open.rs",
                format!("use std::io;\n\n/// Opens the store.\n{open_fn}\n"),
            )],
            expect_step: Step::Move,
            expect_quote: Some(open_fn.to_string()),
            expect_path: "src/open.rs",
            expect_confident: true,
        },
        Scenario {
            name: "moved within the file, past an unrelated rewrite",
            new: Some(format!(
                "use std::io;\n\npub fn close(store: Store) {{\n    store.flush();\n    drop(store);\n}}\n\n/// Opens the store.\n{open_fn}\n"
            )),
            others: vec![],
            expect_step: Step::Diff,
            expect_quote: Some(open_fn.to_string()),
            expect_path: "src/store.rs",
            expect_confident: true,
        },
        Scenario {
            name: "duplicate snippet: context picks the original",
            new: Some(format!(
                "{ORIGINAL}\nmod copy {{\n{open_fn}\n}}\n"
            )),
            others: vec![],
            expect_step: Step::Diff,
            expect_quote: Some(open_fn.to_string()),
            expect_path: "src/store.rs",
            expect_confident: true,
        },
        Scenario {
            name: "rewritten beyond recognition is orphaned, never re-attached",
            new: Some(ORIGINAL.replace(
                open_fn,
                "pub async fn open_pool(cfg: Config) -> Result<Pool, Error> {\n    let pool = Pool::builder().max_size(cfg.size).build().await?;\n    Ok(pool)\n}",
            )),
            others: vec![],
            expect_step: Step::Orphaned,
            expect_quote: None,
            expect_path: "src/store.rs",
            expect_confident: true,
        },
        Scenario {
            name: "delete entirely",
            new: Some(without_open),
            others: vec![],
            expect_step: Step::Orphaned,
            expect_quote: None,
            expect_path: "src/store.rs",
            expect_confident: true,
        },
        Scenario {
            name: "file deleted",
            new: None,
            others: vec![],
            expect_step: Step::Orphaned,
            expect_quote: None,
            expect_path: "src/store.rs",
            expect_confident: true,
        },
    ]
}

fn run(s: &Scenario) -> Relocation {
    let anchor = open_anchor(ORIGINAL);
    let others: Vec<(&str, &str)> = s.others.iter().map(|(p, c)| (*p, c.as_str())).collect();
    relocate(&anchor, Some(ORIGINAL), s.new.as_deref(), &others)
}

#[test]
fn every_scenario_lands_on_the_expected_step() {
    let mut failures = Vec::new();
    for s in scenarios() {
        let r = run(&s);
        let mut problems = Vec::new();
        if r.step != s.expect_step {
            problems.push(format!("step {:?}, expected {:?}", r.step, s.expect_step));
        }
        if let Some(q) = &s.expect_quote {
            if &r.anchor.quote != q {
                problems.push(format!("quote {:?}, expected {:?}", r.anchor.quote, q));
            }
            if r.anchor.state != AnchorState::Attached {
                problems.push("not attached".into());
            }
            let content = if r.anchor.path == "src/store.rs" {
                s.new.clone().unwrap()
            } else {
                s.others
                    .iter()
                    .find(|(p, _)| *p == r.anchor.path)
                    .unwrap()
                    .1
                    .clone()
            };
            if content.get(r.anchor.start_byte..r.anchor.end_byte) != Some(r.anchor.quote.as_str())
            {
                problems.push("byte range does not match the quote".into());
            }
        } else if r.anchor.state != AnchorState::Orphaned {
            problems.push(format!(
                "expected orphaned, got {:?} at {}",
                r.anchor.method,
                r.anchor.location()
            ));
        }
        if r.anchor.path != s.expect_path {
            problems.push(format!(
                "path {}, expected {}",
                r.anchor.path, s.expect_path
            ));
        }
        if s.expect_confident
            && r.anchor.state == AnchorState::Attached
            && r.anchor.confidence < CONFIDENT
        {
            problems.push(format!(
                "confidence {} is below {CONFIDENT}",
                r.anchor.confidence
            ));
        }
        if !problems.is_empty() {
            failures.push(format!("{}: {}", s.name, problems.join("; ")));
        }
    }
    assert!(
        failures.is_empty(),
        "anchor scenarios failed:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn an_orphan_keeps_its_last_quote_and_location() {
    let deleted = scenarios()
        .into_iter()
        .find(|s| s.name == "delete entirely")
        .unwrap();
    let r = run(&deleted);
    let original = open_anchor(ORIGINAL);
    assert_eq!(r.anchor.quote, original.quote);
    assert_eq!(r.anchor.location(), original.location());
    assert_eq!(r.anchor.method, Method::Orphaned);
}

#[test]
fn restore_when_the_text_reappears_with_its_context() {
    let orphan = open_anchor(ORIGINAL).orphaned();
    // Switching back to a branch that has the function again.
    let restored = restore(&orphan, &[("src/store.rs", ORIGINAL)]).expect("restored");
    assert_eq!(restored.method, Method::Restored);
    assert_eq!(restored.state, AnchorState::Attached);
    assert_eq!(restored.start_byte, orphan.start_byte);
    // The same text somewhere unrelated is not enough (MVP-005 requires matching context).
    let quote = &orphan.quote;
    let elsewhere = format!("// unrelated\nfn other() {{}}\n\nmod x {{\n{quote}\n}}\n// tail\n");
    assert!(restore(&orphan, &[("src/other.rs", &elsewhere)]).is_none());
}

#[test]
fn an_ambiguous_exact_match_goes_on_the_repair_list() {
    // The region's own lines disappear from their place, and two identical copies with
    // identical surroundings appear elsewhere.
    let block = "fn dup() {\n    one();\n    two();\n}";
    let original = format!("a\nb\nc\n{block}\nd\ne\nf\n");
    let (s, e) = (
        original.find(block).unwrap(),
        original.find(block).unwrap() + block.len(),
    );
    let anchor = Anchor::at("f.rs", &original, s, e, Method::Declared);
    let new = format!("x\ny\nz\n\na\nb\nc\n{block}\nd\ne\nf\n\na\nb\nc\n{block}\nd\ne\nf\n");
    let r = relocate(&anchor, None, Some(&new), &[]);
    assert_eq!(r.step, Step::Exact);
    assert!(r.anchor.confidence < CONFIDENT, "{}", r.anchor.confidence);
}

#[test]
fn a_light_rewrite_is_matched_approximately_with_its_confidence() {
    let anchor = open_anchor(ORIGINAL);
    // Rename one variable throughout the function: too many lines change for diff mapping.
    let new = ORIGINAL
        .replace("let conn =", "let db =")
        .replace("conn.", "db.")
        .replace("{ conn }", "{ conn: db }");
    let r = relocate(&anchor, Some(ORIGINAL), Some(&new), &[]);
    assert_eq!(r.step, Step::Approximate);
    assert!(
        r.anchor.confidence >= 0.8 && r.anchor.confidence < 1.0,
        "{}",
        r.anchor.confidence
    );
    assert!(r.anchor.quote.contains("let db = connect(path);"));
}
