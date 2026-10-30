//! The system-wide registry: discovery and health of per-repository daemons, nothing else
//! (§8.7, FR-088–FR-094, MVP-022).
//!
//! It is never on a request's path (P5): clients find daemons through `daemon.json` and talk
//! to them directly. Daemons register with a lease and renew it by heartbeat; the registry
//! polls their health and never restarts one. After a restart it rebuilds its view from the
//! known repositories' endpoint files and locks.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::daemon::runtime::{self, DaemonInfo, Health};
use crate::error::{Error, Result};
use crate::mcp::jsonrpc::{self, Incoming};
use crate::ops::hlc;

/// How long a registration lasts without a heartbeat (FR-090).
pub const LEASE: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_secs(10);
const TIMEOUT: Duration = Duration::from_secs(1);

/// One registered daemon (FR-091).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    #[serde(flatten)]
    pub info: DaemonInfo,
    /// Milliseconds since the epoch when the lease runs out.
    pub lease_expires: u64,
    pub last_heartbeat: String,
    /// The most recent health report, if the daemon answered.
    pub health: Option<Health>,
}

fn connect(path: &Path) -> Option<UnixStream> {
    let s = UnixStream::connect(path).ok()?;
    let _ = s.set_read_timeout(Some(TIMEOUT));
    let _ = s.set_write_timeout(Some(TIMEOUT));
    Some(s)
}

/// Send one request and read its response.
pub fn request(socket: &Path, method: &str, params: Value) -> Result<Value> {
    let stream = connect(socket)
        .ok_or_else(|| Error::Daemon(format!("nothing is listening on {}", socket.display())))?;
    let mut writer = stream.try_clone().map_err(|e| Error::io("connecting", e))?;
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    writeln!(writer, "{msg}").map_err(|e| Error::io("writing a request", e))?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|e| Error::io("reading a response", e))?;
    let v: Value = serde_json::from_str(&line)?;
    if let Some(err) = v.get("error") {
        return Err(Error::Daemon(
            err["message"].as_str().unwrap_or("error").to_string(),
        ));
    }
    Ok(v["result"].clone())
}

/// Register or renew a daemon. Failures are ignored: the registry is optional (P5).
/// Register or renew this daemon's lease. Failures are the caller's to ignore (P5).
pub fn heartbeat(dir: &Path, info: &DaemonInfo) -> Result<()> {
    request(
        &runtime::registry_socket(dir),
        "register",
        serde_json::to_value(info).unwrap_or_default(),
    )
    .map(|_| ())
}

pub fn unregister(dir: &Path, info: &DaemonInfo) {
    let _ = request(
        &runtime::registry_socket(dir),
        "unregister",
        json!({"root": info.root, "pid": info.pid}),
    );
}

/// The registry's entries, if it is running.
pub fn list(dir: &Path) -> Result<Vec<Entry>> {
    Ok(serde_json::from_value(request(
        &runtime::registry_socket(dir),
        "list",
        json!({}),
    )?)?)
}

/// Ask a daemon for its health directly (discovery without the registry, S14).
pub fn daemon_health(endpoint: &Path) -> Option<Health> {
    request(endpoint, "health", json!({}))
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
}

type Entries = Arc<Mutex<BTreeMap<String, Entry>>>;

fn lease_from_now() -> u64 {
    hlc::now_ms() + LEASE.as_millis() as u64
}

/// Rebuild the view from known repositories: a daemon is there if its lock is held (FR-092).
fn rebuild() -> BTreeMap<String, Entry> {
    let mut out = BTreeMap::new();
    for root in runtime::known_roots() {
        if !runtime::lock_held(&root) {
            continue;
        }
        if let Some(info) = runtime::read_info(&root) {
            out.insert(
                info.root.clone(),
                Entry {
                    info,
                    lease_expires: lease_from_now(),
                    last_heartbeat: hlc::rfc3339(hlc::now_ms()),
                    health: None,
                },
            );
        }
    }
    out
}

