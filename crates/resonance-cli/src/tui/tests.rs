//! The TUI driven through ratatui's test backend (mvp.md §5 "S25 / TUI and change log"):
//! listing, filtering, viewing, editing a graph-native item, adding one with a suggested key
//! and refusing a duplicate key, retiring with confirmation, and opening an imported item's
//! source at its line. Changes carry the person's identity and `origin = tui`.

use std::path::Path;
use std::process::Command;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use resonance_core::api::Engine;
use resonance_core::lifecycle;
use resonance_core::ops::Actor;
use resonance_core::repo::Repo;
use resonance_core::service::Direct;
use serde_json::{Value, json};

use super::app::{App, BackendClient, Client, Effect, Screen};
use super::{editor_command, ui};
use crate::commands::open_store;

const SPEC: &str = "# Requirements\n\n- **FR-001** The store opens a database at a path.\n- **FR-002** Saving is atomic.\n";

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repository with Resonance initialized and a spec imported, and an app over it.
fn setup() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q", "-b", "main"]);
    git(p, &["config", "user.email", "person@example.com"]);
    git(p, &["config", "user.name", "Person"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    std::fs::create_dir_all(p.join("spec")).unwrap();
    std::fs::write(p.join("spec/requirements.md"), SPEC).unwrap();
    git(p, &["add", "-A"]);
    git(p, &["commit", "-q", "-m", "spec"]);
    let repo = Repo::discover(p).unwrap();
    lifecycle::init(&repo, &open_store).unwrap();
    let store = open_store(&repo.store_path()).unwrap();
    let engine = Engine::open(
        repo,
        store,
        Actor::human("person@example.com"),
        p.to_path_buf(),
    )
    .unwrap();
    let mut client = BackendClient(Box::new(Direct { engine }));
    client
        .call("import", json!({"paths": ["spec/requirements.md"]}))
        .unwrap();
    client
        .call("changes_done", json!({"seqs": [1, 2]}))
        .unwrap();
    let app = App::new(Box::new(client)).unwrap();
    (dir, app)
}

fn key(c: KeyCode) -> KeyEvent {
    KeyEvent::new(c, KeyModifiers::NONE)
}

fn press(app: &mut App, c: char) -> Effect {
    app.on_key(key(KeyCode::Char(c)))
}

fn type_text(app: &mut App, s: &str) {
    for c in s.chars() {
        press(app, c);
    }
}

fn save(app: &mut App) {
    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
}

/// The screen as text, at the given size.
fn render_size(app: &mut App, width: u16, height: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
    t.draw(|f| ui::draw(f, app)).unwrap();
    let buf = t.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

/// The screen as text.
fn render(app: &mut App) -> String {
    render_size(app, 140, 30)
}

fn select(app: &mut App, key: &str) {
    let i = app
        .items
        .iter()
        .position(|x| x.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("{key} is not listed"));
    app.table.select(Some(i));
}

fn call(app: &mut App, method: &str, args: Value) -> Value {
    // The app owns its client; go through a fresh call on it.
    app_client(app).call(method, args).unwrap()
}

fn app_client(app: &mut App) -> &mut dyn Client {
    app.client_mut()
}

#[test]
fn lists_filters_and_shows_items() {
    let (_dir, mut app) = setup();
    let screen = render(&mut app);
    assert!(
        screen.contains("FR-001") && screen.contains("FR-002"),
        "{screen}"
    );
    assert!(
        screen.contains("Requirement") && screen.contains("doc"),
        "{screen}"
    );

    press(&mut app, '/');
    type_text(&mut app, "atomic");
    assert!(render(&mut app).contains("/atomic"));
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.items.len(), 1);
    assert!(render(&mut app).contains("[text: atomic]"));
    app.on_key(key(KeyCode::Esc));
    assert_eq!(app.items.len(), 2);

    press(&mut app, 't');
    assert!(render(&mut app).contains("[type: Requirement]"));
    press(&mut app, 't');
    assert!(app.items.is_empty(), "no decisions yet");
    assert!(render(&mut app).contains("Nothing matches"));
    app.on_key(key(KeyCode::Esc));
    press(&mut app, 's');
    assert_eq!(app.filter_status.as_deref(), Some("active"));
    assert_eq!(app.items.len(), 2);
    app.on_key(key(KeyCode::Esc));

    select(&mut app, "FR-001");
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::Detail);
    let screen = render(&mut app);
    assert!(
        screen.contains("Source: spec/requirements.md:3"),
        "{screen}"
    );
    assert!(screen.contains("Links (0)"), "{screen}");
    app.on_key(key(KeyCode::Esc));
    assert_eq!(app.screen, Screen::List);

    press(&mut app, '?');
    assert!(render(&mut app).contains("Help"));
    press(&mut app, 'x');
    assert_eq!(app.screen, Screen::List);
    press(&mut app, 'q');
    assert!(app.quit);
}

