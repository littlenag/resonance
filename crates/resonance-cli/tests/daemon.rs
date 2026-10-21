//! The daemon and registry (B1, mvp.md §3.15): one daemon per repository, on-demand start,
//! idle exit, stop, subscriptions, `--cached`, git operations (S10, MVP-023), crash recovery
//! (S13), discovery and health (S14), replacement of daemons from another build, and the
//! registry. These tests always use daemons, whatever RESONANCE_TEST_DIRECT says.

mod common;

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use common::TestRepo;
use resonance_core::daemon::client::Remote;
use resonance_core::daemon::runtime;
use resonance_core::model::{Layer, NodeType};
use resonance_core::ops::{Actor, Tx};
use resonance_core::repo::Repo;
use resonance_core::store::{GraphScope, GraphStore};
use serde_json::{Value, json};

/// A command that always goes through the daemon.
fn daemon_cmd(repo: &TestRepo, args: &[&str]) -> Command {
    let mut cmd = repo.command(args);
    cmd.env_remove("RESONANCE_NO_DAEMON");
    cmd
}

fn ok(repo: &TestRepo, args: &[&str]) -> String {
    let out = daemon_cmd(repo, args).output().unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn json(repo: &TestRepo, args: &[&str]) -> Value {
    let mut all = vec!["--json"];
    all.extend_from_slice(args);
    serde_json::from_str(&ok(repo, &all)).unwrap()
}

/// A node through MCP, as an agent reads it.
fn show(repo: &TestRepo, reference: &str) -> Value {
    repo.tool("resonance_show", json!({"ref": reference}))
}

/// A node from the daemon without waiting for pending changes (`cached`, FR-102).
fn cached_show(repo: &TestRepo, reference: &str) -> Value {
    use resonance_core::service::{Backend, Call};
    let r = Repo::discover(repo.path()).unwrap();
    let mut remote =
        Remote::connect_existing(&r, Actor::agent("test"), root(repo)).expect("daemon running");
    remote.cached = true;
    remote
        .call(Call::new("show", json!({"ref": reference})))
        .unwrap()
}

fn info(repo: &TestRepo) -> runtime::DaemonInfo {
    runtime::read_info(&repo.path().canonicalize().unwrap()).expect("daemon.json")
}

fn wait_until(what: &str, timeout: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn root(repo: &TestRepo) -> std::path::PathBuf {
    repo.path().canonicalize().unwrap()
}

#[test]
fn one_daemon_per_repository_started_on_demand_and_stopped() {
    let repo = TestRepo::with_project_daemon();
    ok(&repo, &["status"]);
    let first = info(&repo);
    assert!(runtime::lock_held(&root(&repo)), "FR-082");
    assert!(
        Path::new(&first.endpoint).exists(),
        "FR-083: the endpoint is recorded"
    );

    // A second daemon refuses to start (FR-082).
    let out = daemon_cmd(&repo, &["daemon", "run"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("already running"));

    // Every command is served by the same daemon.
    ok(&repo, &["status"]);
    assert_eq!(info(&repo).pid, first.pid);
    let health = json(&repo, &["daemon"]);
    assert_eq!(health["state"], "ready");
    assert_eq!(health["ready"], true);
    assert!(health["requestsServed"].as_u64().unwrap() >= 2);

    let stopped = ok(&repo, &["daemon", "stop"]);
    assert!(stopped.contains("Stopped"), "{stopped}");
    assert!(!runtime::lock_held(&root(&repo)));
    assert!(
        runtime::read_info(&root(&repo)).is_none(),
        "daemon.json is removed"
    );
    assert!(
        !Path::new(&first.endpoint).exists(),
        "the socket is removed"
    );
    assert!(ok(&repo, &["daemon", "stop"]).contains("No daemon"));
}

#[test]
fn an_idle_daemon_exits() {
    let repo = TestRepo::with_project_daemon();
    ok(&repo, &["daemon", "stop"]);
    let out = daemon_cmd(&repo, &["status"])
        .env("RESONANCE_DAEMON_IDLE_SECONDS", "1")
        .output()
        .unwrap();
    assert!(out.status.success());
    // The daemon was started by this command, so it took the 1-second idle period (FR-084).
    wait_until("the idle daemon to exit", Duration::from_secs(15), || {
        !runtime::lock_held(&root(&repo))
    });
}

#[test]
fn reads_follow_saves_and_subscribers_hear_about_changes() {
    let repo = TestRepo::with_project_daemon();
    repo.declare_sample();
    let r = Repo::discover(repo.path()).unwrap();
    let remote =
        Remote::connect_existing(&r, Actor::human("test"), root(&repo)).expect("daemon running");
    let (tx, rx) = mpsc::channel();
    let sub = thread::spawn(move || {
        let _ = remote.subscribe(|note| tx.send(note).is_ok());
    });
    // Give the subscription a moment to register before changing anything.
    thread::sleep(Duration::from_millis(200));
    repo.replace(
        "src/store.rs",
        "use std::io;\n",
        "use std::io;\nuse std::fs;\n",
    );
    // Read-your-writes (FR-102): the very next answer reflects the save.
    assert_eq!(show(&repo, "open")["location"]["startLine"], 5);
    // FR-104: the change stream reported the ingest.
    let mut kinds = Vec::new();
    while let Ok(note) = rx.recv_timeout(Duration::from_secs(5)) {
        kinds.push(note["kind"].as_str().unwrap_or("").to_string());
        if kinds.iter().any(|k| k == "ingest") {
            break;
        }
    }
    assert!(kinds.contains(&"ingest".to_string()), "{kinds:?}");
    // Mutations are announced too.
    repo.annotate("open", "note", "subscribed");
    let mut saw_mutation = false;
    while let Ok(note) = rx.recv_timeout(Duration::from_secs(5)) {
        if note["kind"] == "mutation" && note["method"] == "annotate" {
            saw_mutation = true;
            break;
        }
    }
    assert!(saw_mutation);
    ok(&repo, &["daemon", "stop"]);
    let _ = sub.join();
}

#[test]
fn changes_made_during_a_git_operation_wait_for_it_and_cached_answers_do_not() {
    let repo = TestRepo::with_project_daemon();
    repo.declare_sample();
    // A git command holds the index lock while it changes files (MVP-023).
    let lock = repo.path().join(".git/index.lock");
    fs::write(&lock, "").unwrap();
    repo.replace(
        "src/store.rs",
        "use std::io;\n",
        "use std::io;\nuse std::fs;\n",
    );
    // A cached answer does not wait, and does not see the change yet (FR-102).
    let started = Instant::now();
    let cached = cached_show(&repo, "open");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(cached["location"]["startLine"], 4);
    // The daemon reports the git operation.
    wait_until("the git-operation state", Duration::from_secs(5), || {
        json(&repo, &["--cached", "daemon"])["state"] == "git-operation"
    });
    // When the git command finishes, the changes are applied as one batch.
    let remover = {
        let lock = lock.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            fs::remove_file(lock).unwrap();
        })
    };
    let started = Instant::now();
    let synced = show(&repo, "open");
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "waited for the lock"
    );
    assert_eq!(synced["location"]["startLine"], 5);
    remover.join().unwrap();
}

