//! The per-repository daemon (§8.4, §8.7, MVP-019 to MVP-023).
//!
//! One owner thread holds the [`Engine`] and applies every change, whether it comes from the
//! file watcher or from a request (P1). Connection threads parse JSON-RPC requests from the
//! Unix socket and hand them to the owner. `health` is answered from shared state, so a daemon
//! that is busy reconciling still reports liveness (FR-095).
//!
//! Read-your-writes (FR-102, MVP-020): before answering a request, the owner writes a sync
//! cookie into `.resonance/local/cookies/` and answers only once the watcher has reported it and
//! every earlier change has been applied.
//!
//! The daemon logs through `tracing` (MVP-044) and records what it does as diagnostics events
//! for the web UI, which the binary hosts in the same process through a [`Handle`] (MVP-043).

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
use crate::diag::Diagnostics;
use crate::error::{Error, Result};
use crate::ingest::walk::{compile_ignores, is_ignored_path};
use crate::ingest::{ReconcileOptions, ReconcileReport};
use crate::lifecycle::StoreFactory;
use crate::logging::{self, Logs};
use crate::mcp::jsonrpc::{self, Incoming};
use crate::ops::{Actor, hlc};
use crate::repo::Repo;
use crate::repo::config::Config;
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
/// How often the daemon checks that its socket file is still in place.
const SOCKET_CHECK: Duration = Duration::from_secs(1);
/// JSON-RPC error code for operation failures; `data.exitCode` carries the CLI exit code.
pub const OPERATION_FAILED: i64 = -32000;
/// How often subscribers that may have gone away are probed, once the daemon could be idle.
const SUBSCRIBER_PROBE: Duration = Duration::from_secs(30);

/// Something the daemon process serves beside its socket, such as the web UI (MVP-043). It
/// gets a [`Handle`] and returns what it serves, or `None` when it serves nothing.
pub type Host = Box<dyn FnOnce(Handle) -> Option<Hosted> + Send>;

/// What a [`Host`] serves: its address, and the thread to wait for when the daemon stops.
pub struct Hosted {
    pub url: String,
    pub thread: thread::JoinHandle<()>,
}

/// Options for running a daemon.
pub struct DaemonOptions {
    pub binary_id: String,
    /// Overrides `daemon.idleSeconds`.
    pub idle: Option<Duration>,
    /// Served beside the socket for the daemon's lifetime.
    pub host: Option<Host>,
}

/// A request on its way to the owner.
struct Request {
    call: Call,
    reply: Sender<Result<Value>>,
    received: Instant,
    /// The diagnostics client that sent it.
    client: Option<u64>,
}

enum Job {
    Fs(notify::Result<Event>),
    Call(Request),
    Sync { reply: Sender<Result<Value>> },
    Subscribe(Sender<Value>),
    Shutdown { reply: Sender<Result<Value>> },
}

enum Waiter {
    Call(Request),
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
    /// Start a successor once this daemon has stopped (FR-295).
    restart: bool,
}

/// An in-process client of the daemon, for what the binary hosts beside the socket (MVP-043):
/// the same operations as the socket, plus the diagnostics and the log (MVP-044).
#[derive(Clone)]
pub struct Handle {
    tx: Sender<Job>,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    started: Instant,
    pub diag: Arc<Diagnostics>,
    pub logs: Arc<Logs>,
    pub repo: Repo,
}

impl Handle {
    /// Run one operation, served like a socket request (read-your-writes included).
    pub fn call(&self, call: Call, client: Option<u64>) -> Result<Value> {
        let (reply, rx) = mpsc::channel();
        self.tx
            .send(Job::Call(Request {
                call,
                reply,
                received: Instant::now(),
                client,
            }))
            .map_err(|_| Error::Daemon("the daemon is stopping".into()))?;
        rx.recv()
            .map_err(|_| Error::Daemon("the daemon stopped before answering".into()))?
    }

    /// Receive change notifications (FR-104) on `tx`. An open subscription keeps the daemon
    /// from going idle (FR-084).
    pub fn subscribe(&self, tx: Sender<Value>) -> bool {
        self.tx.send(Job::Subscribe(tx)).is_ok()
    }

    pub fn health(&self) -> Health {
        self.shared.lock().expect("shared").health.clone()
    }

    /// Say in `health` why the web UI is not served (FR-286).
    pub fn set_ui_error(&self, e: &str) {
        self.shared.lock().expect("shared").health.ui_error = Some(e.to_string());
    }