/// Run the registry until asked to shut down (`resonance registry run`).
pub fn run(dir: &Path) -> Result<()> {
    runtime::ensure_dir(dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("registry.lock"))
        .map_err(|e| Error::io("opening the registry lock", e))?;
    if lock.try_lock().is_err() {
        return Err(Error::Daemon("a registry is already running".into()));
    }
    let socket = runtime::registry_socket(dir);
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)
        .map_err(|e| Error::io(format!("listening on {}", socket.display()), e))?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&socket, fs::Permissions::from_mode(0o600));
    }
    let entries: Entries = Arc::new(Mutex::new(rebuild()));
    eprintln!(
        "[{}] resonance registry listening on {} ({} daemons found)",
        hlc::rfc3339(hlc::now_ms()),
        socket.display(),
        entries.lock().expect("entries").len()
    );

    // Health polling and lease expiry (FR-090, FR-094: monitor only).
    {
        let entries = Arc::clone(&entries);
        thread::spawn(move || {
            loop {
                poll(&entries);
                thread::sleep(POLL);
            }
        });
    }
    let stop = Arc::new(Mutex::new(false));
    listener
        .set_nonblocking(true)
        .map_err(|e| Error::io("configuring the registry socket", e))?;
    while !*stop.lock().expect("stop") {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let entries = Arc::clone(&entries);
                let stop = Arc::clone(&stop);
                thread::spawn(move || serve(stream, &entries, &stop));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
    let _ = fs::remove_file(&socket);
    drop(lock);
    Ok(())
}

/// Expire missed leases and refresh each daemon's health.
fn poll(entries: &Entries) {
    let snapshot: Vec<Entry> = entries.lock().expect("entries").values().cloned().collect();
    let now = hlc::now_ms();
    for e in snapshot {
        let health = daemon_health(Path::new(&e.info.endpoint));
        let mut map = entries.lock().expect("entries");
        if e.lease_expires < now && health.is_none() {
            map.remove(&e.info.root);
        } else if let Some(cur) = map.get_mut(&e.info.root) {
            cur.health = health;
        }
    }
}

fn serve(stream: UnixStream, entries: &Entries, stop: &Arc<Mutex<bool>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        let (id, method, params) = match jsonrpc::parse(&line) {
            Ok(Incoming::Request { id, method, params }) => (id, method, params),
            Ok(_) => continue,
            Err(r) => {
                let _ = writeln!(writer, "{r}");
                continue;
            }
        };
        let response = match method.as_str() {
            "register" => match serde_json::from_value::<DaemonInfo>(params) {
                Ok(info) => {
                    runtime::add_known_root(Path::new(&info.root));
                    let mut map = entries.lock().expect("entries");
                    let health = map.get(&info.root).and_then(|e| e.health.clone());
                    map.insert(
                        info.root.clone(),
                        Entry {
                            info,
                            lease_expires: lease_from_now(),
                            last_heartbeat: hlc::rfc3339(hlc::now_ms()),
                            health,
                        },
                    );
                    jsonrpc::result(id, json!({"leaseSeconds": LEASE.as_secs()}))
                }
                Err(e) => jsonrpc::error(id, jsonrpc::INVALID_PARAMS, &e.to_string()),
            },
            "unregister" => {
                let root = params["root"].as_str().unwrap_or_default().to_string();
                let pid = params["pid"].as_u64().unwrap_or(0) as u32;
                let mut map = entries.lock().expect("entries");
                if map.get(&root).is_some_and(|e| e.info.pid == pid) {
                    map.remove(&root);
                }
                jsonrpc::result(id, json!({}))
            }
            "lookup" => {
                // Which endpoint serves this directory? (FR-088)
                let path = PathBuf::from(params["path"].as_str().unwrap_or_default());
                let map = entries.lock().expect("entries");
                let found = map
                    .values()
                    .filter(|e| path.starts_with(&e.info.root))
                    .max_by_key(|e| e.info.root.len())
                    .cloned();
                jsonrpc::result(id, serde_json::to_value(found).unwrap_or_default())
            }
            "list" => {
                let map = entries.lock().expect("entries");
                let list: Vec<&Entry> = map.values().collect();
                jsonrpc::result(id, serde_json::to_value(list).unwrap_or_default())
            }
            "shutdown" => {
                *stop.lock().expect("stop") = true;
                jsonrpc::result(id, json!({"stopping": true}))
            }
            other => jsonrpc::error(
                id,
                jsonrpc::METHOD_NOT_FOUND,
                &format!("method not found: {other}"),
            ),
        };
        let _ = writeln!(writer, "{response}");
        if *stop.lock().expect("stop") {
            return;
        }
    }
}