#[test]
fn a_branch_switch_is_applied_and_reflected_in_the_next_answer() {
    let repo = TestRepo::with_project_daemon();
    repo.declare_sample();
    repo.git(&["checkout", "-q", "-b", "many"]);
    for i in 0..40 {
        repo.write(
            &format!("gen/file_{i:02}.rs"),
            &format!("pub const N: u32 = {i};\n"),
        );
    }
    repo.replace("src/store.rs", "use std::io;\n", "// many\nuse std::io;\n");
    repo.commit_all("many files");
    repo.git(&["checkout", "-q", "main"]);
    ok(&repo, &["status"]);
    let r = Repo::discover(repo.path()).unwrap();
    let remote = Remote::connect_existing(&r, Actor::human("test"), root(&repo)).unwrap();
    let (tx, rx) = mpsc::channel();
    let sub = thread::spawn(move || {
        let _ = remote.subscribe(|note| tx.send(note).is_ok());
    });
    thread::sleep(Duration::from_millis(200));
    repo.git(&["checkout", "-q", "many"]);
    // S10 AC2: the answer reflects the new commit.
    let status = json(&repo, &["status"]);
    assert_eq!(status["counts"]["files"].as_u64().unwrap(), 4 + 40);
    assert_eq!(show(&repo, "open")["location"]["startLine"], 5);
    // S10 AC1: not forty separate edits.
    let mut ingests = 0;
    while let Ok(note) = rx.recv_timeout(Duration::from_millis(500)) {
        if note["kind"] == "ingest" {
            ingests += 1;
        }
    }
    assert!(ingests <= 5, "{ingests} ingest batches for one checkout");
    ok(&repo, &["daemon", "stop"]);
    let _ = sub.join();
}

