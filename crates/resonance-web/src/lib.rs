//! The web UI's server (FR-286–FR-292, MVP-043), hosted in the daemon's process.
//!
//! It binds `127.0.0.1` only, on this working tree's last port when free, else on one the
//! system assigns (or `ui.port`). People do not log in, but every request must carry the
//! working tree's token as a cookie, a loopback `Host`, and, for a `POST`, its own `Origin`
//! (FR-288). `resonance ui` hands the token over through `/auth`.
//!
//! - `GET /auth?token=…&next=…` sets the cookie and redirects to a local path;
//! - `POST /api/call` runs one allowed operation, as the person, with origin `ui`;
//! - `GET /api/events` streams change notifications, diagnostics events and log records
//!   (Server-Sent Events);
//! - anything else is the embedded bundle.

mod http;

use std::io::Read;
use std::net::SocketAddr;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use resonance_core::daemon::runtime::{self, UiState};
use resonance_core::daemon::{Handle, Hosted};
use resonance_core::error::Error;
use resonance_core::ops::Actor;
use resonance_core::repo::config::UiConfig;
use resonance_core::service::Call;
use serde_json::{Value, json};
use tiny_http::{Method, Request, Server};

pub use http::{local_next, percent_decode, query_param};

mod assets {
    include!(concat!(env!("OUT_DIR"), "/assets.rs"));
}

/// Whether this build embeds the real UI rather than the placeholder page.
pub const BUNDLED: bool = assets::BUILT;

/// Operations the UI may run through the engine (MVP-043). Anything else is refused.
pub const ENGINE_METHODS: &[&str] = &[
    "status",
    "spec_list",
    "spec_show",
    "show",
    "file_text",
    "spec_suggest_key",
    "spec_add",
    "spec_edit",
    "spec_retire",
    "changes",
    "changes_show",
    "changes_done",
    "trace",
    "why",
    "search",
    "repair_list",
    // Operational actions (FR-295).
    "reconcile",
    "rebuild_derived",
];

/// Operations the daemon answers itself (MVP-044, FR-295).
pub const DAEMON_METHODS: &[&str] = &[
    "session",
    "health",
    "diagnostics",
    "logs",
    "log_level",
    "daemon_restart",
    "daemon_stop",
];

/// Largest request body accepted.
const MAX_BODY: u64 = 4 << 20;
/// How often an idle event stream sends a comment, to notice a closed page.
const KEEPALIVE: Duration = Duration::from_secs(15);

struct Ctx {
    handle: Handle,
    person: Actor,
    port: u16,
    token: String,
    cookie: String,
}

/// Start the web UI for the daemon behind `handle`, as `person`. Returns `None` (and says
/// why in the daemon's health) when it is turned off or cannot listen.
pub fn host(handle: Handle, person: Actor, cfg: &UiConfig) -> Option<Hosted> {
    if !cfg.enabled {
        handle.set_ui_error("the web UI is turned off (ui.enabled = false)");
        return None;
    }
    match start(handle.clone(), person, cfg) {
        Ok(h) => Some(h),
        Err(e) => {
            let msg = format!("the web UI could not start: {e}");
            tracing::error!("{msg}");
            handle.set_ui_error(&msg);
            handle.diag.record(
                "ui",
                "the web UI could not start",
                json!({"error": e.to_string()}),
            );
            None
        }
    }
}

