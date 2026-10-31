//! The web UI, diagnostics and logs (S27, mvp.md §3.20, MVP-043, MVP-044), driven through the
//! HTTP API as the browser uses it. These tests always use daemons: the web UI is the daemon's.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

use common::TestRepo;
use resonance_core::daemon::runtime;
use serde_json::{Value, json};

struct Ui {
    port: u16,
    token: String,
    cookie: String,
}

/// `resonance ui --no-open --json`, then sign in through `/auth`.
fn open(repo: &TestRepo) -> Ui {
    let v = repo.json(&["ui", "--no-open"]);
    let port = v["port"].as_u64().expect("a port") as u16;
    let url = v["url"].as_str().expect("a url").to_string();
    let token = url
        .split("token=")
        .nth(1)
        .and_then(|t| t.split('&').next())
        .expect("a token")
        .to_string();
    let (status, headers, _) = http(
        port,
        "GET",
        &format!("/auth?token={token}&next=%2Fspec"),
        &[],
        None,
    );
    assert_eq!(status, 303);
    assert_eq!(header(&headers, "location").as_deref(), Some("/spec"));
    let set = header(&headers, "set-cookie").expect("a cookie");
    assert!(
        set.contains("HttpOnly") && set.contains("SameSite=Strict"),
        "{set}"
    );
    let cookie = set.split(';').next().unwrap().to_string();
    Ui {
        port,
        token,
        cookie,
    }
}

fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
}

/// One HTTP/1.1 request. `Host` defaults to this server's.
fn http(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, Vec<(String, String)>, String) {
    // A daemon that is restarting refuses connections for a moment: status 0.
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else {
        return (0, Vec::new(), String::new());
    };
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        req.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    let body = body.unwrap_or("");
    req.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
    s.write_all(req.as_bytes()).unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).unwrap();
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    (status, headers, rest.to_string())
}

/// `POST /api/call` as the page does: with the cookie and its own `Origin`.
fn call(ui: &Ui, method: &str, args: Value) -> (u16, Value) {
    let origin = format!("http://127.0.0.1:{}", ui.port);
    let (status, _, body) = http(
        ui.port,
        "POST",
        "/api/call",
        &[
            ("Cookie", &ui.cookie),
            ("Origin", &origin),
            ("Content-Type", "application/json"),
        ],
        Some(&json!({"method": method, "args": args}).to_string()),
    );
    (status, serde_json::from_str(&body).unwrap_or(Value::Null))
}

fn ok(ui: &Ui, method: &str, args: Value) -> Value {
    let (status, v) = call(ui, method, args);
    assert_eq!(status, 200, "{method}: {v}");
    v["result"].clone()
}

/// An open event stream: its lines, as they arrive.
fn events(ui: &Ui, last_event_id: Option<&str>) -> std::sync::mpsc::Receiver<String> {
    events_on(ui, last_event_id).0
}

