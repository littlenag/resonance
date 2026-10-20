//! Where daemons and the registry live: the per-user runtime directory, socket paths,
//! `daemon.json`, the daemon lock and the list of known repositories (MVP-019, MVP-022).

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::content_hash;
use crate::repo::Repo;

/// Socket paths must stay under the platform limit (104 bytes on macOS, 108 on Linux).
const MAX_SOCKET_PATH: usize = 100;

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// The per-user runtime directory (MVP-019).
pub fn runtime_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("RESONANCE_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    let preferred = if let Some(x) = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(PathBuf::from(x).join("resonance"))
    } else {
        home().map(|h| h.join(".resonance").join("run"))
    };
    match preferred {
        // Room for "/<16 hex>.sock".
        Some(p) if p.as_os_str().len() + 22 <= MAX_SOCKET_PATH => p,
        _ => {
            let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
            PathBuf::from(format!("/tmp/resonance-{user}"))
        }
    }
}

/// Create the runtime directory, readable only by this user.
pub fn ensure_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if !dir.exists() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
    }
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    Ok(())
}

/// The socket of the daemon for `root`: a hash of the canonical root keeps it short.
pub fn socket_path(dir: &Path, root: &Path) -> PathBuf {
    let hash = content_hash(root.as_os_str().as_encoded_bytes());
    dir.join(format!("{}.sock", &hash[..16]))
}

pub fn registry_socket(dir: &Path) -> PathBuf {
    dir.join("registry.sock")
}

/// `~/.resonance/known-roots`: repositories that have run a daemon (MVP-022).
pub fn known_roots_file() -> Option<PathBuf> {
    home().map(|h| h.join(".resonance").join("known-roots"))
}