fn start(handle: Handle, person: Actor, cfg: &UiConfig) -> Result<Hosted, Error> {
    let repo = handle.repo.clone();
    let saved = runtime::read_ui(&repo);
    let token = saved
        .as_ref()
        .map(|s| s.token.clone())
        .map(Ok)
        .unwrap_or_else(new_token)?;
    let mut candidates = Vec::new();
    if cfg.port != 0 {
        candidates.push(cfg.port);
    }
    if let Some(s) = &saved
        && s.port != 0
        && !candidates.contains(&s.port)
    {
        candidates.push(s.port);
    }
    let (server, port) = bind(&candidates)?;
    runtime::write_ui(
        &repo,
        &UiState {
            port,
            token: token.clone(),
        },
    )?;
    let url = format!("http://127.0.0.1:{port}/");
    tracing::info!(port, bundled = BUNDLED, "web UI listening on {url}");
    handle.diag.record(
        "ui",
        format!("listening on {url}"),
        json!({"port": port, "bundled": BUNDLED, "reused": candidates.contains(&port)}),
    );
    let ctx = Arc::new(Ctx {
        cookie: runtime::ui_cookie(&repo.root),
        handle,
        person,
        port,
        token,
    });
    let thread = thread::Builder::new()
        .name("resonance-web".into())
        .spawn(move || {
            while !ctx.handle.stopping() {
                match server.recv_timeout(Duration::from_millis(200)) {
                    Ok(Some(req)) => {
                        let ctx = Arc::clone(&ctx);
                        thread::spawn(move || serve(&ctx, req));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!("web UI: {e}");
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
            tracing::debug!("web UI stopped");
        })
        .map_err(|e| Error::io("starting the web UI", e))?;
    Ok(Hosted { url, thread })
}

/// Listen on the first free candidate port, else on one the system assigns. A port held a
/// moment longer by a daemon that is restarting is retried briefly.
fn bind(candidates: &[u16]) -> Result<(Server, u16), Error> {
    for &port in candidates {
        for _ in 0..10 {
            if let Ok(s) = Server::http(SocketAddr::from(([127, 0, 0, 1], port))) {
                return Ok((s, port));
            }
            thread::sleep(Duration::from_millis(50));
        }
        tracing::info!(port, "port {port} is taken; choosing another");
    }
    let s = Server::http(SocketAddr::from(([127, 0, 0, 1], 0)))
        .map_err(|e| Error::Daemon(format!("listening on 127.0.0.1: {e}")))?;
    let port = s
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or_else(|| Error::Daemon("the web UI's listener has no port".into()))?;
    Ok((s, port))
}

/// 32 random bytes, as hex.
fn new_token() -> Result<String, Error> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| Error::io("reading /dev/urandom for the web UI's token", e))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn serve(ctx: &Ctx, req: Request) {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    let path = path.to_string();
    let query = query.to_string();
    if !http::host_allowed(http::header(&req, "Host").as_deref(), ctx.port) {
        tracing::warn!(host = %http::header(&req, "Host").unwrap_or_default(), "refused a request for another host");
        return http::respond_text(
            req,
            403,
            "Forbidden: this server answers only on 127.0.0.1 and localhost.",
        );
    }
    if path == "/auth" {
        return auth(ctx, req, &query);
    }
    let authorized = http::cookie(&req, &ctx.cookie)
        .is_some_and(|t| http::same_secret(t.as_bytes(), ctx.token.as_bytes()));
    if !authorized {
        if path.starts_with("/api/") {
            return http::respond_json(
                req,
                401,
                &json!({"error": {"message": "not signed in to this daemon's web UI; run `resonance ui`", "exitCode": 1}}),
            );
        }
        return http::respond_html(req, 403, FORBIDDEN_PAGE);
    }
    match (req.method(), path.as_str()) {
        (Method::Post, "/api/call") => {
            if !http::origin_allowed(http::header(&req, "Origin").as_deref(), ctx.port, true) {
                return http::respond_json(
                    req,
                    403,
                    &json!({"error": {"message": "refused: a request from another origin", "exitCode": 1}}),
                );
            }
            api_call(ctx, req)
        }
        (Method::Get, "/api/events") => {
            if !http::origin_allowed(http::header(&req, "Origin").as_deref(), ctx.port, false) {
                return http::respond_text(req, 403, "refused: a request from another origin");
            }
            events(ctx, req)
        }
        (Method::Get, _) | (Method::Head, _) => static_file(req, &path),
        _ => http::respond_text(req, 405, "method not allowed"),
    }
}

const FORBIDDEN_PAGE: &str = "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Resonance</title>\
<body style=\"font-family:system-ui,sans-serif;max-width:40rem;margin:4rem auto\">\
<h1>Resonance</h1><p>This page needs this repository's key. Run <code>resonance ui</code> in the repository to open it.</p></body></html>";

fn auth(ctx: &Ctx, req: Request, query: &str) {
    let token = query_param(query, "token").unwrap_or_default();
    if !http::same_secret(token.as_bytes(), ctx.token.as_bytes()) {
        tracing::warn!("refused a web UI sign-in with a wrong token");
        return http::respond_html(req, 403, FORBIDDEN_PAGE);
    }
    let next = query_param(query, "next")
        .filter(|n| local_next(n))
        .unwrap_or_else(|| "/".into());
    let cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Strict",
        ctx.cookie, ctx.token
    );
    http::redirect(req, &next, &cookie);
}

fn api_call(ctx: &Ctx, mut req: Request) {
    let client = http::header(&req, "X-Resonance-Client").and_then(|c| c.parse::<u64>().ok());
    let mut body = String::new();
    if req
        .as_reader()
        .take(MAX_BODY)
        .read_to_string(&mut body)
        .is_err()
    {
        return http::respond_json(
            req,
            400,
            &json!({"error": {"message": "unreadable request body", "exitCode": 2}}),
        );
    }
    let v: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            return http::respond_json(
                req,
                400,
                &json!({"error": {"message": format!("not JSON: {e}"), "exitCode": 2}}),
            );
        }
    };
    let method = v["method"].as_str().unwrap_or_default().to_string();
    let args = if v["args"].is_null() {
        json!({})
    } else {
        v["args"].clone()
    };
    let result = run(
        ctx,
        &method,
        args,
        v["cached"].as_bool().unwrap_or(false),
        client,
    );
    match result {
        Ok(v) => http::respond_json(req, 200, &json!({"result": v})),
        Err((status, e)) => http::respond_json(
            req,
            status,
            &json!({"error": {"message": e.to_string(), "exitCode": e.exit_code()}}),
        ),
    }
}