#[test]
fn a_crashed_daemon_causes_delay_never_wrong_answers() {
    let repo = TestRepo::with_project_daemon();
    repo.declare_sample();
    let pid = info(&repo).pid;
    Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .unwrap();
    wait_until("the lock to be released", Duration::from_secs(10), || {
        !runtime::lock_held(&root(&repo))
    });
    // Edits while no daemon runs (S13).
    repo.replace(
        "src/store.rs",
        "use std::io;\n",
        "use std::io;\nuse std::fs;\nuse std::env;\n",
    );
    // The next command starts a new daemon, which reconciles before answering (AC1, AC2).
    assert_eq!(show(&repo, "open")["location"]["startLine"], 6);
    assert_ne!(info(&repo).pid, pid);
}

#[test]
fn a_daemon_from_another_build_is_replaced() {
    let repo = TestRepo::with_project_daemon();
    let copy_dir = tempfile::tempdir().unwrap();
    let copy = copy_dir.path().join("resonance");
    fs::copy(common::bin(), &copy).unwrap();
    // A different build: copies may keep the original's modification time, so change it.
    let earlier = std::time::SystemTime::now() - Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(&copy)
        .unwrap()
        .set_modified(earlier)
        .unwrap();
    ok(&repo, &["daemon", "stop"]);
    // Start a daemon with the copy (a different build as far as the client can tell).
    let out = Command::new(&copy)
        .args(["status"])
        .current_dir(repo.path())
        .env("HOME", repo.home())
        .env("RESONANCE_RUNTIME_DIR", runtime_dir_of(&repo))
        .env_remove("RESONANCE_NO_DAEMON")
        .stdout(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let old = info(&repo);
    ok(&repo, &["status"]);
    let new = info(&repo);
    assert_ne!(old.pid, new.pid, "the old daemon was replaced");
    assert_ne!(old.binary_id, new.binary_id);
}

fn runtime_dir_of(repo: &TestRepo) -> String {
    // The test harness passes its runtime directory to every command; read it back from a run.
    let out = repo.command(&["--json", "doctor"]).output().unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    v["runtimeDir"].as_str().unwrap().to_string()
}

#[test]
fn doctor_finds_daemons_without_the_registry() {
    let repo = TestRepo::with_project_daemon();
    ok(&repo, &["status"]);
    let report = json(&repo, &["doctor"]);
    assert_eq!(report["registryRunning"], false);
    let daemons = report["daemons"].as_array().unwrap();
    assert_eq!(daemons.len(), 1, "{report}");
    assert_eq!(daemons[0]["foundThrough"], "daemon.json", "S14 AC1");
    assert_eq!(daemons[0]["health"]["ready"], true, "S14 AC2");
    assert!(daemons[0]["health"]["lagMs"].is_u64());
    assert_eq!(report["repository"]["excluded"], true);
    assert!(
        report["repository"]["integrityProblems"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let text = ok(&repo, &["doctor"]);
    assert!(
        text.contains("ready") && text.contains("No problems found"),
        "{text}"
    );
}

#[test]
fn doctor_reports_a_removed_importer() {
    let repo = TestRepo::with_project();
    ok(&repo, &["daemon", "stop"]);
    let cfg = repo.read(".resonance/config.toml");
    repo.write(
        ".resonance/config.toml",
        &format!("{cfg}\n\n[intent]\nsources = [\"spec/requirements.md\"]\n"),
    );
    let root = Repo::discover(repo.path()).unwrap();
    let mut store = resonance_store_sqlite::SqliteStore::open(&root.store_path()).unwrap();
    store
        .write(GraphScope::Primary, &mut |w| {
            let mut tx = Tx::new(w, Actor::human("person@example.com"))?;
            let mut props = serde_json::Map::new();
            props.insert("key".into(), json!("FR-900"));
            props.insert("text".into(), json!("Brought in by the old importer."));
            props.insert("kind".into(), json!("functional"));
            props.insert("status".into(), json!("active"));
            props.insert("source".into(), json!({"path": "old/reqs.md", "line": 4}));
            tx.create_node("req:FR-900", NodeType::Requirement, Layer::Authored, props)?;
            tx.finish()?;
            Ok(())
        })
        .unwrap();
    drop(store);
    let item = repo.tool("resonance_show_item", json!({"ref": "FR-900"}));
    assert!(item["source"].is_null(), "{item}");
    let report = json(&repo, &["doctor"]);
    let problems = report["problems"].to_string();
    assert!(problems.contains("intent.sources"), "{problems}");
    assert!(problems.contains("FR-900"), "{problems}");
    assert!(problems.contains("resonance_import_items"), "{problems}");
}

#[test]
fn the_registry_tracks_daemons_and_rebuilds_after_a_restart() {
    let repo = TestRepo::with_project_daemon();
    let runtime_dir = runtime_dir_of(&repo);
    let spawn_registry = || {
        daemon_cmd(&repo, &["registry", "run"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    };
    let mut registry = spawn_registry();
    let socket = Path::new(&runtime_dir).join("registry.sock");
    wait_until("the registry socket", Duration::from_secs(10), || {
        socket.exists()
    });

    ok(&repo, &["status"]); // Starts a daemon, which registers (FR-090).
    let entries = || -> Vec<Value> {
        resonance_core::registry::request(&socket, "list", serde_json::json!({}))
            .map(|v| v.as_array().cloned().unwrap_or_default())
            .unwrap_or_default()
    };
    wait_until("the daemon to register", Duration::from_secs(10), || {
        entries().len() == 1
    });
    let e = &entries()[0];
    assert_eq!(e["root"], root(&repo).display().to_string(), "FR-091");
    assert!(e["pid"].as_u64().is_some() && e["leaseExpires"].as_u64().is_some());
    let report = json(&repo, &["doctor"]);
    assert_eq!(report["registryRunning"], true);
    assert_eq!(report["daemons"][0]["foundThrough"], "registry");

    // A restarted registry rebuilds its view from endpoint files and locks (FR-092).
    registry.kill().unwrap();
    let _ = registry.wait();
    let _ = fs::remove_file(&socket);
    let mut registry = spawn_registry();
    wait_until("the restarted registry", Duration::from_secs(10), || {
        socket.exists()
    });
    wait_until(
        "the daemon in the rebuilt view",
        Duration::from_secs(10),
        || entries().len() == 1,
    );

    // A stopped daemon unregisters.
    ok(&repo, &["daemon", "stop"]);
    wait_until("the daemon to unregister", Duration::from_secs(10), || {
        entries().is_empty()
    });
    let _ = resonance_core::registry::request(&socket, "shutdown", serde_json::json!({}));
    let _ = registry.wait();
}

#[test]
fn registry_install_previews_and_changes_nothing_without_confirmation() {
    let repo = TestRepo::with_project_daemon();
    let preview = ok(&repo, &["registry", "install", "--dry-run"]);
    assert!(
        preview.contains("registry") && preview.contains("run"),
        "{preview}"
    );
    if cfg!(target_os = "macos") {
        assert!(preview.contains("launchctl bootstrap"), "{preview}");
        assert!(!repo.home().join("Library/LaunchAgents").exists());
    } else {
        assert!(
            preview.contains("systemctl --user enable --now"),
            "{preview}"
        );
        assert!(!repo.home().join(".config/systemd").exists());
    }
    let out = daemon_cmd(&repo, &["registry", "install"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "no terminal and no --yes: nothing happens"
    );
    assert!(!repo.home().join("Library").exists() && !repo.home().join(".config").exists());
}

#[test]
fn registry_install_and_uninstall_write_a_user_service() {
    // A fake launchctl or systemctl is first on PATH, so this never loads a service into
    // the user's real session. The plan itself is chosen at compile time.
    let mut repo = TestRepo::with_project_daemon();
    let bin_dir = repo.home().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let log = bin_dir.join("service.log");
    let tool = if cfg!(target_os = "macos") {
        "launchctl"
    } else {
        "systemctl"
    };
    let script = bin_dir.join(tool);
    fs::write(
        &script,
        format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 0\n", log.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    repo.with_path(bin_dir);

    let installed = ok(&repo, &["registry", "install", "--yes"]);
    assert!(installed.contains("Installed and started"), "{installed}");
    let unit = if cfg!(target_os = "macos") {
        repo.home()
            .join("Library/LaunchAgents/dev.resonance.registry.plist")
    } else {
        repo.home()
            .join(".config/systemd/user/resonance-registry.service")
    };
    let body = fs::read_to_string(&unit).unwrap();
    assert!(body.contains("registry"), "{body}");
    assert!(body.contains("run"), "{body}");
    let calls = fs::read_to_string(&log).unwrap();
    if cfg!(target_os = "macos") {
        assert!(
            calls.contains("bootstrap") && calls.contains("gui/"),
            "{calls}"
        );
    } else {
        assert!(
            calls.contains("daemon-reload") && calls.contains("enable --now"),
            "{calls}"
        );
    }

    let removed = ok(&repo, &["registry", "uninstall", "--yes"]);
    assert!(removed.contains("Stopped and removed"), "{removed}");
    assert!(!unit.exists(), "uninstall removes the service file");
    let calls = fs::read_to_string(&log).unwrap();
    if cfg!(target_os = "macos") {
        assert!(calls.contains("bootout"), "{calls}");
    } else {
        assert!(calls.contains("disable --now"), "{calls}");
    }
}

#[test]
fn the_daemon_leaves_git_status_unchanged() {
    let repo = TestRepo::with_project_daemon();
    repo.declare_sample();
    repo.replace(
        "src/store.rs",
        "use std::io;\n",
        "use std::io;\nuse std::fs;\n",
    );
    show(&repo, "open");
    repo.git(&["checkout", "--", "src/store.rs"]);
    ok(&repo, &["status"]);
    assert_eq!(
        repo.git_status(),
        "",
        "cookies, sockets and logs are invisible to git"
    );
}

#[test]
fn a_deleted_lock_file_never_lets_a_second_daemon_take_over() {
    // FR-082: flock locks an inode, so deleting daemon.lock under a running daemon must not
    // make it look gone.
    let repo = TestRepo::with_project_daemon();
    ok(&repo, &["status"]);
    let first = info(&repo).pid;
    fs::remove_file(repo.path().join(".resonance/local/daemon.lock")).unwrap();
    assert!(
        runtime::lock_held(&root(&repo)),
        "a daemon answering on its socket is running"
    );

    // A second daemon refuses to start, and leaves the first one's socket alone.
    let out = daemon_cmd(&repo, &["daemon", "run"]).output().unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("already running"), "{err}");
    let health = json(&repo, &["daemon"]);
    assert_eq!(
        health["pid"], first,
        "commands still reach the first daemon"
    );

    // Stopping waits until the first daemon is really gone; then a new one starts cleanly.
    ok(&repo, &["daemon", "stop"]);
    assert!(!runtime::lock_held(&root(&repo)));
    ok(&repo, &["status"]);
    assert_ne!(info(&repo).pid, first);
    ok(&repo, &["daemon", "stop"]);
}

#[test]
fn a_foreign_listener_is_replaced_while_this_daemon_holds_the_lock() {
    use std::os::unix::net::UnixListener;
    let repo = TestRepo::with_project_daemon();
    ok(&repo, &["status"]);
    let first = info(&repo);
    let endpoint = std::path::PathBuf::from(&first.endpoint);
    // The daemon's own socket moves aside, and a process that is not a daemon takes the path.
    let aside = endpoint.with_extension("old");
    fs::rename(&endpoint, &aside).unwrap();
    let foreign = UnixListener::bind(&endpoint).unwrap();

    // This daemon holds the lock, so it reclaims the path instead of standing down.
    wait_until(
        "the daemon to reclaim its socket",
        Duration::from_secs(15),
        || runtime::serves_repo(&endpoint, &root(&repo)),
    );
    let health = json(&repo, &["daemon"]);
    assert_eq!(health["pid"], first.pid, "{health}");
    assert!(runtime::lock_file_held(
        &repo.path().join(".resonance/local/daemon.lock")
    ));
    drop(foreign);
    let _ = fs::remove_file(&aside);
    ok(&repo, &["daemon", "stop"]);
    assert!(!endpoint.exists(), "the owner removes its own socket");
}

#[test]
fn a_socket_that_does_not_answer_health_does_not_block_startup() {
    use std::os::unix::net::UnixListener;
    let repo = TestRepo::with_project_daemon();
    let previous = info(&repo);
    let endpoint = std::path::PathBuf::from(&previous.endpoint);
    ok(&repo, &["daemon", "stop"]);
    let foreign = UnixListener::bind(&endpoint).unwrap();
    // The stopped daemon's record, now pointing at a listener that is not Resonance.
    fs::write(
        repo.path().join(".resonance/local/daemon.json"),
        serde_json::to_string_pretty(&previous).unwrap(),
    )
    .unwrap();
    assert!(
        !runtime::listener_alive(&root(&repo)),
        "a listener that fails health is not this daemon"
    );
    assert!(!runtime::lock_held(&root(&repo)));
    let health = json(&repo, &["daemon"]);
    assert_ne!(health["pid"], previous.pid, "{health}");
    assert_eq!(health["root"], root(&repo).display().to_string());
    drop(foreign);
    ok(&repo, &["daemon", "stop"]);
}

#[test]
fn a_copied_daemon_record_does_not_use_the_other_repository() {
    let a = TestRepo::with_project_daemon();
    ok(&a, &["status"]);
    let a_info = info(&a);
    let b = TestRepo::with_project_daemon();
    ok(&b, &["daemon", "stop"]);
    fs::copy(
        a.path().join(".resonance/local/daemon.json"),
        b.path().join(".resonance/local/daemon.json"),
    )
    .unwrap();
    assert!(
        !runtime::listener_alive(&root(&b)),
        "A's socket is not B's daemon"
    );
    let health = json(&b, &["daemon"]);
    assert_ne!(health["pid"], a_info.pid, "{health}");
    assert_eq!(health["root"], root(&b).display().to_string());
    let a_health = json(&a, &["daemon"]);
    assert_eq!(
        a_health["pid"], a_info.pid,
        "A's daemon was left running: {a_health}"
    );
    ok(&a, &["daemon", "stop"]);
    ok(&b, &["daemon", "stop"]);
}

#[test]
fn deleting_the_lock_and_the_socket_leaves_one_writer() {
    // FR-082: flock stays on the unlinked inode, and a missing socket is not a live listener.
    // Removing both at once must not leave two daemons writing the store, and the one that
    // stands down must not unlink the survivor's socket.
    let repo = TestRepo::with_project_daemon();
    ok(&repo, &["status"]);
    let first = info(&repo);
    let endpoint = std::path::PathBuf::from(&first.endpoint);
    fs::remove_file(repo.path().join(".resonance/local/daemon.lock")).unwrap();
    fs::remove_file(&endpoint).unwrap();

    let mut child = daemon_cmd(&repo, &["daemon", "run"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("timed out with two daemons, or with none");
        }
        if let Some(status) = child.try_wait().unwrap() {
            let mut err = String::new();
            if let Some(mut stderr) = child.stderr.take() {
                use std::io::Read;
                stderr.read_to_string(&mut err).unwrap();
            }
            assert!(
                !status.success(),
                "a second daemon started and then exited on its own: {err}"
            );
            assert!(
                err.contains("already running"),
                "the first daemon reclaimed both files: {err}"
            );
            let health = json(&repo, &["daemon"]);
            assert_eq!(
                health["pid"], first.pid,
                "the first daemon is still the writer"
            );
            assert!(endpoint.exists(), "the survivor's socket stays");
            ok(&repo, &["daemon", "stop"]);
            assert!(!endpoint.exists(), "the owner removes its own socket");
            break;
        }
        if let Some(now) = runtime::read_info(&root(&repo))
            && now.pid != first.pid
            && endpoint.exists()
            && !runtime::process_alive(first.pid)
        {
            // The second daemon waited out the first and is the only writer.
            let health = json(&repo, &["daemon"]);
            assert_eq!(health["pid"], now.pid);
            assert!(endpoint.exists());
            ok(&repo, &["daemon", "stop"]);
            assert!(
                !endpoint.exists(),
                "stopping the successor must not have been undone by the first daemon"
            );
            let status = child.wait().unwrap();
            assert!(
                status.success(),
                "the successor daemon exits cleanly when stopped"
            );
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_deleted_socket_is_bound_again_and_clients_reconnect() {
    let repo = TestRepo::with_project_daemon();
    ok(&repo, &["status"]);
    let first = info(&repo).pid;
    let endpoint = std::path::PathBuf::from(info(&repo).endpoint);
    fs::remove_file(&endpoint).unwrap();
    assert!(
        runtime::lock_held(&root(&repo)),
        "the lock still shows the daemon"
    );

    // The next command reaches the same daemon once it has bound its socket again, without
    // waiting for the daemon to go idle.
    let started = Instant::now();
    let health = json(&repo, &["daemon"]);
    assert_eq!(health["pid"], first, "{health}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert!(endpoint.exists());
    ok(&repo, &["daemon", "stop"]);
    assert!(
        !endpoint.exists(),
        "its own socket is removed when it stops"
    );
}