/// A user-level service definition for the registry, and how to load and unload it (FR-093).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePlan {
    pub path: PathBuf,
    pub contents: String,
    pub load: Vec<Vec<String>>,
    pub unload: Vec<Vec<String>>,
}

fn uid() -> String {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "501".into())
}

/// The launchd agent (macOS) or `systemd --user` unit (Linux) that runs the registry.
pub fn service_plan(exe: &Path, home: &Path) -> Result<ServicePlan> {
    let exe = exe.display().to_string();
    if cfg!(target_os = "macos") {
        let path = home.join("Library/LaunchAgents/dev.resonance.registry.plist");
        let log = home.join(".resonance/registry.log").display().to_string();
        let contents = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>dev.resonance.registry</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>registry</string>
    <string>run</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#
        );
        let target = format!("gui/{}", uid());
        let p = path.display().to_string();
        Ok(ServicePlan {
            load: vec![vec![
                "launchctl".into(),
                "bootstrap".into(),
                target.clone(),
                p.clone(),
            ]],
            unload: vec![vec!["launchctl".into(), "bootout".into(), target, p]],
            path,
            contents,
        })
    } else {
        let path = home.join(".config/systemd/user/resonance-registry.service");
        let contents = format!(
            "[Unit]\nDescription=Resonance registry (discovery and health of per-repository daemons)\n\n[Service]\nExecStart={exe} registry run\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n"
        );
        let sc = |args: &[&str]| -> Vec<String> {
            std::iter::once("systemctl".to_string())
                .chain(std::iter::once("--user".to_string()))
                .chain(args.iter().map(|a| a.to_string()))
                .collect()
        };
        Ok(ServicePlan {
            load: vec![
                sc(&["daemon-reload"]),
                sc(&["enable", "--now", "resonance-registry.service"]),
            ],
            unload: vec![
                sc(&["disable", "--now", "resonance-registry.service"]),
                sc(&["daemon-reload"]),
            ],
            path,
            contents,
        })
    }
}

fn run_all(commands: &[Vec<String>]) -> Result<String> {
    let mut out = String::new();
    for c in commands {
        let o = Command::new(&c[0])
            .args(&c[1..])
            .output()
            .map_err(|e| Error::Invalid(format!("could not run {}: {e}", c[0])))?;
        out.push_str(&String::from_utf8_lossy(&o.stdout));
        out.push_str(&String::from_utf8_lossy(&o.stderr));
        if !o.status.success() {
            return Err(Error::Invalid(format!(
                "`{}` failed: {}",
                c.join(" "),
                out.trim()
            )));
        }
    }
    Ok(out.trim().to_string())
}

/// Write the service definition and load it.
pub fn install(plan: &ServicePlan) -> Result<String> {
    if let Some(dir) = plan.path.parent() {
        fs::create_dir_all(dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
    }
    fs::write(&plan.path, &plan.contents)
        .map_err(|e| Error::io(format!("writing {}", plan.path.display()), e))?;
    run_all(&plan.load)
}

/// Unload the service and remove its definition.
pub fn uninstall(plan: &ServicePlan) -> Result<String> {
    let out = run_all(&plan.unload).unwrap_or_else(|e| e.to_string());
    if plan.path.exists() {
        fs::remove_file(&plan.path)
            .map_err(|e| Error::io(format!("removing {}", plan.path.display()), e))?;
    }
    Ok(out)
}
