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
        let _ = self.command(&["daemon", "stop"]).output();
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

    /// A clone of `source` (a teammate's working tree), not initialized.
    pub fn clone_of(source: &Path) -> TestRepo {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = TestRepo {
            dir,
            home: tempfile::tempdir().expect("home"),
            runtime: tempfile::Builder::new()
                .prefix("rr")
                .tempdir_in("/tmp")
                .expect("runtime dir"),
            extra_path: None,
            force_daemon: false,
        };
        let out = Command::new("git")
            .args(["clone", "-q"])
            .arg(source)
            .arg(repo.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git clone");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        repo.git(&["config", "user.email", "mate@example.com"]);
        repo.git(&["config", "user.name", "Mate"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
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

    /// Put a fake `claude` first on PATH, so that `init` can register the MCP server without a
    /// real Claude Code. Returns the log of its calls (`<cwd>|<args>` per line).
    pub fn fake_claude(&mut self) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = self.home().join("bin");
        fs::create_dir_all(&dir).unwrap();
        let log = dir.join("claude-calls.log");
        let path = dir.join("claude");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\necho \"$PWD|$*\" >> '{}'\necho 'Added stdio MCP server resonance'\n",
                log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        self.with_path(dir);
        log
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
        let out = self.git_command(args).output().expect("git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// git, with the same Resonance environment as `command`, so that hooks git runs behave
    /// like the test's own `resonance` runs.
    pub fn git_command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
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
        cmd
    }

    /// Run git and return its output without asserting success.
    pub fn git_output(&self, args: &[&str]) -> Output {
        self.git_command(args).output().expect("git")
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

    /// Call MCP tools in one `resonance mcp` session, as an agent would. Returns each call's
    /// `data`, or its error text.
    pub fn tools(&self, calls: &[(&str, Value)]) -> Vec<Result<Value, String>> {
        use std::io::Write;
        use std::process::Stdio;
        let mut child = self
            .command(&["mcp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start resonance mcp");
        let mut stdin = child.stdin.take().unwrap();
        let mut lines = vec![
            serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "clientInfo": {"name": "test-agent"}}}),
        ];
        for (i, (name, args)) in calls.iter().enumerate() {
            lines.push(
                serde_json::json!({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call",
                "params": {"name": name, "arguments": args}}),
            );
        }
        for l in lines {
            writeln!(stdin, "{l}").unwrap();
        }
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        let responses: Vec<Value> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
            .collect();
        assert_eq!(
            responses.len(),
            calls.len() + 1,
            "resonance mcp: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        responses[1..]
            .iter()
            .map(|r| {
                let text = r["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{r}"))
                    .to_string();
                if r["result"]["isError"] == true {
                    Err(text)
                } else {
                    let body: Value = serde_json::from_str(&text).unwrap();
                    Ok(body["data"].clone())
                }
            })
            .collect()
    }

    /// Call one MCP tool and require success; returns its `data`.
    pub fn tool(&self, name: &str, args: Value) -> Value {
        self.tools(&[(name, args.clone())])
            .remove(0)
            .unwrap_or_else(|e| panic!("{name} {args}: {e}"))
    }

    /// Call one MCP tool and require an error; returns its text.
    pub fn tool_err(&self, name: &str, args: Value) -> String {
        match self.tools(&[(name, args.clone())]).remove(0) {
            Ok(v) => panic!("{name} {args} succeeded: {v}"),
            Err(e) => e,
        }
    }

    /// The arguments of a region `path:START-END` or `path:LINE`.
    pub fn region(region: &str) -> Value {
        let (path, lines) = region.rsplit_once(':').expect("path:START-END");
        let (a, b) = lines.split_once('-').unwrap_or((lines, lines));
        serde_json::json!({"path": path, "startLine": a.parse::<u64>().unwrap(), "endLine": b.parse::<u64>().unwrap()})
    }

    /// Declare a code unit (`resonance_declare_unit`).
    pub fn declare(&self, region: &str, name: &str) -> Value {
        let mut a = Self::region(region);
        a["name"] = name.into();
        self.tool("resonance_declare_unit", a)
    }

    /// Declare a test with its run command (`resonance_declare_test`).
    pub fn declare_test(&self, region: &str, name: &str, run: &str) -> Value {
        let mut a = Self::region(region);
        a["name"] = name.into();
        a["run"] = run.into();
        self.tool("resonance_declare_test", a)
    }

    /// Link two nodes as an agent (`resonance_link`, confidence 0.9).
    pub fn link(&self, source: &str, etype: &str, target: &str) -> Value {
        self.tool(
            "resonance_link",
            serde_json::json!({"source": source, "type": etype, "target": target, "confidence": 0.9}),
        )
    }

    /// Annotate a node (`resonance_annotate`).
    pub fn annotate(&self, target: &str, kind: &str, text: &str) -> Value {
        self.tool(
            "resonance_annotate",
            serde_json::json!({"target": target, "kind": kind, "text": text}),
        )
    }

    /// What an agent brings in from `spec/requirements.md` (MVP-042).
    pub fn import_sample(&self) -> Value {
        let item = |key: &str, text: &str, line: u64| {
            serde_json::json!({"type": "Requirement", "key": key, "fields": {"text": text},
                "region": {"startLine": line, "endLine": line}})
        };
        self.tool(
            "resonance_import_items",
            serde_json::json!({"document": "spec/requirements.md", "items": [
                item("FR-001", "The store opens a database at a path.", 3),
                item("FR-002", "The store closes cleanly, flushing pending writes.", 4),
                item("FR-003", "The store reports its size.", 5),
                item("NFR-001", "Opening is fast: p95 under 10 ms.", 9),
            ]}),
        )
    }

    /// Bring in the sample requirements, declare the sample's units and tests, and link them.
    pub fn declare_sample(&self) {
        self.import_sample();
        self.tools(&[
            ("resonance_declare_unit", serde_json::json!({"path": "src/store.rs", "startLine": 4, "endLine": 9, "name": "open", "kind": "function"})),
            ("resonance_declare_unit", serde_json::json!({"path": "src/store.rs", "startLine": 11, "endLine": 14, "name": "close", "kind": "function"})),
            ("resonance_declare_test", serde_json::json!({"path": "tests/store_test.sh", "startLine": 3, "endLine": 5, "name": "opens_db", "run": "sh tests/store_test.sh open"})),
            ("resonance_link", serde_json::json!({"source": "open", "type": "satisfies", "target": "FR-001", "confidence": 0.9})),
            ("resonance_link", serde_json::json!({"source": "close", "type": "satisfies", "target": "FR-002", "confidence": 0.9})),
            ("resonance_link", serde_json::json!({"source": "opens_db", "type": "verifies", "target": "FR-001", "confidence": 0.9})),
        ])
        .into_iter()
        .for_each(|r| {
            r.unwrap();
        });
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