#[test]
fn adds_with_a_suggested_key_and_refuses_a_duplicate() {
    let (_dir, mut app) = setup();
    press(&mut app, 'a');
    assert_eq!(app.screen, Screen::Form);
    let form = app.form.clone().unwrap();
    assert_eq!(form.key, "FR-003", "FR-273: the next free key");
    assert!(render(&mut app).contains("Add Requirement"));

    // The text is long: it is edited in $EDITOR.
    app.on_key(key(KeyCode::Down));
    let effect = app.on_key(key(KeyCode::Enter));
    assert_eq!(
        effect,
        Effect::EditText {
            field: "text".into(),
            text: String::new()
        }
    );
    app.finish_text("text", "Exports are complete.\n");
    app.on_key(key(KeyCode::Down));
    app.on_key(key(KeyCode::Down));
    type_text(&mut app, "high");
    save(&mut app);
    assert_eq!(app.screen, Screen::List, "{:?}", app.message);
    assert_eq!(app.selected().unwrap().key.as_deref(), Some("FR-003"));
    assert_eq!(app.selected().unwrap().priority.as_deref(), Some("high"));

    // A duplicate key is refused, and the form stays open.
    press(&mut app, 'a');
    for _ in 0..6 {
        app.on_key(key(KeyCode::Backspace));
    }
    type_text(&mut app, "FR-001");
    app.on_key(key(KeyCode::Down));
    app.on_key(key(KeyCode::Enter));
    app.finish_text("text", "Again");
    save(&mut app);
    assert_eq!(app.screen, Screen::Form);
    let (msg, err) = app.message.clone().unwrap();
    assert!(err && msg.contains("already exists"), "{msg}");
    app.on_key(key(KeyCode::Esc));

    // Another type: the key follows its own prefix.
    press(&mut app, 'a');
    app.on_key(key(KeyCode::Up));
    app.on_key(key(KeyCode::Right));
    let form = app.form.clone().unwrap();
    assert_eq!(form.ntype.to_string(), "Decision");
    assert_eq!(form.key, "ADR-001");
    app.on_key(key(KeyCode::Esc));

    // The change log has the person's addition, from the TUI.
    press(&mut app, 'l');
    assert_eq!(app.screen, Screen::Log);
    assert_eq!(app.log.len(), 1);
    assert_eq!(app.log[0].origin, "tui");
    assert_eq!(app.log[0].actor, "human:person@example.com");
    assert!(render(&mut app).contains("FR-003 created"));
}

#[test]
fn edits_and_retires_graph_native_items_in_place() {
    let (_dir, mut app) = setup();
    call(
        &mut app,
        "spec_add",
        json!({"type": "Design", "key": "DES-001", "fields": {"title": "Storage layer", "text": "One file"}}),
    );
    call(
        &mut app,
        "spec_add",
        json!({"type": "Requirement", "key": "FR-010", "fields": {"text": "Linked"}}),
    );
    call(
        &mut app,
        "link",
        json!({"src": "FR-010", "type": "derivedFrom", "dst": "FR-001"}),
    );
    app.refresh();

    select(&mut app, "DES-001");
    press(&mut app, 'e');
    assert_eq!(app.screen, Screen::Form);
    for _ in 0.."Storage layer".len() {
        app.on_key(key(KeyCode::Backspace));
    }
    type_text(&mut app, "Store");
    save(&mut app);
    let item = call(&mut app, "spec_show", json!({"ref": "DES-001"}));
    assert_eq!(item["title"], "Store");

    // Retire: confirmation first, with the link count (FR-271).
    select(&mut app, "FR-010");
    press(&mut app, 'r');
    assert_eq!(app.screen, Screen::Confirm);
    let screen = render(&mut app);
    assert!(
        screen.contains("Retire FR-010?") && screen.contains("It has 1 link(s)"),
        "{screen}"
    );
    press(&mut app, 'n');
    assert_eq!(
        call(&mut app, "spec_show", json!({"ref": "FR-010"}))["status"],
        "active"
    );
    press(&mut app, 'r');
    press(&mut app, 'y');
    assert_eq!(
        call(&mut app, "spec_show", json!({"ref": "FR-010"}))["status"],
        "retired"
    );
}

#[test]
fn imported_items_open_their_source_document() {
    let (dir, mut app) = setup();
    select(&mut app, "FR-002");
    let effect = press(&mut app, 'e');
    let root = std::fs::canonicalize(dir.path()).unwrap();
    match effect {
        Effect::EditFile { path, line } => {
            assert_eq!(
                std::fs::canonicalize(path).unwrap(),
                root.join("spec/requirements.md")
            );
            assert_eq!(line, 4);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        press(&mut app, 'r'),
        Effect::EditFile { line: 4, .. }
    ));
    assert_eq!(app.screen, Screen::List, "nothing is retired here");

    let cmd = editor_command("code -w", Path::new("/x/spec.md"), Some(4));
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(cmd.get_program(), "sh");
    assert_eq!(args, ["-c", "code -w \"$@\"", "sh", "+4", "/x/spec.md"]);
}