/// An open event stream, and its connection, to close it the way a closed page does.
fn events_on(
    ui: &Ui,
    last_event_id: Option<&str>,
) -> (std::sync::mpsc::Receiver<String>, TcpStream) {
    let mut s = TcpStream::connect(("127.0.0.1", ui.port)).unwrap();
    let conn = s.try_clone().unwrap();
    let mut req = format!(
        "GET /api/events HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nCookie: {}\r\nAccept: text/event-stream\r\n",
        ui.port, ui.cookie
    );
    if let Some(id) = last_event_id {
        req.push_str(&format!("Last-Event-ID: {id}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(s).lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });
    (rx, conn)
}

/// Wait for a line equal to `want`; returns the lines seen before it.
fn wait_for(rx: &std::sync::mpsc::Receiver<String>, want: &str) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        if let Ok(l) = rx.recv_timeout(Duration::from_millis(200)) {
            if l == want {
                return seen;
            }
            seen.push(l);
        }
    }
    panic!("no `{want}` in the event stream; saw {seen:?}");
}

fn wait_until(what: &str, timeout: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn info(repo: &TestRepo) -> runtime::DaemonInfo {
    runtime::read_info(&repo.path().canonicalize().unwrap()).expect("daemon.json")
}

#[test]
fn ui_starts_the_daemon_and_records_its_address() {
    let repo = TestRepo::with_project_daemon();
    let ui = open(&repo);
    let info = info(&repo);
    assert_eq!(
        info.ui.as_deref(),
        Some(format!("http://127.0.0.1:{}/", ui.port).as_str())
    );
    let path = repo.path().join(".resonance/local/ui.json");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let state: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(state["token"], ui.token);
    // Nothing the web UI keeps shows in git.
    assert_eq!(repo.git_status(), "");
    // The page itself.
    let (status, headers, body) = http(
        ui.port,
        "GET",
        "/spec/FR-001",
        &[("Cookie", &ui.cookie)],
        None,
    );
    assert_eq!(status, 200);
    assert!(body.contains("<html"), "{body}");
    assert!(
        header(&headers, "content-security-policy")
            .is_some_and(|c| c.contains("frame-ancestors 'none'"))
    );
    assert!(header(&headers, "access-control-allow-origin").is_none());
}

#[test]
fn the_ui_is_not_offered_in_direct_mode() {
    let repo = TestRepo::with_project_daemon();
    let err = repo.fails(&["--no-daemon", "ui", "--no-open"], 2);
    assert!(err.contains("not available with --no-daemon"), "{err}");
}

#[test]
fn requests_need_the_token_a_loopback_host_and_their_own_origin() {
    let repo = TestRepo::with_project_daemon();
    let ui = open(&repo);
    let body = Some(r#"{"method":"status"}"#);
    let origin = format!("http://127.0.0.1:{}", ui.port);
    // No cookie.
    let (status, _, _) = http(ui.port, "POST", "/api/call", &[("Origin", &origin)], body);
    assert_eq!(status, 401);
    let (status, _, page) = http(ui.port, "GET", "/", &[], None);
    assert_eq!(status, 403);
    assert!(page.contains("resonance ui"), "{page}");
    // Another host (DNS rebinding).
    let (status, _, _) = http(
        ui.port,
        "GET",
        "/",
        &[("Host", "evil.example"), ("Cookie", &ui.cookie)],
        None,
    );
    assert_eq!(status, 403);
    // A wrong token signs nobody in; `next` stays on this server.
    let (status, _, _) = http(ui.port, "GET", "/auth?token=nope", &[], None);
    assert_eq!(status, 403);
    let (status, headers, _) = http(
        ui.port,
        "GET",
        &format!("/auth?token={}&next=%2F%2Fevil.example", ui.token),
        &[],
        None,
    );
    assert_eq!(status, 303);
    assert_eq!(header(&headers, "location").as_deref(), Some("/"));
    // A POST from another origin, or with none.
    for o in [Some("http://evil.example"), None] {
        let mut h = vec![("Cookie", ui.cookie.as_str())];
        if let Some(o) = o {
            h.push(("Origin", o));
        }
        let (status, _, _) = http(ui.port, "POST", "/api/call", &h, body);
        assert_eq!(status, 403, "origin {o:?}");
    }
    // Methods outside the allowlist.
    let (status, v) = call(&ui, "link", json!({}));
    assert_eq!(status, 403, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not available in the web UI")
    );
    // And with everything right.
    assert!(ok(&ui, "status", json!({}))["counts"].is_object());
}

#[test]
fn spec_work_goes_to_the_change_log_as_the_person_from_the_ui() {
    let repo = TestRepo::with_project_daemon();
    let ui = open(&repo);
    let key = ok(&ui, "spec_suggest_key", json!({"type": "Requirement"}))["key"]
        .as_str()
        .unwrap()
        .to_string();
    let added = ok(
        &ui,
        "spec_add",
        json!({"type": "Requirement", "key": key, "fields": {"text": "The UI shall greet."}}),
    );
    assert_eq!(added["key"], key.as_str());
    // An existing key is refused (FR-273).
    let (status, v) = call(
        &ui,
        "spec_add",
        json!({"type": "Requirement", "key": key, "fields": {"text": "Again"}}),
    );
    assert_ne!(status, 200, "{v}");
    ok(
        &ui,
        "spec_edit",
        json!({"ref": key, "fields": {"priority": "high"}}),
    );
    let log = ok(&ui, "changes", json!({}));
    let entry = log
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["item"]["key"] == key.as_str())
        .expect("an entry");
    assert_eq!(entry["origin"], "ui");
    assert_eq!(entry["actor"], "human:dev@example.com");
    // Retiring keeps the item, and clearing removes the entry from the log only.
    let retired = ok(&ui, "spec_retire", json!({"ref": key}));
    assert_eq!(retired["item"]["status"], "retired");
    let seqs: Vec<u64> = ok(&ui, "changes", json!({}))
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["seq"].as_u64().unwrap())
        .collect();
    ok(
        &ui,
        "changes_done",
        json!({"seqs": seqs, "note": "cleared in the web UI"}),
    );
    assert!(ok(&ui, "changes", json!({})).as_array().unwrap().is_empty());
    assert_eq!(
        ok(&ui, "spec_show", json!({"ref": key}))["status"],
        "retired"
    );
}

#[test]
fn the_event_stream_delivers_changes_and_resumes() {
    let repo = TestRepo::with_project_daemon();
    let ui = open(&repo);
    let rx = events(&ui, None);
    wait_for(&rx, "event: hello");
    // A change made by an agent over MCP reaches the page.
    repo.tool(
        "resonance_create_requirement",
        json!({"key": "FR-300", "text": "From the agent"}),
    );
    wait_for(&rx, "event: changed");
    // A reconnecting page gets what it missed, before the hello.
    let replay = events(&ui, Some("0.0"));
    let before = wait_for(&replay, "event: hello");
    assert!(before.iter().any(|l| l == "event: event"), "{before:?}");
    // A cursor from another daemon starts the page afresh.
    let fresh = events(&ui, Some("999999.999999"));
    let before = wait_for(&fresh, "event: hello");
    assert!(before.iter().any(|l| l == "event: reset"), "{before:?}");
}

#[test]
fn an_open_page_keeps_the_daemon_and_a_closed_one_does_not() {
    let repo = TestRepo::with_project_daemon();
    repo.ok(&["daemon", "stop"]);
    let out = repo
        .command(&["--json", "ui", "--no-open"])
        .env_remove("RESONANCE_NO_DAEMON")
        .env("RESONANCE_DAEMON_IDLE_SECONDS", "2")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let url = v["url"].as_str().unwrap();
    let port = v["port"].as_u64().unwrap() as u16;
    let token = url
        .split("token=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap();
    let (_, headers, _) = http(port, "GET", &format!("/auth?token={token}"), &[], None);
    let cookie = header(&headers, "set-cookie")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let ui = Ui {
        port,
        token: token.into(),
        cookie,
    };
    let pid = info(&repo).pid;
    let (rx, conn) = events_on(&ui, None);
    wait_for(&rx, "event: hello");
    thread::sleep(Duration::from_secs(5));
    assert!(
        runtime::process_alive(pid),
        "an open page keeps the daemon running"
    );
    // Closing the page: the daemon notices at the stream's next keepalive.
    conn.shutdown(std::net::Shutdown::Both).unwrap();
    drop(rx);
    wait_until("the daemon to go idle", Duration::from_secs(45), || {
        !runtime::process_alive(pid)
    });
}

#[test]
fn daemons_take_different_ports_and_a_restart_keeps_its_own() {
    let a = TestRepo::with_project_daemon();
    let b = TestRepo::with_project_daemon();
    let ua = open(&a);
    let ub = open(&b);
    assert_ne!(ua.port, ub.port);
    // Each daemon reads only its own cookie.
    let (status, _) = call(
        &Ui {
            port: ub.port,
            token: ub.token.clone(),
            cookie: ua.cookie.clone(),
        },
        "status",
        json!({}),
    );
    assert_eq!(status, 401);
    let before = info(&a).pid;
    let v = ok(&ua, "daemon_restart", json!({}));
    assert_eq!(v["restart"], true);
    wait_until("the next daemon", Duration::from_secs(30), || {
        runtime::read_info(&a.path().canonicalize().unwrap())
            .is_some_and(|i| i.pid != before && i.ui.is_some())
    });
    let after = info(&a);
    assert_eq!(
        after.ui.as_deref(),
        Some(format!("http://127.0.0.1:{}/", ua.port).as_str())
    );
    // The page's cookie still works with the successor.
    wait_until("the successor to answer", Duration::from_secs(30), || {
        call(&ua, "health", json!({})).0 == 200
    });
    assert_eq!(ok(&ua, "health", json!({}))["pid"], after.pid);
}

#[test]
fn diagnostics_report_activity_without_content() {
    let repo = TestRepo::with_project_daemon();
    let ui = open(&repo);
    ok(
        &ui,
        "spec_add",
        json!({"type": "Requirement", "key": "FR-400", "fields": {"text": "SECRET-ITEM-TEXT"}}),
    );
    ok(&ui, "reconcile", json!({}));
    let d = ok(&ui, "diagnostics", json!({}));
    let kinds: Vec<&str> = d["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    for k in ["ui", "state", "reconcile", "request", "changeset", "action"] {
        assert!(kinds.contains(&k), "no {k} event in {kinds:?}");
    }
    let methods = d["counters"]["methods"].as_array().unwrap();
    let add = methods
        .iter()
        .find(|m| m["method"] == "spec_add")
        .expect("spec_add counted");
    assert_eq!(add["count"], 1);
    assert_eq!(d["identity"]["pid"], info(&repo).pid);
    assert!(
        d["config"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["key"] == "log.level")
    );
    assert!(
        !d.to_string().contains("SECRET-ITEM-TEXT"),
        "item text leaked into diagnostics"
    );
    // Filtering by kind.
    let only = ok(&ui, "diagnostics", json!({"kinds": ["action"]}));
    assert!(
        only["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["kind"] == "action")
    );
    // A small buffer drops the oldest events and says so; counters stay.
    let config = repo.read(".resonance/config.toml");
    assert!(config.contains("bufferEvents = 10000"));
    repo.write(
        ".resonance/config.toml",
        &config.replace("bufferEvents = 10000", "bufferEvents = 3"),
    );
    ok(&ui, "daemon_restart", json!({}));
    let before = info(&repo).pid;
    wait_until("the next daemon", Duration::from_secs(30), || {
        runtime::read_info(&repo.path().canonicalize().unwrap()).is_some_and(|i| i.pid != before)
            && call(&ui, "health", json!({})).0 == 200
    });
    for _ in 0..5 {
        ok(&ui, "status", json!({}));
    }
    let d = ok(&ui, "diagnostics", json!({}));
    assert_eq!(d["events"].as_array().unwrap().len(), 3);
    assert!(d["dropped"].as_u64().unwrap() > 0);
    let status = d["counters"]["methods"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["method"] == "status")
        .unwrap()
        .clone();
    assert!(status["count"].as_u64().unwrap() >= 5);
}

#[test]
fn logs_follow_the_level_and_go_to_the_configured_file() {
    let repo = TestRepo::with_project_daemon();
    let file = repo.home().join("logs/resonance.log");
    repo.ok(&["daemon", "stop"]);
    let out = repo
        .command(&["--json", "ui", "--no-open"])
        .env_remove("RESONANCE_NO_DAEMON")
        .env("RESONANCE_LOG_FILE", &file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let port = v["port"].as_u64().unwrap() as u16;
    let token = v["url"]
        .as_str()
        .unwrap()
        .split("token=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_string();
    let (_, headers, _) = http(port, "GET", &format!("/auth?token={token}"), &[], None);
    let cookie = header(&headers, "set-cookie")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let ui = Ui {
        port,
        token,
        cookie,
    };
    let logs = ok(&ui, "logs", json!({}));
    assert_eq!(logs["level"], "info");
    assert_eq!(logs["file"], file.display().to_string());
    assert!(
        logs["records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["component"] == "ui")
    );
    let set = ok(&ui, "log_level", json!({"level": "debug"}));
    assert_eq!(set["level"], "debug");
    let (status, _) = call(&ui, "log_level", json!({"level": "loud"}));
    assert_eq!(status, 400);
    assert_eq!(ok(&ui, "logs", json!({}))["level"], "debug");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("web UI listening"), "{text}");
    assert!(text.contains("log level changed"), "{text}");
    // Warnings and above only.
    let warn = ok(&ui, "logs", json!({"level": "warn"}));
    assert!(
        warn["records"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["level"] == "warn" || r["level"] == "error")
    );
}

#[test]
fn a_log_file_in_the_working_tree_is_refused() {
    let repo = TestRepo::with_project_daemon();
    let config = repo
        .read(".resonance/config.toml")
        .replace("file = \"\"", "file = \"logs/daemon.log\"");
    repo.write(".resonance/config.toml", &config);
    repo.ok(&["daemon", "stop"]);
    let ui = open(&repo);
    let logs = ok(&ui, "logs", json!({}));
    assert!(
        logs["file"]
            .as_str()
            .unwrap()
            .ends_with(".resonance/local/daemon.log"),
        "{logs}"
    );
    assert!(logs["records"].as_array().unwrap().iter().any(|r| {
        r["level"] == "warn"
            && r["message"]
                .as_str()
                .unwrap()
                .contains("outside .resonance/local/")
    }));
    assert!(!repo.path().join("logs").exists());
}

#[test]
fn file_text_serves_only_files_resonance_tracks() {
    let repo = TestRepo::with_project_daemon();
    let ui = open(&repo);
    let v = ok(&ui, "file_text", json!({"path": "src/store.rs"}));
    assert_eq!(v["path"], "src/store.rs");
    assert_eq!(v["text"], repo.read("src/store.rs").as_str());
    for path in [
        "../outside.txt",
        "/etc/passwd",
        ".resonance/local/ui.json",
        "no/such/file.rs",
    ] {
        let (status, v) = call(&ui, "file_text", json!({ "path": path }));
        assert_ne!(status, 200, "{path}: {v}");
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("not a file Resonance tracks"),
            "{path}: {v}"
        );
    }
}