    /// Whether the daemon is stopping, so that hosted servers stop too.
    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Ask the daemon to stop; with `restart`, a successor starts once it has (FR-295).
    pub fn shutdown(&self, restart: bool) -> Result<Value> {
        let (reply, rx) = mpsc::channel();
        if restart {
            self.shared.lock().expect("shared").restart = true;
        }
        self.tx
            .send(Job::Shutdown { reply })
            .map_err(|_| Error::Daemon("the daemon is stopping".into()))?;
        rx.recv()
            .map_err(|_| Error::Daemon("the daemon stopped before answering".into()))?
    }

    /// Identity, health, configuration with sources, events after `since` (of `kinds`),
    /// counters and clients (FR-293). Served from memory; never waits for the watermark.
    pub fn diagnostics(&self, args: &Value) -> Value {
        let since = args.get("since").and_then(Value::as_u64);
        let kinds: Vec<String> = args
            .get("kinds")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|k| k.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let mut v = self.diag.snapshot(since, &kinds);
        let h = self.health();
        v["identity"] = json!({
            "pid": h.pid,
            "version": h.version,
            "binaryId": h.binary_id,
            "schemaVersion": h.schema_version,
            "startedAt": h.started_at,
            "uptimeSeconds": self.started.elapsed().as_secs(),
            "root": h.root,
            "endpoint": h.endpoint,
            "ui": h.ui,
        });
        v["health"] = serde_json::to_value(&h).unwrap_or_default();
        v["config"] = Config::sources(&self.repo.config_path(), |k| std::env::var(k).ok())
            .ok()
            .and_then(|s| serde_json::to_value(s).ok())
            .unwrap_or_else(|| json!([]));
        v["log"] = json!({"level": self.logs.level(), "file": self.logs.file()});
        v
    }

    /// Log records after `since`, at `level` or more severe (FR-298).
    pub fn log_records(&self, args: &Value) -> Value {
        self.logs.read(
            args.get("since").and_then(Value::as_u64),
            args.get("level").and_then(Value::as_str),
        )
    }

    /// Change the log level for the rest of the daemon's life (FR-295, FR-298).
    pub fn set_log_level(&self, level: &str, actor: Option<&str>) -> Result<Value> {
        let before = self.logs.level();
        let now = self.logs.set_level(level)?;
        tracing::info!(from = %before, to = %now, "log level changed");
        self.diag.record(
            "action",
            format!("log level {before} -> {now}"),
            json!({"action": "log_level", "from": before, "to": now, "actor": actor}),
        );
        Ok(json!({"level": now}))
    }
}