/// Run one operation for the UI.
fn run(
    ctx: &Ctx,
    method: &str,
    args: Value,
    cached: bool,
    client: Option<u64>,
) -> Result<Value, (u16, Error)> {
    let actor = ctx.person.to_string();
    let h = &ctx.handle;
    if DAEMON_METHODS.contains(&method) {
        return match method {
            "session" => Ok(json!({
                "actor": actor,
                "root": h.repo.root.display().to_string(),
                "repository": h.repo.name(),
                "branch": h.repo.git(&["branch", "--show-current"]).ok(),
                "ui": h.health().ui,
                "bundled": BUNDLED,
            })),
            "health" => Ok(serde_json::to_value(h.health()).unwrap_or_default()),
            "diagnostics" => Ok(h.diagnostics(&args)),
            "logs" => Ok(h.log_records(&args)),
            "log_level" => h
                .set_log_level(args["level"].as_str().unwrap_or_default(), Some(&actor))
                .map_err(|e| (400, e)),
            "daemon_restart" | "daemon_stop" => {
                let restart = method == "daemon_restart";
                h.diag.record(
                    "action",
                    if restart {
                        "restart requested"
                    } else {
                        "stop requested"
                    },
                    json!({"action": method, "actor": actor}),
                );
                tracing::info!(actor = %actor, "{} requested from the web UI", if restart { "restart" } else { "stop" });
                // Answer first; the daemon stops after the response has gone out.
                let h = h.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(100));
                    let _ = h.shutdown(restart);
                });
                Ok(json!({"stopping": true, "restart": restart}))
            }
            _ => unreachable!("listed in DAEMON_METHODS"),
        };
    }
    if !ENGINE_METHODS.contains(&method) {
        return Err((
            403,
            Error::Usage(format!("`{method}` is not available in the web UI")),
        ));
    }
    if matches!(method, "reconcile" | "rebuild_derived") {
        h.diag.record(
            "action",
            format!("{method} requested"),
            json!({"action": method, "actor": actor, "args": args}),
        );
    }
    let mut call = Call::new(method, args);
    call.actor = Some(ctx.person.clone());
    call.cwd = Some(h.repo.root.clone());
    call.origin = Some("ui".into());
    call.cached = cached;
    h.call(call, client).map_err(|e| {
        let status = match &e {
            Error::Usage(_) | Error::Invalid(_) => 400,
            _ => 422,
        };
        (status, e)
    })
}

