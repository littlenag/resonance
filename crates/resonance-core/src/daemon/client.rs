//! Clients of the daemon: find it, start it on demand, and send it calls (FR-083, FR-084,
//! FR-252).
//!
//! A client finds the daemon through `.resonance/local/daemon.json` (no registry needed, S14), or
//! at the socket path computed from the repository root. If none answers, it starts
//! `resonance daemon --foreground` as a detached process and waits for it (P4). A daemon from
//! a different build of `resonance` is asked to stop and replaced, so that an upgraded binary
//! never talks to an old daemon.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::runtime::{self, Health};
use crate::error::{Error, Result};
use crate::ops::Actor;
use crate::repo::Repo;
use crate::service::{Backend, Call};

/// How long a client waits for a daemon it started to accept connections.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long `stop` waits for a daemon to release its lock.
const STOP_TIMEOUT: Duration = Duration::from_secs(15);
/// The daemon log is started afresh when it grows past this.
const LOG_LIMIT: u64 = 1 << 20;

/// A connection to a repository's daemon.
pub struct Remote {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
    /// Who is asking, for provenance (FR-134).
    pub actor: Actor,
    /// Where relative paths are resolved from.
    pub cwd: PathBuf,
    /// Answer without waiting for pending file changes (FR-102).
    pub cached: bool,
}

/// The endpoints a daemon for `repo` may be listening on, most likely first.
pub fn endpoints(repo: &Repo) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(info) = runtime::read_info(&repo.root) {
        out.push(PathBuf::from(info.endpoint));
    }
    let computed = runtime::socket_path(&runtime::runtime_dir(), &repo.root);
    if !out.contains(&computed) {
        out.push(computed);
    }
    out
}

fn try_connect(repo: &Repo) -> Option<UnixStream> {
    endpoints(repo)
        .into_iter()
        .find_map(|p| UnixStream::connect(p).ok())
}

impl Remote {
    fn from_stream(stream: UnixStream, actor: Actor, cwd: PathBuf) -> Result<Remote> {
        let writer = stream
            .try_clone()
            .map_err(|e| Error::io("connecting to the daemon", e))?;
        Ok(Remote {
            reader: BufReader::new(stream),
            writer,
            next_id: 1,
            actor,
            cwd,
            cached: false,
        })
    }

    /// Connect to a running daemon, without starting one.
    pub fn connect_existing(repo: &Repo, actor: Actor, cwd: PathBuf) -> Option<Remote> {
        try_connect(repo).and_then(|s| Remote::from_stream(s, actor, cwd).ok())
    }

    /// Connect, starting a daemon with `exe` if none is running or the running one comes from
    /// a different build (FR-084).
    pub fn connect(repo: &Repo, exe: &Path, actor: Actor, cwd: PathBuf) -> Result<Remote> {
        if !repo.is_initialized() {
            return Err(Error::NotInitialized);
        }
        let ours = runtime::binary_id(exe);
        if let Some(mut remote) = Remote::connect_existing(repo, actor.clone(), cwd.clone()) {
            match remote.health() {
                Ok(h) if h.binary_id == ours => return Ok(remote),
                _ => {
                    // An older (or newer) build: replace it.
                    let _ = remote.request("shutdown", json!({}));
                    drop(remote);
                    wait_for_lock_release(repo, STOP_TIMEOUT);
                }
            }
        }
        let mut child = start(repo, exe)?;
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            if let Some(stream) = try_connect(repo) {
                let mut remote = Remote::from_stream(stream, actor.clone(), cwd.clone())?;
                if remote.health().is_ok() {
                    return Ok(remote);
                }
            }
            if let Ok(Some(status)) = child.try_wait() {
                // It may have lost a race with another daemon; that one will answer.
                if !status.success() && !runtime::lock_held(&repo.root) {
                    return Err(Error::Daemon(format!(
                        "the daemon exited ({status}) before it was ready; see .resonance/daemon.log:\n{}",
                        log_tail(repo)
                    )));
                }
            }
            if Instant::now() >= deadline {
                return Err(Error::Daemon(format!(
                    "the daemon did not start within {}s; see .resonance/daemon.log, or run with --no-daemon",
                    START_TIMEOUT.as_secs()
                )));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Send one JSON-RPC request and wait for its response, skipping notifications.
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.writer, "{msg}")
            .map_err(|e| Error::Daemon(format!("lost the connection: {e}")))?;
        self.writer
            .flush()
            .map_err(|e| Error::Daemon(format!("lost the connection: {e}")))?;
        loop {
            let mut line = String::new();
            let n = self
                .reader
                .read_line(&mut line)
                .map_err(|e| Error::Daemon(format!("lost the connection: {e}")))?;
            if n == 0 {
                return Err(Error::Daemon(
                    "the daemon closed the connection before answering (see .resonance/daemon.log)"
                        .into(),
                ));
            }
            let v: Value = serde_json::from_str(&line)?;
            if v.get("id") != Some(&json!(id)) {
                continue; // A notification.
            }
            if let Some(err) = v.get("error") {
                let message = err["message"]
                    .as_str()
                    .unwrap_or("the daemon reported an error")
                    .to_string();
                let code = err["data"]["exitCode"].as_i64().unwrap_or(1) as i32;
                return Err(Error::Remote { message, code });
            }
            return Ok(v["result"].clone());
        }
    }

