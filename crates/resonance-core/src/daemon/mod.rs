//! The per-repository daemon (§8.4, §8.7, MVP-019 to MVP-023).
//!
//! One owner thread holds the [`Engine`] and applies every change, whether it comes from the
//! file watcher or from a request (P1). Connection threads parse JSON-RPC requests from the
//! Unix socket and hand them to the owner. `health` is answered from shared state, so a daemon
//! that is busy reconciling still reports liveness (FR-095).
//!
//! Read-your-writes (FR-102, MVP-020): before answering a request, the owner writes a sync
//! cookie into `.resonance/cookies/` and answers only once the watcher has reported it and
//! every earlier change has been applied.

pub mod client;
pub mod runtime;
pub mod watch;

use std::collections::{BTreeSet, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use notify::{Event, EventKind};
use serde_json::{Value, json};

use crate::api::{Engine, VERSION};
use crate::error::{Error, Result};
use crate::ingest::walk::{compile_ignores, is_ignored_path};
use crate::ingest::{ReconcileOptions, ReconcileReport};
use crate::lifecycle::StoreFactory;
use crate::mcp::jsonrpc::{self, Incoming};
use crate::ops::{Actor, hlc};
use crate::repo::Repo;
use crate::service::{self, Call};
use crate::store::SCHEMA_VERSION;
pub use runtime::{DaemonInfo, Health};

/// How long changes settle before they are applied without a request waiting.
const DEBOUNCE: Duration = Duration::from_millis(50);
/// How long a request waits for its sync cookie before the daemon distrusts the watcher.
const COOKIE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long `.git/index.lock` may hold changes back before it is treated as stale (MVP-023).
const GIT_LOCK_TIMEOUT: Duration = Duration::from_secs(10);
/// Registry heartbeat interval (MVP-022).
const HEARTBEAT: Duration = Duration::from_secs(10);
/// JSON-RPC error code for operation failures; `data.exitCode` carries the CLI exit code.
pub const OPERATION_FAILED: i64 = -32000;

/// Options for running a daemon.
#[derive(Debug, Clone)]
pub struct DaemonOptions {
    pub binary_id: String,
    /// Overrides `daemon.idleSeconds`.
    pub idle: Option<Duration>,
}

enum Job {
    Fs(notify::Result<Event>),
    Call {
        call: Call,
        reply: Sender<Result<Value>>,
    },
    Sync {
        reply: Sender<Result<Value>>,
    },
    Subscribe(Sender<Value>),
    Shutdown {
        reply: Sender<Result<Value>>,
    },
}

enum Waiter {
    Call(Call, Sender<Result<Value>>),
    Sync(Sender<Result<Value>>),
}

struct Waiting {
    cookie: String,
    since: Instant,
    waiter: Waiter,
}

/// State shared with connection threads, for `health`.
struct Shared {
    health: Health,
}

fn rfc_now() -> String {
    hlc::rfc3339(hlc::now_ms())
}

/// The error object for a failed operation.
pub fn error_value(e: &Error) -> Value {
    json!({"code": OPERATION_FAILED, "message": e.to_string(), "data": {"exitCode": e.exit_code()}})
}

/// Run a daemon for `repo` until it is idle, asked to shut down, or fails.
pub fn run(
    repo: Repo,
    open_store: StoreFactory<'_>,
    actor: Actor,
    opts: DaemonOptions,
) -> Result<()> {
    if !repo.is_initialized() {
        return Err(Error::NotInitialized);
    }
    // One daemon per repository: the lock, not the registry, guarantees it (FR-082).
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(runtime::lock_path(&repo))
        .map_err(|e| Error::io("opening .resonance/daemon.lock", e))?;
    if lock.try_lock().is_err() {
        let pid = runtime::read_info(&repo.root)
            .map(|i| i.pid.to_string())
            .unwrap_or_else(|| "unknown".into());
        return Err(Error::AlreadyRunning(pid));
    }

    let dir = runtime::runtime_dir();
    runtime::ensure_dir(&dir)?;
    let endpoint = runtime::socket_path(&dir, &repo.root);
    let _ = fs::remove_file(&endpoint); // A stale socket: we hold the lock.
    let listener = UnixListener::bind(&endpoint)
        .map_err(|e| Error::io(format!("listening on {}", endpoint.display()), e))?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&endpoint, fs::Permissions::from_mode(0o600));
    }
    listener
        .set_nonblocking(true)
        .map_err(|e| Error::io("configuring the socket", e))?;

    let info = DaemonInfo {
        root: repo.root.display().to_string(),
        endpoint: endpoint.display().to_string(),
        pid: std::process::id(),
        version: VERSION.to_string(),
        binary_id: opts.binary_id.clone(),
        schema_version: SCHEMA_VERSION,
        started_at: rfc_now(),
    };
    fs::write(
        runtime::info_path(&repo),
        serde_json::to_string_pretty(&info)?,
    )
    .map_err(|e| Error::io("writing .resonance/daemon.json", e))?;
    runtime::add_known_root(&repo.root);
    eprintln!(
        "[{}] resonance daemon {} started for {} on {}",
        info.started_at, VERSION, info.root, info.endpoint
    );

    let shared = Arc::new(Mutex::new(Shared {
        health: Health {
            state: "starting".into(),
            ready: false,
            pid: info.pid,
            root: info.root.clone(),
            endpoint: info.endpoint.clone(),
            version: VERSION.into(),
            binary_id: opts.binary_id.clone(),
            schema_version: SCHEMA_VERSION,
            started_at: info.started_at.clone(),
            ..Default::default()
        },
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Job>();

    // Accept connections.
    let accept = {
        let tx = tx.clone();
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        thread::spawn(move || accept_loop(listener, tx, shared, stop))
    };
    // Registry heartbeat (P5: failures are ignored).
    let heartbeat = {
        let stop = Arc::clone(&stop);
        let info = info.clone();
        let dir = dir.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                crate::registry::heartbeat(&dir, &info);
                for _ in 0..(HEARTBEAT.as_millis() / 100) {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
            crate::registry::unregister(&dir, &info);
        })
    };

    let result = Owner::new(
        &repo,
        open_store,
        actor,
        Arc::clone(&shared),
        opts,
        tx.clone(),
    )
    .and_then(|owner| owner.run(rx));
    if let Err(e) = &result {
        eprintln!("[{}] daemon stopped with an error: {e}", rfc_now());
    }

    stop.store(true, Ordering::SeqCst);
    let _ = accept.join();
    let _ = heartbeat.join();
    // Remove our endpoint, unless a newer daemon already replaced it.
    if runtime::read_info(&repo.root).is_some_and(|i| i.pid == info.pid) {
        let _ = fs::remove_file(runtime::info_path(&repo));
    }
    let _ = fs::remove_file(&endpoint);
    let _ = fs::remove_dir_all(repo.resonance_dir().join("cookies"));
    eprintln!("[{}] daemon exited", rfc_now());
    drop(lock);
    result
}

fn accept_loop(
    listener: UnixListener,
    tx: Sender<Job>,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let tx = tx.clone();
                let shared = Arc::clone(&shared);
                thread::spawn(move || {
                    shared.lock().expect("shared").health.connections += 1;
                    let _ = serve_connection(stream, &tx, &shared);
                    shared.lock().expect("shared").health.connections -= 1;
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn write_line(stream: &mut UnixStream, v: &Value) -> std::io::Result<()> {
    let mut text = serde_json::to_string(v).unwrap_or_default();
    text.push('\n');
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

fn serve_connection(
    stream: UnixStream,
    tx: &Sender<Job>,
    shared: &Arc<Mutex<Shared>>,
) -> std::io::Result<()> {
    let mut writer = stream.try_clone()?;
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let (id, method, params) = match jsonrpc::parse(&line) {
            Ok(Incoming::Request { id, method, params }) => (id, method, params),
            Ok(_) => continue,
            Err(response) => {
                write_line(&mut writer, &response)?;
                continue;
            }
        };
        let response = match method.as_str() {
            "health" => {
                let h = shared.lock().expect("shared").health.clone();
                jsonrpc::result(id, serde_json::to_value(h).unwrap_or_default())
            }
            "call" => match serde_json::from_value::<Call>(params) {
                Ok(call) => {
                    let (rtx, rrx) = mpsc::channel();
                    if tx.send(Job::Call { call, reply: rtx }).is_err() {
                        return Ok(());
                    }
                    respond(id, rrx)
                }
                Err(e) => jsonrpc::error(id, jsonrpc::INVALID_PARAMS, &e.to_string()),
            },
            "sync" => {
                let (rtx, rrx) = mpsc::channel();
                if tx.send(Job::Sync { reply: rtx }).is_err() {
                    return Ok(());
                }
                respond(id, rrx)
            }
            "shutdown" => {
                let (rtx, rrx) = mpsc::channel();
                if tx.send(Job::Shutdown { reply: rtx }).is_err() {
                    return Ok(());
                }
                respond(id, rrx)
            }
            "subscribe" => {
                // Change stream (FR-104): after the confirmation, only notifications follow.
                let (stx, srx) = mpsc::channel();
                if tx.send(Job::Subscribe(stx)).is_err() {
                    return Ok(());
                }
                write_line(
                    &mut writer,
                    &jsonrpc::result(id, json!({"subscribed": true})),
                )?;
                shared.lock().expect("shared").health.subscribers += 1;
                let outcome = (|| -> std::io::Result<()> {
                    for note in srx {
                        write_line(&mut writer, &note)?;
                    }
                    Ok(())
                })();
                shared.lock().expect("shared").health.subscribers -= 1;
                return outcome;
            }
            other => jsonrpc::error(
                id,
                jsonrpc::METHOD_NOT_FOUND,
                &format!("method not found: {other}"),
            ),
        };
        write_line(&mut writer, &response)?;
    }
    Ok(())
}

fn respond(id: Value, rx: Receiver<Result<Value>>) -> Value {
    match rx.recv() {
        Ok(Ok(v)) => jsonrpc::result(id, v),
        Ok(Err(e)) => json!({"jsonrpc": "2.0", "id": id, "error": error_value(&e)}),
        Err(_) => jsonrpc::error(
            id,
            jsonrpc::INTERNAL_ERROR,
            "the daemon stopped before answering",
        ),
    }
}

/// The owner: the only code that touches the graph (P1).
struct Owner {
    engine: Engine,
    root: PathBuf,
    git_dir: PathBuf,
    cookie_dir: PathBuf,
    shared: Arc<Mutex<Shared>>,
    watch: Option<watch::Watch>,
    pending: BTreeSet<String>,
    full: bool,
    first_pending: Option<Instant>,
    last_event: Option<Instant>,
    events_received: u64,
    watermark: u64,
    waiting: Vec<Waiting>,
    seen_cookies: HashSet<String>,
    cookie_seq: u64,
    subscribers: Vec<Sender<Value>>,
    last_activity: Instant,
    idle: Duration,
    shutdown: Vec<Sender<Result<Value>>>,
    requests_served: u64,
}

impl Owner {
    fn new(
        repo: &Repo,
        open_store: StoreFactory<'_>,
        actor: Actor,
        shared: Arc<Mutex<Shared>>,
        opts: DaemonOptions,
        tx: Sender<Job>,
    ) -> Result<Owner> {
        let store = open_store(&repo.store_path())?;
        let engine = Engine::open(repo.clone(), store, actor, repo.root.clone())?;
        let git_dir = repo
            .git(&["rev-parse", "--absolute-git-dir"])
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo.root.join(".git"));
        let cookie_dir = repo.resonance_dir().join("cookies");
        let _ = fs::remove_dir_all(&cookie_dir);
        fs::create_dir_all(&cookie_dir).map_err(|e| Error::io("creating .resonance/cookies", e))?;
        let idle = opts.idle.unwrap_or(Duration::from_secs(
            engine.config.daemon.idle_seconds.max(1),
        ));
        let mut owner = Owner {
            root: repo.root.clone(),
            git_dir,
            cookie_dir,
            shared,
            watch: None,
            pending: BTreeSet::new(),
            full: false,
            first_pending: None,
            last_event: None,
            events_received: 0,
            watermark: 0,
            waiting: Vec::new(),
            seen_cookies: HashSet::new(),
            cookie_seq: 0,
            subscribers: Vec::new(),
            last_activity: Instant::now(),
            idle,
            shutdown: Vec::new(),
            requests_served: 0,
            engine,
        };
        // Watch before the startup reconcile, so that nothing changed during it is missed.
        let extra = vec![
            owner.cookie_dir.clone(),
            owner.git_dir.clone(),
            owner.git_dir.join("info"),
        ];
        let sink_tx = tx.clone();
        match watch::Watch::start(
            &owner.root,
            &owner.engine.config.ingest.ignore,
            &extra,
            move |ev| {
                let _ = sink_tx.send(Job::Fs(ev));
            },
        ) {
            Ok(w) => owner.watch = Some(w),
            Err(e) => owner.set_error(&format!("{e}; every request will reconcile instead")),
        }
        Ok(owner)
    }

    fn set_state(&self, state: &str) {
        let mut s = self.shared.lock().expect("shared");
        s.health.state = state.to_string();
        s.health.ready = matches!(state, "ready" | "degraded");
    }

    fn set_error(&self, e: &str) {
        eprintln!("[{}] {e}", rfc_now());
        self.shared.lock().expect("shared").health.last_error = Some(e.to_string());
    }

    fn publish(&mut self) {
        if self.pending.is_empty() && !self.full {
            // Nothing outstanding: every event received so far is accounted for.
            self.watermark = self.events_received;
        } else if self.git_lock_age().is_some_and(|a| a < GIT_LOCK_TIMEOUT) {
            // Changes are being held back until the git command finishes (MVP-023).
            self.set_state("git-operation");
        }
        let mut s = self.shared.lock().expect("shared");
        s.health.ingest_watermark = self.watermark;
        s.health.events_received = self.events_received;
        s.health.pending_events = self.pending.len() + usize::from(self.full);
        s.health.lag_ms = self
            .first_pending
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        s.health.requests_served = self.requests_served;
    }

    fn notify(&mut self, note: Value) {
        let msg = json!({"jsonrpc": "2.0", "method": "changed", "params": note});
        self.subscribers.retain(|s| s.send(msg.clone()).is_ok());
    }

    fn run(mut self, rx: Receiver<Job>) -> Result<()> {
        // FR-047: reconcile before serving anything (P4).
        self.set_state("reconciling");
        let report = self.engine.reconcile(ReconcileOptions::default())?;
        self.after_ingest(&report);
        self.set_state(if self.watch.is_some() {
            "ready"
        } else {
            "degraded"
        });
        self.publish();

        loop {
            let busy = !self.pending.is_empty() || self.full || !self.waiting.is_empty();
            let timeout = if busy {
                Duration::from_millis(10)
            } else {
                Duration::from_millis(500)
            };
            match rx.recv_timeout(timeout) {
                Ok(job) => self.handle(job),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            // Take everything that is already queued before acting on it.
            while let Ok(job) = rx.try_recv() {
                self.handle(job);
            }
            self.maybe_apply(false);
            self.serve_waiting();
            self.publish();
            if !self.shutdown.is_empty() {
                for r in self.shutdown.drain(..) {
                    let _ = r.send(Ok(json!({"stopping": true})));
                }
                break;
            }
            if !self.root.join(".resonance").join("graph.db").exists() {
                // `.resonance/` was removed (for example by `git clean -fdx`): nothing to serve.
                eprintln!("[{}] .resonance/ disappeared; exiting", rfc_now());
                for w in self.waiting.drain(..) {
                    let err = Err(Error::NotInitialized);
                    match w.waiter {
                        Waiter::Call(_, reply) | Waiter::Sync(reply) => {
                            let _ = reply.send(err);
                        }
                    }
                }
                break;
            }
            let quiet = self.waiting.is_empty()
                && self.subscribers.is_empty()
                && self.shared.lock().expect("shared").health.connections == 0;
            if quiet && self.last_activity.elapsed() >= self.idle {
                eprintln!("[{}] idle for {}s; exiting", rfc_now(), self.idle.as_secs());
                break;
            }
        }
        Ok(())
    }

    fn handle(&mut self, job: Job) {
        match job {
            Job::Fs(ev) => self.on_event(ev),
            Job::Call { call, reply } => {
                self.last_activity = Instant::now();
                if call.cached || self.watch.is_none() {
                    if self.watch.is_none() {
                        // No watcher: reconcile on demand, as in direct mode (P4).
                        self.full = true;
                        self.maybe_apply(true);
                    }
                    self.serve(call, reply);
                } else {
                    let cookie = self.new_cookie();
                    self.waiting.push(Waiting {
                        cookie,
                        since: Instant::now(),
                        waiter: Waiter::Call(call, reply),
                    });
                }
            }
            Job::Sync { reply } => {
                self.last_activity = Instant::now();
                let cookie = self.new_cookie();
                self.waiting.push(Waiting {
                    cookie,
                    since: Instant::now(),
                    waiter: Waiter::Sync(reply),
                });
            }
            Job::Subscribe(s) => {
                self.last_activity = Instant::now();
                self.subscribers.push(s);
            }
            Job::Shutdown { reply } => self.shutdown.push(reply),
        }
    }

    fn new_cookie(&mut self) -> String {
        self.cookie_seq += 1;
        let name = format!("{}-{}", std::process::id(), self.cookie_seq);
        if let Err(e) = fs::write(self.cookie_dir.join(&name), b"") {
            self.set_error(&format!("could not write a sync cookie: {e}"));
        }
        name
    }

    fn on_event(&mut self, ev: notify::Result<Event>) {
        let ev = match ev {
            Ok(ev) => ev,
            Err(e) => {
                self.set_error(&format!("file watcher error: {e}; reconciling everything"));
                self.mark_full();
                return;
            }
        };
        self.events_received += 1;
        if ev.need_rescan() {
            self.mark_full();
        }
        let is_dir_create = matches!(
            ev.kind,
            EventKind::Create(notify::event::CreateKind::Folder)
        );
        for path in &ev.paths {
            self.on_path(path, is_dir_create);
        }
    }

    fn mark_full(&mut self) {
        self.full = true;
        self.first_pending.get_or_insert_with(Instant::now);
        self.last_event = Some(Instant::now());
    }

    fn on_path(&mut self, path: &Path, is_dir_create: bool) {
        if let Ok(cookie) = path.strip_prefix(&self.cookie_dir) {
            let cookie = cookie.to_string_lossy().into_owned();
            if self.waiting.iter().any(|w| w.cookie == cookie) {
                self.seen_cookies.insert(cookie);
            }
            return;
        }
        if let Ok(rel) = path.strip_prefix(&self.git_dir) {
            // Only the exclude file matters: it changes which files are ingested.
            if rel == Path::new("info/exclude") {
                self.mark_full();
            }
            return;
        }
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return;
        };
        let rel = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if rel.is_empty() {
            self.mark_full();
            return;
        }
        if rel == ".resonance/config.toml" {
            let _ = self.engine.reload_config();
            self.mark_full();
            return;
        }
        if rel.starts_with(".resonance/")
            || rel == ".resonance"
            || rel.starts_with(".git/")
            || rel == ".git"
        {
            return;
        }
        if rel == ".gitignore" || rel.ends_with("/.gitignore") {
            self.mark_full();
            return;
        }
        let extra = compile_ignores(&self.engine.config.ingest.ignore).unwrap_or_default();
        if is_ignored_path(&rel, &extra) {
            return;
        }
        if (is_dir_create || path.is_dir())
            && let Some(w) = &mut self.watch
        {
            w.add_tree(&rel, &self.engine.config.ingest.ignore);
        }
        self.pending.insert(rel);
        self.first_pending.get_or_insert_with(Instant::now);
        self.last_event = Some(Instant::now());
    }

    /// Whether a git command is changing the working tree, and for how long (MVP-023).
    fn git_lock_age(&self) -> Option<Duration> {
        let lock = self.git_dir.join("index.lock");
        let md = fs::metadata(lock).ok()?;
        Some(md.modified().ok()?.elapsed().unwrap_or_default())
    }

    /// Apply pending changes once they have settled, or right away when `now` (a request is
    /// waiting). Changes made under `.git/index.lock` wait for the git command to finish.
    fn maybe_apply(&mut self, now: bool) {
        if self.pending.is_empty() && !self.full {
            return;
        }
        let settled = self.last_event.is_none_or(|t| t.elapsed() >= DEBOUNCE);
        if !now && !settled {
            return;
        }
        if let Some(age) = self.git_lock_age() {
            if age < GIT_LOCK_TIMEOUT {
                self.set_state("git-operation");
                return;
            }
            self.set_error(".git/index.lock is older than 10 seconds; treating it as stale");
            self.set_state("degraded");
        }
        let full = std::mem::take(&mut self.full);
        let paths: Vec<String> = std::mem::take(&mut self.pending).into_iter().collect();
        let through = self.events_received;
        let result = if full {
            self.engine.reconcile(ReconcileOptions::default())
        } else {
            self.engine.reconcile_paths(&paths)
        };
        match result {
            Ok(report) => {
                self.watermark = through;
                self.first_pending = None;
                self.after_ingest(&report);
                let degraded = self.shared.lock().expect("shared").health.state == "degraded";
                if !degraded {
                    self.set_state("ready");
                }
            }
            Err(e) => {
                self.set_error(&format!("ingest failed: {e}"));
                self.set_state("degraded");
                // Try again from scratch on the next round.
                self.full = true;
            }
        }
    }

    fn after_ingest(&mut self, report: &ReconcileReport) {
        self.shared.lock().expect("shared").health.last_ingest = Some(rfc_now());
        if report.changed_anything() {
            let note = json!({"kind": "ingest", "watermark": self.watermark, "report": report});
            self.notify(note);
        }
    }

    /// Answer the requests whose sync cookie has been seen, in arrival order.
    fn serve_waiting(&mut self) {
        while let Some((cookie, since)) = self.waiting.first().map(|w| (w.cookie.clone(), w.since))
        {
            let seen = self.seen_cookies.contains(&cookie);
            let timed_out = since.elapsed() >= COOKIE_TIMEOUT;
            if !seen && !timed_out {
                return;
            }
            if timed_out && !seen {
                self.set_error(
                    "the watcher did not report a sync cookie in time; reconciling everything",
                );
                self.mark_full();
            }
            // Everything that happened before the cookie must be applied first (MVP-020).
            if !self.pending.is_empty() || self.full {
                let git_wait = self.git_lock_age().is_some_and(|a| a < GIT_LOCK_TIMEOUT);
                if git_wait && since.elapsed() < GIT_LOCK_TIMEOUT {
                    return;
                }
                if git_wait {
                    self.set_state("degraded");
                }
                // Apply even under a stale or long-held lock rather than wait forever.
                let lock_hold = self.git_lock_age();
                if lock_hold.is_some() {
                    self.apply_now();
                } else {
                    self.maybe_apply(true);
                }
                if !self.pending.is_empty() || self.full {
                    return;
                }
            }
            let w = self.waiting.remove(0);
            self.seen_cookies.remove(&w.cookie);
            let _ = fs::remove_file(self.cookie_dir.join(&w.cookie));
            match w.waiter {
                Waiter::Call(call, reply) => self.serve(call, reply),
                Waiter::Sync(reply) => {
                    let _ = reply.send(Ok(json!({"watermark": self.watermark})));
                }
            }
        }
    }

    /// Apply pending changes now, regardless of `.git/index.lock`.
    fn apply_now(&mut self) {
        let full = std::mem::take(&mut self.full);
        let paths: Vec<String> = std::mem::take(&mut self.pending).into_iter().collect();
        let through = self.events_received;
        let result = if full {
            self.engine.reconcile(ReconcileOptions::default())
        } else {
            self.engine.reconcile_paths(&paths)
        };
        match result {
            Ok(report) => {
                self.watermark = through;
                self.first_pending = None;
                self.after_ingest(&report);
            }
            Err(e) => {
                self.set_error(&format!("ingest failed: {e}"));
                self.full = true;
            }
        }
    }

    fn serve(&mut self, call: Call, reply: Sender<Result<Value>>) {
        self.requests_served += 1;
        let mutation = service::MUTATIONS.contains(&call.method.as_str());
        let method = call.method.clone();
        let result = service::dispatch(&mut self.engine, &call);
        if mutation && result.is_ok() {
            self.notify(json!({"kind": "mutation", "method": method}));
        }
        let _ = reply.send(result);
        self.last_activity = Instant::now();
    }
}