#[test]
fn detail_shows_spec_status_and_other_definitions() {
    let (dir, mut app) = setup();
    std::fs::write(
        dir.path().join("spec/requirements.md"),
        "# Requirements\n\n- **FR-001** The store opens a database at a path. [Decided]\n- **FR-002** Saving is atomic.\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("spec/also.md"),
        "- **FR-001** A second definition of the same key.\n",
    )
    .unwrap();
    call(
        &mut app,
        "import",
        json!({"paths": ["spec/requirements.md", "spec/also.md"]}),
    );
    app.refresh();
    select(&mut app, "FR-001");
    app.on_key(key(KeyCode::Enter));
    let screen = render(&mut app);
    assert!(
        screen.contains("specStatus: Decided"),
        "the trailing status label is a field of the item:\n{screen}"
    );
    assert!(
        screen.contains("alsoDefinedIn:") && screen.contains("spec/also.md:1"),
        "a later source of the same key is listed:\n{screen}"
    );
    // `n` moves to the next item and starts that item at the top.
    press(&mut app, 'j');
    press(&mut app, 'j');
    press(&mut app, 'n');
    assert_eq!(app.detail_scroll, 0);
    let screen = render(&mut app);
    assert!(screen.contains("FR-002"), "{screen}");
    assert!(!screen.contains("specStatus: Decided"), "{screen}");
}

#[test]
fn detail_and_change_log_scroll() {
    let (_dir, mut app) = setup();
    let text: String = (0..40)
        .map(|i| format!("scroll-line-{i:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    call(
        &mut app,
        "spec_add",
        json!({"type": "Requirement", "key": "FR-040", "fields": {"text": text}}),
    );
    for i in 0..12 {
        call(
            &mut app,
            "spec_add",
            json!({"type": "Decision", "key": format!("ADR-{i:03}"), "fields": {"title": format!("decision {i}"), "decision": "yes"}}),
        );
    }
    app.refresh();

    select(&mut app, "FR-040");
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.screen, Screen::Detail);
    // Short terminal: the header fields fill the window, so the long body is clipped.
    let top = render_size(&mut app, 80, 12);
    assert!(top.contains("FR-040"), "{top}");
    assert!(
        !top.contains("scroll-line-30"),
        "the tail is below the window:\n{top}"
    );

    // j moves down until the first body line appears, then until it leaves.
    let mut saw_start = top.contains("scroll-line-00");
    let mut passed_start = false;
    for _ in 0..80 {
        press(&mut app, 'j');
        let screen = render_size(&mut app, 80, 12);
        if screen.contains("scroll-line-00") {
            saw_start = true;
        } else if saw_start {
            assert!(
                screen.contains("scroll-line-"),
                "j keeps a later line on screen:\n{screen}"
            );
            passed_start = true;
            break;
        }
    }
    assert!(saw_start && passed_start, "j scrolls the open item");

    // A reload of the same item (the direct-mode refresh) keeps the place.
    let kept = app.detail_scroll;
    assert!(kept > 0);
    app.refresh();
    assert_eq!(app.screen, Screen::Detail);
    assert_eq!(app.detail_scroll, kept);
    assert!(
        !render_size(&mut app, 80, 12).contains("scroll-line-00"),
        "refresh does not jump back to the top"
    );

    let at = app.detail_scroll;
    app.on_key(key(KeyCode::PageUp));
    let _ = render_size(&mut app, 80, 12);
    assert!(app.detail_scroll < at, "page up scrolls toward the start");

    // Extra j presses stop at the last line instead of scrolling into a blank page.
    for _ in 0..200 {
        press(&mut app, 'j');
    }
    let end = render_size(&mut app, 80, 12);
    assert!(
        end.contains("Links") || end.contains("scroll-line-39"),
        "the end stays on screen:\n{end}"
    );
    let capped = app.detail_scroll;
    press(&mut app, 'j');
    let _ = render_size(&mut app, 80, 12);
    assert_eq!(app.detail_scroll, capped);

    for _ in 0..200 {
        press(&mut app, 'k');
    }
    let _ = render_size(&mut app, 80, 12);
    assert_eq!(
        app.detail_scroll, 0,
        "k scrolls back to the start and stops"
    );

    // The change log is its own screen: j scrolls it and does not leave it.
    press(&mut app, 'l');
    assert_eq!(app.screen, Screen::Log);
    let log = render_size(&mut app, 100, 12);
    assert!(log.contains("FR-040 created"), "{log}");
    assert!(
        !log.contains("decision 11"),
        "later entries start off screen:\n{log}"
    );
    for _ in 0..8 {
        press(&mut app, 'j');
    }
    assert_eq!(app.screen, Screen::Log);
    let log = render_size(&mut app, 100, 12);
    assert!(
        !log.contains("FR-040 created"),
        "j scrolls the change log:\n{log}"
    );
    assert!(
        log.contains("decision 8") || log.contains("decision 11"),
        "{log}"
    );
    // A key that is not a log command stays on the log.
    press(&mut app, 'x');
    assert_eq!(app.screen, Screen::Log);
    app.on_key(key(KeyCode::Esc));
    assert_eq!(app.screen, Screen::Detail);
}