    pub fn health(&mut self) -> Result<Health> {
        Ok(serde_json::from_value(self.request("health", json!({}))?)?)
    }

    /// Subscribe to change notifications (FR-104). Each is passed to `on_change` until it
    /// returns false or the daemon goes away.
    pub fn subscribe(mut self, mut on_change: impl FnMut(Value) -> bool) -> Result<()> {
        self.request("subscribe", json!({}))?;
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line).unwrap_or(0) == 0 {
                return Ok(());
            }
            let v: Value = serde_json::from_str(&line)?;
            if v.get("method").and_then(Value::as_str) == Some("changed")
                && !on_change(v["params"].clone())
            {
                return Ok(());
            }
        }
    }
}

impl Backend for Remote {
    fn call(&mut self, mut call: Call) -> Result<Value> {
        if call.actor.is_none() {
            call.actor = Some(self.actor.clone());
        }
        if call.cwd.is_none() {
            call.cwd = Some(self.cwd.clone());
        }
        call.cached |= self.cached;
        if call.env.is_empty() {
            call.env = crate::service::resonance_env();
        }
        self.request("call", serde_json::to_value(&call)?)
    }
}

fn log_tail(repo: &Repo) -> String {
    let text = fs::read_to_string(runtime::log_path(repo)).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(10)..].join("\n")
}

/// Start `exe daemon --foreground` for `repo`, detached, logging to `.resonance/daemon.log`.
pub fn start(repo: &Repo, exe: &Path) -> Result<Child> {
    let log = runtime::log_path(repo);
    if fs::metadata(&log).is_ok_and(|m| m.len() > LOG_LIMIT) {
        let _ = fs::remove_file(&log);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map_err(|e| Error::io("opening .resonance/daemon.log", e))?;
    let mut cmd = Command::new(exe);
    cmd.arg("-C")
        .arg(&repo.root)
        .args(["daemon", "--foreground"])
        .current_dir(&repo.root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(file))
        .env_remove("RESONANCE_NO_DAEMON");
    {
        // Its own process group: the daemon outlives the command that started it.
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
        .map_err(|e| Error::Daemon(format!("could not start the daemon: {e}")))
}

fn wait_for_lock_release(repo: &Repo, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while runtime::lock_held(&repo.root) {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    true
}

/// Ask the daemon for `repo` to stop, and wait until it has. Returns whether one was running.
pub fn stop(repo: &Repo) -> Result<bool> {
    let Some(mut remote) = Remote::connect_existing(repo, Actor::resonance(), repo.root.clone())
    else {
        return Ok(false);
    };
    let _ = remote.request("shutdown", json!({}));
    drop(remote);
    if !wait_for_lock_release(repo, STOP_TIMEOUT) {
        return Err(Error::Daemon("the daemon did not stop in time".into()));
    }
    Ok(true)
}
