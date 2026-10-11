//! Shared helpers: a temporary git repository and a way to run `resonance` in it.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

/// A throwaway git repository with its own HOME, so that no user configuration leaks in.
pub struct TestRepo {
    dir: TempDir,
    home: TempDir,
    /// A short directory for daemon sockets (socket paths are limited to about 100 bytes).
    runtime: TempDir,
    extra_path: Option<PathBuf>,
    /// Always go through a daemon, even when the suite runs in direct mode.
    force_daemon: bool,
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        // Stop this repository's daemon, if one was started.
        let _ = self.command(&["daemon", "--stop"]).output();
    }
}

/// Whether the tests run in direct mode (`RESONANCE_TEST_DIRECT=1`) instead of through daemons.
pub fn direct_mode() -> bool {
    std::env::var("RESONANCE_TEST_DIRECT").is_ok_and(|v| v == "1")
}

pub fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_resonance")
}

impl TestRepo {
    /// An empty repository with one commit.
    pub fn new() -> TestRepo {
        let repo = TestRepo {
            dir: tempfile::tempdir().expect("tempdir"),
            home: tempfile::tempdir().expect("home"),
            runtime: tempfile::Builder::new()
                .prefix("rr")
                .tempdir_in("/tmp")
                .expect("runtime dir"),
            extra_path: None,
            force_daemon: false,
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "dev@example.com"]);
        repo.git(&["config", "user.name", "Dev"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.write("README.md", "# demo\n");
        repo.commit_all("initial");
        repo
    }

    /// A repository with a small Rust-like project, a spec, and Resonance initialized.
    pub fn with_project() -> TestRepo {
        Self::project(false)
    }

    /// [`TestRepo::with_project`], always served by a daemon.
    pub fn with_project_daemon() -> TestRepo {
        Self::project(true)
    }

    fn project(force_daemon: bool) -> TestRepo {
        let mut repo = TestRepo::new();
        repo.force_daemon = force_daemon;
        repo.write("spec/requirements.md", SPEC);
        repo.write("src/store.rs", STORE);
        repo.write("tests/store_test.sh", TEST_SCRIPT);
        repo.commit_all("project");
        repo.ok(&["init", "--no-agent"]);
        repo
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn home(&self) -> &Path {
        self.home.path()
    }

    /// Prepend a directory to PATH for every `resonance` run.
    pub fn with_path(&mut self, dir: PathBuf) {
        self.extra_path = Some(dir);
    }

    pub fn write(&self, rel: &str, content: &str) {
        let p = self.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    pub fn write_bytes(&self, rel: &str, content: &[u8]) {
        let p = self.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path().join(rel)).unwrap()
    }

    pub fn remove(&self, rel: &str) {
        fs::remove_file(self.path().join(rel)).unwrap();
    }

    pub fn rename(&self, from: &str, to: &str) {
        let dst = self.path().join(to);
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        fs::rename(self.path().join(from), dst).unwrap();
    }

    /// Replace the first occurrence of `from` in a file.
    pub fn replace(&self, rel: &str, from: &str, to: &str) {
        let text = self.read(rel);
        assert!(text.contains(from), "{rel} does not contain {from:?}");
        self.write(rel, &text.replacen(from, to, 1));
    }

    pub fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .env("HOME", self.home())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    /// `git status --porcelain --ignored=no`.
    pub fn git_status(&self) -> String {
        self.git(&["status", "--porcelain"])
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(bin());
        cmd.args(args)
            .current_dir(self.path())
            .env("HOME", self.home())
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for (k, _) in std::env::vars() {
            if k.starts_with("RESONANCE_") {
                cmd.env_remove(k);
            }
        }
        cmd.env("RESONANCE_RUNTIME_DIR", self.runtime.path())
            .env("RESONANCE_DAEMON_IDLE_SECONDS", "120");
        if direct_mode() && !self.force_daemon {
            cmd.env("RESONANCE_NO_DAEMON", "1");
        }
        if let Some(extra) = &self.extra_path {
            let path = std::env::var("PATH").unwrap_or_default();
            cmd.env("PATH", format!("{}:{path}", extra.display()));
        }
        cmd
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("run resonance")
    }

    /// Run and require success; returns standard output.
    pub fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "resonance {args:?} failed ({:?}):\nstdout: {}\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Run with `--json` and require success.
    pub fn json(&self, args: &[&str]) -> Value {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let out = self.ok(&all);
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("not JSON ({e}): {out}"))
    }

    /// Run and require failure with this exit code; returns standard error.
    pub fn fails(&self, args: &[&str], code: i32) -> String {
        let out = self.run(args);
        assert_eq!(
            out.status.code(),
            Some(code),
            "resonance {args:?}: expected exit {code}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    /// Declare the standard units and tests of the sample project and link them.
    pub fn declare_sample(&self) {
        self.ok(&["import", "spec/requirements.md"]);
        self.ok(&[
            "unit",
            "add",
            "src/store.rs:4-9",
            "--name",
            "open",
            "--kind",
            "function",
        ]);
        self.ok(&[
            "unit",
            "add",
            "src/store.rs:11-14",
            "--name",
            "close",
            "--kind",
            "function",
        ]);
        self.ok(&[
            "test",
            "add",
            "tests/store_test.sh:3-5",
            "--name",
            "opens_db",
            "--run",
            "sh tests/store_test.sh open",
        ]);
        self.ok(&["link", "open", "satisfies", "FR-001"]);
        self.ok(&["link", "close", "satisfies", "FR-002"]);
        self.ok(&["link", "opens_db", "verifies", "FR-001"]);
    }
}

pub const SPEC: &str = "\
# Sample requirements

- **FR-001** The store opens a database at a path. [Decided]
- **FR-002** The store closes cleanly, flushing pending writes. [Proposed]
- **FR-003** The store reports its size.

| ID | Requirement | Target | Status |
| --- | --- | --- | --- |
| **NFR-001** | Opening is fast | p95 under 10 ms | [Open] |
";

pub const STORE: &str = "\
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

pub const TEST_SCRIPT: &str = "\
#!/bin/sh
# Tests for the store, one per argument.
case \"$1\" in
  open) echo \"open ok\"; exit 0 ;;
esac
echo \"unknown test $1\"; exit 1
";