/// The event stream (FR-290): change notifications, diagnostics events and log records. Every
/// diagnostics event and log record carries `id: <event seq>.<log seq>`, so a reconnecting
/// browser resumes with `Last-Event-ID`, or is told to reload (`reset`) when the buffers no
/// longer hold the gap.
fn events(ctx: &Ctx, req: Request) {
    let h = &ctx.handle;
    let resume = http::header(&req, "Last-Event-ID").and_then(|id| {
        let (e, l) = id.split_once('.')?;
        Some((e.parse::<u64>().ok()?, l.parse::<u64>().ok()?))
    });
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    // Listen before reading the buffers, so that nothing falls between the two.
    if !h.subscribe(tx.clone()) {
        return http::respond_text(req, 503, "the daemon is stopping");
    }
    h.diag.listen(tx.clone());
    h.logs.listen(tx);
    let client = h.diag.connected("ui");
    h.diag.identify(client, "ui", Some(&ctx.person.to_string()));
    let mut out = req.into_writer();
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n{}\r\n",
        http::SECURITY_HEADERS
            .iter()
            .map(|(k, v)| format!("{k}: {v}\r\n"))
            .collect::<String>()
    );
    let mut stream = http::EventStream::new(&mut out);
    let mut last_event = h.diag.last_seq();
    let mut last_log = h.logs.last_seq();
    let result = (|| -> std::io::Result<()> {
        stream.raw(head.as_bytes())?;
        // Sequence numbers start again with each daemon: a cursor beyond them belongs to the
        // previous one, so the page starts afresh.
        if let Some((e, l)) = resume {
            let other_daemon = e > h.diag.last_seq() || l > h.logs.last_seq();
            let replay = if other_daemon {
                None
            } else {
                h.diag.after(e).zip(h.logs.after(l))
            };
            match replay {
                Some((events, records)) => {
                    last_event = e;
                    last_log = l;
                    for ev in events {
                        last_event = ev.seq;
                        stream.send(Some((last_event, last_log)), "event", &json!(ev))?;
                    }
                    for r in records {
                        last_log = r.seq;
                        stream.send(Some((last_event, last_log)), "log", &json!(r))?;
                    }
                }
                None => stream.send(
                    Some((last_event, last_log)),
                    "reset",
                    &json!({"reason": "the daemon restarted, or dropped events while you were away"}),
                )?,
            }
        }
        stream.send(
            Some((last_event, last_log)),
            "hello",
            &json!({
                "client": client,
                "eventSeq": last_event,
                "logSeq": last_log,
                "pid": h.health().pid,
                "startedAt": h.health().started_at,
            }),
        )?;
        let mut idle = Duration::ZERO;
        loop {
            if h.stopping() {
                stream.send(None, "bye", &json!({"reason": "the daemon is stopping"}))?;
                return Ok(());
            }
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(msg) => {
                    let method = msg["method"].as_str().unwrap_or_default();
                    if method == "ping" {
                        // The daemon could go idle and asks whether this page is still open:
                        // only a write tells.
                        stream.raw(b": ping\n\n")?;
                        continue;
                    }
                    idle = Duration::ZERO;
                    let params = &msg["params"];
                    match method {
                        "event" => {
                            let seq = params["seq"].as_u64().unwrap_or(0);
                            if seq <= last_event {
                                continue;
                            }
                            last_event = seq;
                            stream.send(Some((last_event, last_log)), "event", params)?;
                        }
                        "log" => {
                            let seq = params["seq"].as_u64().unwrap_or(0);
                            if seq <= last_log {
                                continue;
                            }
                            last_log = seq;
                            stream.send(Some((last_event, last_log)), "log", params)?;
                        }
                        "changed" => stream.send(None, "changed", params)?,
                        _ => {}
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    idle += Duration::from_secs(1);
                    if idle >= KEEPALIVE {
                        idle = Duration::ZERO;
                        stream.raw(b": keepalive\n\n")?;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    })();
    drop(result);
    h.diag.disconnected(client);
}

fn static_file(req: Request, path: &str) {
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let found = assets::ASSETS.iter().find(|(p, _)| *p == rel);
    let (name, bytes) = match found {
        Some((p, b)) => (*p, *b),
        // Client-side routes (`/spec/FR-001`) get the app; missing files are not found.
        None if !rel.rsplit('/').next().unwrap_or("").contains('.') || rel.starts_with("spec/") => {
            match assets::ASSETS.iter().find(|(p, _)| *p == "index.html") {
                Some((p, b)) => (*p, *b),
                None => return http::respond_text(req, 404, "not found"),
            }
        }
        None => return http::respond_text(req, 404, "not found"),
    };
    let cache = if name.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    http::respond_bytes(req, 200, http::content_type(name), cache, bytes);
}