/// The lock inodes this daemon holds. On macOS, closing any descriptor of a file releases the
/// process's flock on it, so each inode is opened once and never duplicated.
struct DaemonLock {
    path: PathBuf,
    /// The repository this daemon serves. A socket counts only when `health` names it.
    root: PathBuf,
    files: Mutex<Vec<fs::File>>,
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
    // One daemon per repository (FR-082). The guarantee is the lock inode at `daemon.lock`, or a
    // daemon that still answers on this repository's socket. Deleting either file, or both, does
    // not make a running daemon gone.
    let lock_path = runtime::lock_path(&repo);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|e| Error::io("opening .resonance/local/daemon.lock", e))?;
    if lock.try_lock().is_err() {
        let pid = runtime::read_info(&repo.root)
            .map(|i| i.pid.to_string())
            .unwrap_or_else(|| "unknown".into());
        return Err(Error::AlreadyRunning(pid));
    }

    let dir = runtime::runtime_dir();
    runtime::ensure_dir(&dir)?;
    let endpoint = runtime::socket_path(&dir, &repo.root);
    // Holding the lock is not enough: if `daemon.lock` was deleted under a running daemon, that
    // daemon still holds the old inode. A daemon whose `health` names this repository is
    // running; never take its socket away (FR-082). A listener that does not is not that daemon.
    if runtime::listener_alive(&repo.root) {
        let pid = runtime::read_info(&repo.root)
            .map(|i| i.pid.to_string())
            .unwrap_or_else(|| "unknown".into());
        return Err(Error::AlreadyRunning(pid));
    }
    // The previous daemon may still be inside its owner thread after losing both files. Wait
    // until that process has left the store before binding or opening it ourselves.
    if let Some(prev) = runtime::read_info(&repo.root)
        && runtime::same_root(&prev.root, &repo.root)
        && prev.pid != std::process::id()
        && runtime::process_alive(prev.pid)
        && process_looks_like_daemon(prev.pid)
    {
        tracing::info!("waiting for daemon {} to release the store", prev.pid);
        let exited = wait_until_exited(prev.pid, Duration::from_secs(15));
        if !exited || runtime::listener_alive(&repo.root) {
            return Err(Error::AlreadyRunning(prev.pid.to_string()));
        }
    }
    // Do not bind or unlink unless this process holds the inode now at `daemon.lock`.
    if runtime::fd_id(&lock) != runtime::file_id(&lock_path) {
        let pid = runtime::read_info(&repo.root)
            .map(|i| i.pid.to_string())
            .unwrap_or_else(|| "unknown".into());
        return Err(Error::AlreadyRunning(pid));
    }
    // The log follows the configuration from here on (MVP-044).
    let (config, config_error) = match Config::load(&repo.config_path()) {
        Ok(c) => (c, None),
        Err(e) => (Config::default(), Some(e.to_string())),
    };
    let (logs, log_note) = logging::init(&repo, &config.log);
    if let Some(e) = config_error {
        tracing::warn!("{e}; the daemon's log and web UI use the defaults");
    }
    if let Some(note) = log_note {
        tracing::warn!("{note}");
    }
    let _ = fs::remove_file(&endpoint); // A stale socket: nobody answers on it.
    let listener = bind_socket(&endpoint)?;
    // The path inode, taken while this process holds the lock, so no other daemon can have
    // replaced the socket between bind and this stat. `fstat` of the listener is not that
    // inode: on macOS it does not name the socket file.
    let our_socket = runtime::file_id(&endpoint);

    let mut info = DaemonInfo {
        root: repo.root.display().to_string(),
        endpoint: endpoint.display().to_string(),
        pid: std::process::id(),
        version: VERSION.to_string(),
        binary_id: opts.binary_id.clone(),
        schema_version: SCHEMA_VERSION,
        started_at: rfc_now(),
        ui: None,
    };
    fs::write(
        runtime::info_path(&repo),
        serde_json::to_string_pretty(&info)?,
    )
    .map_err(|e| Error::io("writing .resonance/local/daemon.json", e))?;
    runtime::add_known_root(&repo.root);
    tracing::info!(
        version = VERSION,
        pid = info.pid,
        "resonance daemon started for {} on {}",
        info.root,
        info.endpoint
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
        restart: false,
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel::<Job>();
    let diag = Arc::new(Diagnostics::new(config.diagnostics.buffer_events));
    let handle = Handle {
        tx: tx.clone(),
        shared: Arc::clone(&shared),
        stop: Arc::clone(&stop),
        started: Instant::now(),
        diag: Arc::clone(&diag),
        logs,
        repo: repo.clone(),
    };
    // What the binary serves beside the socket: the web UI (MVP-043).
    let mut opts = opts;
    let hosted = opts.host.take().and_then(|host| host(handle.clone()));
    if let Some(h) = &hosted {
        info.ui = Some(h.url.clone());
        shared.lock().expect("shared").health.ui = Some(h.url.clone());
        let _ = fs::write(
            runtime::info_path(&repo),
            serde_json::to_string_pretty(&info).unwrap_or_default(),
        );
    }

    // One set of lock fds. See [`DaemonLock`].
    let locks = Arc::new(DaemonLock {
        path: lock_path,
        root: repo.root.clone(),
        files: Mutex::new(vec![lock]),
    });
    // Accept connections.
    let accept = {
        let handle = handle.clone();
        let endpoint = endpoint.clone();
        let locks = Arc::clone(&locks);
        thread::spawn(move || accept_loop(listener, &endpoint, our_socket, &locks, handle))
    };
    // Registry heartbeat (P5: failures are ignored), and keeping the lock file in place: if
    // `daemon.lock` is deleted or replaced, lock the file now at that path too, so that the
    // documented lock keeps showing this daemon (FR-082).
    let heartbeat = {
        let stop = Arc::clone(&stop);
        let info = info.clone();
        let dir = dir.clone();
        let locks = Arc::clone(&locks);
        let diag = Arc::clone(&diag);
        thread::spawn(move || {
            // Diagnostics record changes only: registered, or the registry stopped answering.
            let mut registered: Option<bool> = None;
            while !stop.load(Ordering::SeqCst) {
                let result = crate::registry::heartbeat(&dir, &info);
                if registered != Some(result.is_ok()) {
                    match &result {
                        Ok(()) => {
                            diag.record("registry", "registered with the registry", json!({}))
                        }
                        Err(e) => {
                            tracing::debug!("registry: {e}");
                            diag.record(
                                "registry",
                                "the registry is not answering (it is optional)",
                                json!({"error": e.to_string()}),
                            );
                        }
                    }
                    registered = Some(result.is_ok());
                }
                {
                    let mut held = locks.files.lock().expect("locks");
                    keep_lock(&locks.path, &mut held);
                }
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
        Arc::clone(&diag),
        opts,
        tx.clone(),
    )
    .and_then(|owner| owner.run(rx));
    if let Err(e) = &result {
        tracing::error!("daemon stopped with an error: {e}");
    }

    stop.store(true, Ordering::SeqCst);
    // `accept_loop` unlinks the socket, if it is still ours, before it drops the listener.
    let _ = accept.join();
    if let Some(h) = hosted {
        let _ = h.thread.join();
    }
    // Remove our endpoint record only if it still names this process. The lock is still held.
    if runtime::read_info(&repo.root).is_some_and(|i| i.pid == info.pid) {
        let _ = fs::remove_file(runtime::info_path(&repo));
    }
    let _ = heartbeat.join();
    let _ = fs::remove_dir_all(repo.local_dir().join("cookies"));
    let restart = shared.lock().expect("shared").restart;
    tracing::info!("daemon exited");
    drop(locks);
    if restart && result.is_ok() {
        // The successor takes the lock, the socket and the web UI's port (FR-295).
        match std::env::current_exe() {
            Ok(exe) => {
                if let Err(e) = client::start(&repo, &exe) {
                    tracing::error!("could not start the next daemon: {e}");
                }
            }
            Err(e) => tracing::error!("could not find the resonance executable to restart: {e}"),
        }
    }
    result
}

/// Wait until `pid` is no longer running.
fn wait_until_exited(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while runtime::process_alive(pid) {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    true
}

/// Whether `pid` is a Resonance process. A recycled pid must not block startup.
fn process_looks_like_daemon(pid: u32) -> bool {
    let Ok(out) = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return true;
    };
    if !out.status.success() {
        return true;
    }
    String::from_utf8_lossy(&out.stdout).contains("resonance")
}

/// If the lock file at `path` is not one this daemon holds (it was deleted or replaced), create
/// or open it and lock it as well. Failures are left for the next heartbeat.
fn keep_lock(path: &std::path::Path, locks: &mut Vec<fs::File>) {
    use std::os::unix::fs::MetadataExt;
    let current = runtime::file_id(path);
    let held = locks.iter().any(|f| {
        f.metadata()
            .ok()
            .is_some_and(|m| Some((m.dev(), m.ino())) == current)
    });
    if held {
        return;
    }
    if let Ok(f) = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        && f.try_lock().is_ok()
    {
        tracing::warn!(
            "{} was deleted or replaced; locked it again",
            path.display()
        );
        locks.push(f);
    }
}

/// Listen on `endpoint`, readable only by this user, without blocking.
fn bind_socket(endpoint: &Path) -> Result<UnixListener> {
    let listener = UnixListener::bind(endpoint)
        .map_err(|e| Error::io(format!("listening on {}", endpoint.display()), e))?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(endpoint, fs::Permissions::from_mode(0o600));
    }
    listener
        .set_nonblocking(true)
        .map_err(|e| Error::io("configuring the socket", e))?;
    Ok(listener)
}

/// What to do about the socket file at `endpoint`.
enum SocketCheck {
    /// It is ours, or it could not be repaired this time.
    Fine,
    /// It was deleted (or replaced by a file nobody listens on) and is bound again.
    Rebound(UnixListener, Option<(u64, u64)>),
    /// Another daemon answers there: this one must not compete with it (FR-082).
    Taken,
}

/// Clients find the daemon only through its socket file, so if that file is deleted while the
/// daemon runs (or the runtime directory is cleaned), bind it again rather than leave clients
/// unable to connect until the daemon goes idle.
///
/// Rebinding unlinks and binds. That happens only while this process holds the inode currently
/// at `daemon.lock`. Otherwise another daemon owns the repository, and this one must not take
/// its socket or remove it (FR-082).
fn keep_socket(endpoint: &Path, ours: Option<(u64, u64)>, locks: &DaemonLock) -> SocketCheck {
    let current = runtime::file_id(endpoint);
    if current.is_some() && current == ours {
        return SocketCheck::Fine;
    }
    {
        let mut held = locks.files.lock().expect("locks");
        if !holds_lock(&locks.path, &held) && !relock(&locks.path, &mut held) {
            // No lock on the path. Stand down whether or not a listener is there yet: the
            // process that holds the new inode is about to bind, or already has.
            return SocketCheck::Taken;
        }
    }
    // We hold the lock. Stand down only for a daemon that reports this repository.
    // Anything else is not ours: unlink it and bind again.
    if current.is_some() && runtime::serves_repo(endpoint, &locks.root) {
        return SocketCheck::Taken;
    }
    if let Some(dir) = endpoint.parent() {
        let _ = runtime::ensure_dir(dir);
    }
    let _ = fs::remove_file(endpoint); // Nobody answers on whatever is there.
    match bind_socket(endpoint) {
        Ok(listener) => {
            // The flock is still held, so this stat is the socket just bound.
            let id = runtime::file_id(endpoint);
            tracing::warn!(
                "{} was deleted or replaced; listening on it again",
                endpoint.display()
            );
            SocketCheck::Rebound(listener, id)
        }
        Err(_) => SocketCheck::Fine, // Try again at the next check.
    }
}

/// Whether one of `locks` is the inode currently at `path`.
fn holds_lock(path: &Path, locks: &[fs::File]) -> bool {
    let Some(current) = runtime::file_id(path) else {
        return false;
    };
    locks.iter().any(|f| runtime::fd_id(f) == Some(current))
}

/// Open and lock `path` when it is not one of `locks`. Returns whether this process then holds
/// the inode at `path`.
fn relock(path: &Path, locks: &mut Vec<fs::File>) -> bool {
    keep_lock(path, locks);
    holds_lock(path, locks)
}

/// Accept connections until stopped, keeping the socket file in place.
fn accept_loop(
    mut listener: UnixListener,
    endpoint: &Path,
    mut ours: Option<(u64, u64)>,
    locks: &DaemonLock,
    handle: Handle,
) {
    let mut checked = Instant::now();
    while !handle.stopping() {
        if checked.elapsed() >= SOCKET_CHECK {
            checked = Instant::now();
            {
                let mut held = locks.files.lock().expect("locks");
                keep_lock(&locks.path, &mut held);
            }
            match keep_socket(endpoint, ours, locks) {
                SocketCheck::Fine => {}
                SocketCheck::Rebound(l, id) => {
                    listener = l;
                    ours = id;
                }
                SocketCheck::Taken => {
                    tracing::warn!(
                        "this daemon no longer holds {} or {}; it stops",
                        locks.path.display(),
                        endpoint.display()
                    );
                    let (reply, _) = mpsc::channel();
                    let _ = handle.tx.send(Job::Shutdown { reply });
                    // The successor owns the path. Forgetting ours keeps exit from unlinking it.
                    ours = None;
                    break;
                }
            }
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let handle = handle.clone();
                thread::spawn(move || {
                    handle.shared.lock().expect("shared").health.connections += 1;
                    let mut client = None;
                    let _ = serve_connection(stream, &handle, &mut client);
                    if let Some(id) = client {
                        handle.diag.disconnected(id);
                    }
                    handle.shared.lock().expect("shared").health.connections -= 1;
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
    // Unlink before dropping the listener, and only while this process holds the lock inode
    // now at `daemon.lock`. Opening a Unix socket path fails (ENOTSUP on macOS), so the check
    // is the path inode recorded at bind. The listener stays open across that check: dropping
    // it first would let a successor bind into the gap and then be unlinked.
    unlink_bound_socket(&listener, endpoint, ours, locks);
}

/// Remove `endpoint` only when it is still the socket this daemon bound and this daemon holds
/// the lock. `listener` stays in scope so that socket is open until the check finishes.
fn unlink_bound_socket(
    listener: &UnixListener,
    endpoint: &Path,
    ours: Option<(u64, u64)>,
    locks: &DaemonLock,
) {
    let _open = listener;
    let Some(ours) = ours else {
        return;
    };
    let held = locks.files.lock().expect("locks");
    if holds_lock(&locks.path, &held) && runtime::file_id(endpoint) == Some(ours) {
        let _ = fs::remove_file(endpoint);
    }
}

fn write_line(stream: &mut UnixStream, v: &Value) -> std::io::Result<()> {
    let mut text = serde_json::to_string(v).unwrap_or_default();
    text.push('\n');
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

/// Serve one socket connection. `client` is its diagnostics ID, assigned at its first request
/// other than `health`, so that health probes do not show as clients.
fn serve_connection(
    stream: UnixStream,
    handle: &Handle,
    client: &mut Option<u64>,
) -> std::io::Result<()> {
    let tx = &handle.tx;
    let shared = &handle.shared;
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
        if method != "health" && client.is_none() {
            *client = Some(handle.diag.connected("socket"));
        }
        let response = match method.as_str() {
            "health" => {
                let h = shared.lock().expect("shared").health.clone();
                jsonrpc::result(id, serde_json::to_value(h).unwrap_or_default())
            }
            "diagnostics" => jsonrpc::result(id, handle.diagnostics(&params)),
            "logs" => jsonrpc::result(id, handle.log_records(&params)),
            "log_level" => {
                let level = params["level"].as_str().unwrap_or_default();
                match handle.set_log_level(level, params["actor"].as_str()) {
                    Ok(v) => jsonrpc::result(id, v),
                    Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": error_value(&e)}),
                }
            }
            "call" => match serde_json::from_value::<Call>(params) {
                Ok(call) => {
                    let (rtx, rrx) = mpsc::channel();
                    let req = Request {
                        call,
                        reply: rtx,
                        received: Instant::now(),
                        client: *client,
                    };
                    if tx.send(Job::Call(req)).is_err() {
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
    diag: Arc<Diagnostics>,
    watch: Option<watch::Watch>,
    pending: BTreeSet<String>,
    full: bool,
    /// Git or the delta history changed: catch up before anything else (MVP-029).
    git_dirty: bool,
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
    /// When subscribers were last probed for having gone away.
    last_probe: Instant,
}

impl Owner {
    #[allow(clippy::too_many_arguments)]
    fn new(
        repo: &Repo,
        open_store: StoreFactory<'_>,
        actor: Actor,
        shared: Arc<Mutex<Shared>>,
        diag: Arc<Diagnostics>,
        opts: DaemonOptions,
        tx: Sender<Job>,
    ) -> Result<Owner> {
        let store = open_store(&repo.store_path())?;
        let engine = Engine::open(repo.clone(), store, actor, repo.root.clone())?;
        let git_dir = repo
            .git(&["rev-parse", "--absolute-git-dir"])
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo.root.join(".git"));
        // Watched directories must exist (inotify cannot watch a directory before it exists).
        let _ = fs::create_dir_all(repo.deltas_dir());
        let cookie_dir = repo.local_dir().join("cookies");
        let _ = fs::remove_dir_all(&cookie_dir);
        fs::create_dir_all(&cookie_dir)
            .map_err(|e| Error::io("creating .resonance/local/cookies", e))?;
        let idle = opts.idle.unwrap_or(Duration::from_secs(
            engine.config.daemon.idle_seconds.max(1),
        ));
        let mut owner = Owner {
            root: repo.root.clone(),
            git_dir,
            cookie_dir,
            shared,
            diag,
            watch: None,
            pending: BTreeSet::new(),
            full: false,
            git_dirty: false,
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
            last_probe: Instant::now(),
            engine,
        };
        // Watch before the startup reconcile, so that nothing changed during it is missed.
        let extra = vec![
            owner.cookie_dir.clone(),
            owner.git_dir.clone(),
            owner.git_dir.join("info"),
            owner.git_dir.join("logs"),
            owner.git_dir.join("refs").join("heads"),
            repo.resonance_dir(),
            repo.deltas_dir(),
            repo.private_dir(),
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
        let before = {
            let mut s = self.shared.lock().expect("shared");
            let before = std::mem::replace(&mut s.health.state, state.to_string());
            s.health.ready = matches!(state, "ready" | "degraded");
            before
        };
        if before != state {
            tracing::debug!(from = %before, to = state, "state changed");
            self.diag.record(
                "state",
                format!("{before} -> {state}"),
                json!({"from": before, "to": state}),
            );
        }
    }

    fn set_error(&self, e: &str) {
        tracing::warn!("{e}");
        self.shared.lock().expect("shared").health.last_error = Some(e.to_string());
    }

    fn publish(&mut self) {
        if self.pending.is_empty() && !self.full && !self.git_dirty {
            // Nothing outstanding: every event received so far is accounted for.
            self.watermark = self.events_received;
        } else if self.git_lock_age().is_some_and(|a| a < GIT_LOCK_TIMEOUT) {
            // Changes are being held back until the git command finishes (MVP-023).
            self.set_state("git-operation");
        }
        let mut s = self.shared.lock().expect("shared");
        s.health.ingest_watermark = self.watermark;
        s.health.events_received = self.events_received;
        s.health.pending_events =
            self.pending.len() + usize::from(self.full) + usize::from(self.git_dirty);
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
        // Changes found now were made while no daemon was running (C3, MVP-031).
        self.engine.set_startup(true);
        let report = self.engine.reconcile(ReconcileOptions::default())?;
        self.after_ingest(&report, "startup", 0);
        self.set_state(if self.watch.is_some() {
            "ready"
        } else {
            "degraded"
        });
        self.publish();

        loop {
            let busy =
                !self.pending.is_empty() || self.full || self.git_dirty || !self.waiting.is_empty();
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
            if !self
                .root
                .join(".resonance")
                .join("local")
                .join("state.db")
                .exists()
            {
                // `.resonance/` was removed (for example by `git clean -fdx`): nothing to serve.
                tracing::warn!(".resonance/ disappeared; exiting");
                for w in self.waiting.drain(..) {
                    let err = Err(Error::NotInitialized);
                    match w.waiter {
                        Waiter::Call(Request { reply, .. }) | Waiter::Sync(reply) => {
                            let _ = reply.send(err);
                        }
                    }
                }
                break;
            }
            // A subscriber that went away is noticed only when something is sent to it. Probe
            // now and then once the daemon could be idle, so that a closed page or TUI does
            // not keep it running.
            if !self.subscribers.is_empty()
                && self.last_activity.elapsed() >= self.idle
                && self.last_probe.elapsed() >= SUBSCRIBER_PROBE.min(self.idle)
            {
                self.last_probe = Instant::now();
                let ping = json!({"jsonrpc": "2.0", "method": "ping"});
                self.subscribers.retain(|s| s.send(ping.clone()).is_ok());
            }
            let quiet = self.waiting.is_empty()
                && self.subscribers.is_empty()
                && self.shared.lock().expect("shared").health.connections == 0;
            if quiet && self.last_activity.elapsed() >= self.idle {
                tracing::info!("idle for {}s; exiting", self.idle.as_secs());
                break;
            }
        }
        Ok(())
    }

    fn handle(&mut self, job: Job) {
        match job {
            Job::Fs(ev) => self.on_event(ev),
            Job::Call(req) => {
                self.last_activity = Instant::now();
                if req.call.cached || self.watch.is_none() {
                    if self.watch.is_none() {
                        // No watcher: reconcile on demand, as in direct mode (P4).
                        self.full = true;
                        self.maybe_apply(true);
                    }
                    self.serve(req);
                } else {
                    let cookie = self.new_cookie();
                    self.waiting.push(Waiting {
                        cookie,
                        since: Instant::now(),
                        waiter: Waiter::Call(req),
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

    fn mark_git(&mut self) {
        self.git_dirty = true;
        self.first_pending.get_or_insert_with(Instant::now);
        self.last_event = Some(Instant::now());
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
            // The exclude file changes which files are ingested.
            if rel == Path::new("info/exclude") {
                self.mark_full();
                return;
            }
            // HEAD, branches and the reflog: a git operation moved something (MVP-029).
            let r = rel.to_string_lossy();
            let moves = matches!(
                r.as_ref(),
                "HEAD" | "ORIG_HEAD" | "packed-refs" | "logs/HEAD" | "MERGE_HEAD"
            ) || r.starts_with("refs/heads/")
                || r.starts_with("logs/refs/heads/");
            if moves {
                self.mark_git();
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
        let history_file = rel == ".resonance/graph.db"
            || (rel.starts_with(".resonance/deltas/")
                || rel.starts_with(".resonance/local/private/"))
                && rel.ends_with(".delta");
        if history_file {
            // The delta history changed (a git operation, or by hand): catch up, unless it is
            // the open delta exactly as the daemon wrote it (FR-044).
            if !self.engine.is_own_write(path) {
                self.mark_git();
            }
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
        if self.pending.is_empty() && !self.full && !self.git_dirty {
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
        let trigger = self.trigger(now);
        let full = std::mem::take(&mut self.full);
        self.git_dirty = false;
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
                self.after_ingest(&report, trigger, paths.len());
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

    /// What caused the changes about to be applied, for diagnostics.
    fn trigger(&self, request: bool) -> &'static str {
        if self.git_dirty {
            "git"
        } else if self.full {
            "full"
        } else if request {
            "request"
        } else {
            "watcher"
        }
    }

    fn after_ingest(&mut self, report: &ReconcileReport, trigger: &str, paths: usize) {
        self.shared.lock().expect("shared").health.last_ingest = Some(rfc_now());
        self.record_ingest(report, trigger, paths);
        if report.changed_anything() {
            let note = json!({"kind": "ingest", "watermark": self.watermark, "report": report});
            self.notify(note);
        }
    }

    /// Diagnostics for an applied batch of changes (MVP-044): the reconcile, the git operation
    /// caught up, a sealed delta and classified divergences. Counts and paths, never content.
    fn record_ingest(&self, report: &ReconcileReport, trigger: &str, paths: usize) {
        let files = report.files_added + report.files_changed + report.files_removed;
        tracing::debug!(
            trigger,
            paths,
            files,
            operations = report.operations,
            duration_ms = report.duration_ms,
            "reconciled"
        );
        self.diag.record(
            "reconcile",
            format!(
                "{trigger}: {files} file(s) changed, {} operation(s) in {} ms",
                report.operations, report.duration_ms
            ),
            json!({
                "trigger": trigger,
                "paths": paths,
                "filesScanned": report.files_scanned,
                "filesAdded": report.files_added,
                "filesChanged": report.files_changed,
                "filesRemoved": report.files_removed,
                "filesMoved": report.files_moved,
                "anchors": report.anchors,
                "operations": report.operations,
                "durationMs": report.duration_ms,
            }),
        );
        if let Some(history) = &report.history {
            if let Some(mv) = &history.head_move {
                tracing::info!(operation = %mv.operation, "caught up with git");
                self.diag.record(
                    "git",
                    format!("caught up: {}", mv.operation),
                    json!({
                        "operation": mv.operation,
                        "from": mv.from,
                        "to": mv.to,
                        "reassociated": mv.reassociated,
                        "detached": mv.detached,
                        "refolded": history.refolded,
                        "held": history.held,
                        "arrived": history.arrived.len(),
                    }),
                );
                if let Some(delta) = &mv.sealed {
                    self.diag.record(
                        "delta",
                        format!("sealed {delta}"),
                        json!({"change": "sealed", "delta": delta}),
                    );
                }
            } else if history.refolded {
                self.diag.record(
                    "git",
                    "re-folded the delta history",
                    json!({"refolded": true, "held": history.held, "operations": history.operations}),
                );
            }
        }
        for d in &report.divergences {
            self.diag.record(
                "divergence",
                format!("{:?} ({:?}): {}", d.class, d.level, d.resolution),
                json!({
                    "id": d.id,
                    "class": d.class,
                    "level": d.level,
                    "resolution": d.resolution,
                    "paths": d.paths.iter().take(10).collect::<Vec<_>>(),
                    "pathCount": d.paths.len(),
                }),
            );
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
            if !self.pending.is_empty() || self.full || self.git_dirty {
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
                if !self.pending.is_empty() || self.full || self.git_dirty {
                    return;
                }
            }
            let w = self.waiting.remove(0);
            self.seen_cookies.remove(&w.cookie);
            let _ = fs::remove_file(self.cookie_dir.join(&w.cookie));
            match w.waiter {
                Waiter::Call(req) => self.serve(req),
                Waiter::Sync(reply) => {
                    let _ = reply.send(Ok(json!({"watermark": self.watermark})));
                }
            }
        }
    }

    /// Apply pending changes now, regardless of `.git/index.lock`.
    fn apply_now(&mut self) {
        let trigger = self.trigger(true);
        let full = std::mem::take(&mut self.full);
        self.git_dirty = false;
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
                self.after_ingest(&report, trigger, paths.len());
            }
            Err(e) => {
                self.set_error(&format!("ingest failed: {e}"));
                self.full = true;
            }
        }
    }

    fn serve(&mut self, req: Request) {
        let Request {
            call,
            reply,
            received,
            client,
        } = req;
        self.requests_served += 1;
        let waited = received.elapsed();
        let started = Instant::now();
        let mutation = service::MUTATIONS.contains(&call.method.as_str());
        let method = call.method.clone();
        let result = service::dispatch(&mut self.engine, &call);
        let actor = call.actor.as_ref().map(|a| a.to_string());
        let error = result.as_ref().err().map(|e| e.to_string());
        self.diag.request(
            &method,
            actor.as_deref(),
            call.origin.as_deref(),
            client,
            waited.as_millis() as u64,
            started.elapsed().as_millis() as u64,
            error.as_deref(),
        );
        if let Some(e) = &error {
            tracing::debug!(method = %method, "request failed: {e}");
        }
        if mutation && result.is_ok() {
            self.record_change(&call, actor.as_deref(), result.as_ref().ok());
            self.notify(json!({"kind": "mutation", "method": method}));
        } else if method == "changes_done"
            && let Ok(v) = &result
        {
            let n = call.args["seqs"].as_array().map_or(0, Vec::len);
            self.diag.record(
                "changelog",
                format!("{n} entr{} cleared", if n == 1 { "y" } else { "ies" }),
                json!({"change": "cleared", "seqs": call.args["seqs"], "actor": actor, "origin": call.origin, "result": v.get("removed")}),
            );
            self.notify(json!({"kind": "mutation", "method": method}));
        }
        let _ = reply.send(result);
        self.last_activity = Instant::now();
    }

    /// Diagnostics for a change made through the API: the change set, and a compaction.
    fn record_change(&self, call: &Call, actor: Option<&str>, result: Option<&Value>) {
        let method = call.method.as_str();
        let target = call
            .args
            .get("ref")
            .or_else(|| call.args.get("key"))
            .and_then(Value::as_str)
            .or_else(|| result.and_then(|v| v["key"].as_str()));
        self.diag.record(
            "changeset",
            match target {
                Some(t) => format!("{method} {t} by {}", actor.unwrap_or("?")),
                None => format!("{method} by {}", actor.unwrap_or("?")),
            },
            json!({"method": method, "target": target, "actor": actor, "origin": call.origin}),
        );
        if method == "compact" {
            self.diag.record(
                "delta",
                "compacted",
                json!({"change": "compacted", "actor": actor}),
            );
        }
    }
}