pub fn known_roots() -> Vec<PathBuf> {
    known_roots_file()
        .and_then(|f| fs::read_to_string(f).ok())
        .map(|s| {
            s.lines()
                .filter(|l| !l.trim().is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Remember a repository root (deduplicated).
pub fn add_known_root(root: &Path) {
    let Some(file) = known_roots_file() else {
        return;
    };
    if known_roots().iter().any(|r| r == root) {
        return;
    }
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&file) {
        let _ = writeln!(f, "{}", root.display());
    }
}

/// `.resonance/local/daemon.json` (FR-083).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonInfo {
    pub root: String,
    pub endpoint: String,
    pub pid: u32,
    pub version: String,
    /// Identifies the exact build, so that clients can replace a daemon from an older build.
    pub binary_id: String,
    pub schema_version: u32,
    pub started_at: String,
}

pub fn info_path(repo: &Repo) -> PathBuf {
    repo.local_dir().join("daemon.json")
}

pub fn lock_path(repo: &Repo) -> PathBuf {
    repo.local_dir().join("daemon.lock")
}

pub fn log_path(repo: &Repo) -> PathBuf {
    repo.local_dir().join("daemon.log")
}

pub fn read_info(root: &Path) -> Option<DaemonInfo> {
    let text =
        fs::read_to_string(root.join(".resonance").join("local").join("daemon.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Whether a daemon is running for the repository at `root` (FR-082): some process holds the
/// daemon lock, or a daemon for this repository answers `health` on its socket.
///
/// `flock` locks an inode, not a path: if `daemon.lock` is deleted (or replaced) while a daemon
/// runs, the daemon still holds the old inode and the path no longer shows it. A listener that
/// reports this repository is therefore also proof of a running daemon. A process that merely
/// accepts connections is not.
pub fn lock_held(root: &Path) -> bool {
    lock_file_held(&root.join(".resonance").join("local").join("daemon.lock"))
        || listener_alive(root)
}

/// How long a health probe waits for an answer. A foreign listener must not stall startup.
const HEALTH_PROBE: Duration = Duration::from_millis(500);

/// Whether `reported` and `root` are the same repository.
pub fn same_root(reported: &str, root: &Path) -> bool {
    let reported_path = Path::new(reported);
    if reported_path == root {
        return true;
    }
    match (fs::canonicalize(reported_path), fs::canonicalize(root)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Whether `endpoint` is a Resonance daemon for `root`: `health` returns that root.
pub fn serves_repo(endpoint: &Path, root: &Path) -> bool {
    probe_health(endpoint).is_some_and(|h| same_root(&h.root, root))
}

/// Ask `endpoint` for `health`. `None` when nothing answers as a Resonance daemon.
fn probe_health(endpoint: &Path) -> Option<Health> {
    let mut stream = UnixStream::connect(endpoint).ok()?;
    stream.set_read_timeout(Some(HEALTH_PROBE)).ok()?;
    stream.set_write_timeout(Some(HEALTH_PROBE)).ok()?;
    let msg = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "health", "params": {}});
    writeln!(stream, "{msg}").ok()?;
    stream.flush().ok()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok()?;
    let v: serde_json::Value = serde_json::from_str(&line).ok()?;
    serde_json::from_value(v.get("result")?.clone()).ok()
}

/// Whether a daemon for `root` answers on one of its sockets.
pub fn listener_alive(root: &Path) -> bool {
    let mut paths = Vec::new();
    if let Some(info) = read_info(root) {
        paths.push(PathBuf::from(info.endpoint));
    }
    let computed = socket_path(&runtime_dir(), root);
    if !paths.iter().any(|p| p == &computed) {
        paths.push(computed);
    }
    paths.iter().any(|p| serves_repo(p, root))
}

/// The device and inode of a path, to tell one file from its replacement.
pub fn file_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

/// The device and inode of an open file.
pub fn fd_id(file: &fs::File) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    file.metadata().ok().map(|m| (m.dev(), m.ino()))
}

/// Whether `pid` is a running process of this user.
pub fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Whether some process holds the lock file at `path`.
pub fn lock_file_held(path: &Path) -> bool {
    let Ok(f) = OpenOptions::new().read(true).write(true).open(path) else {
        return false;
    };
    match f.try_lock() {
        Ok(()) => {
            let _ = f.unlock();
            false
        }
        Err(_) => true,
    }
}

/// An identifier of this build: the version plus the executable's size and modification time.
pub fn binary_id(exe: &Path) -> String {
    let stamp = fs::metadata(exe)
        .ok()
        .map(|m| {
            let mtime = crate::ingest::walk::mtime_ns(&m);
            format!("{}-{mtime}", m.len())
        })
        .unwrap_or_else(|| "unknown".into());
    format!("{}+{stamp}", crate::api::VERSION)
}

/// What a daemon reports about itself (FR-095).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    /// `starting`, `reconciling`, `ready`, `git-operation` or `degraded`.
    pub state: String,
    /// Whether the graph is current enough to serve (readiness, as opposed to liveness).
    pub ready: bool,
    pub pid: u32,
    pub root: String,
    pub endpoint: String,
    pub version: String,
    pub binary_id: String,
    pub schema_version: u32,
    pub started_at: String,
    /// The ingest watermark: file events applied so far (FR-043).
    pub ingest_watermark: u64,
    pub events_received: u64,
    /// Paths waiting to be applied.
    pub pending_events: usize,
    /// How long the oldest pending change has waited, in milliseconds.
    pub lag_ms: u64,
    pub last_ingest: Option<String>,
    pub last_error: Option<String>,
    pub connections: usize,
    pub subscribers: usize,
    pub requests_served: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_paths_are_short_and_stable() {
        let dir = PathBuf::from("/tmp/r");
        let a = socket_path(
            &dir,
            Path::new("/home/someone/projects/a-very/long/path/to/repo"),
        );
        assert_eq!(
            a,
            socket_path(
                &dir,
                Path::new("/home/someone/projects/a-very/long/path/to/repo")
            )
        );
        assert!(a.as_os_str().len() < 40);
        assert_ne!(a, socket_path(&dir, Path::new("/home/someone/other")));
    }
}
