//! Repository discovery and local configuration.

pub mod config;
pub mod exclude;

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

/// The directory Resonance keeps its data in, at the repository root (FR-021).
pub const RESONANCE_DIR: &str = ".resonance";

/// The never-committed part of `.resonance/` (MVP-025).
pub const LOCAL_DIR: &str = "local";

/// A git repository that Resonance runs in (MVP-009).
#[derive(Debug, Clone)]
pub struct Repo {
    /// The working-tree root, as git reports it (symlinks resolved).
    pub root: PathBuf,
}

impl Repo {
    /// Find the repository containing `start`, using `git rev-parse --show-toplevel` (MVP-009).
    pub fn discover(start: &Path) -> Result<Repo> {
        let out = Command::new("git")
            .arg("-C")
            .arg(start)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .map_err(|e| Error::Git(format!("could not run git: {e}")))?;
        if !out.status.success() {
            return Err(Error::NotARepository(start.display().to_string()));
        }
        let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if root.is_empty() {
            // A bare repository, or inside `.git`.
            return Err(Error::NotARepository(start.display().to_string()));
        }
        let root = PathBuf::from(root);
        let root = root.canonicalize().unwrap_or(root);
        Ok(Repo { root })
    }

    pub fn resonance_dir(&self) -> PathBuf {
        self.root.join(RESONANCE_DIR)
    }

    /// `.resonance/local/`: everything that is never committed (MVP-025).
    pub fn local_dir(&self) -> PathBuf {
        self.resonance_dir().join(LOCAL_DIR)
    }

    /// The live store, `.resonance/local/state.db` (FR-169). Never the snapshot.
    pub fn store_path(&self) -> PathBuf {
        self.local_dir().join("state.db")
    }

    /// The snapshot, `.resonance/graph.db` (MVP-027): committed from stage 2.
    pub fn snapshot_path(&self) -> PathBuf {
        self.resonance_dir().join("graph.db")
    }

    /// Sealed and open deltas (MVP-026): committed from stage 2.
    pub fn deltas_dir(&self) -> PathBuf {
        self.resonance_dir().join("deltas")
    }

    /// Private deltas: an offline layer over a shared graph (FR-187).
    pub fn private_dir(&self) -> PathBuf {
        self.local_dir().join("private")
    }

    /// The journal of every operation applied or written (FR-194).
    pub fn journal_dir(&self) -> PathBuf {
        self.local_dir().join("journal")
    }

    /// Versions displaced by resolutions (FR-192).
    pub fn recovery_dir(&self) -> PathBuf {
        self.local_dir().join("recovery")
    }

    /// Derived data keyed by tree hash (FR-024).
    pub fn cache_dir(&self) -> PathBuf {
        self.local_dir().join("cache")
    }

    pub fn config_path(&self) -> PathBuf {
        self.resonance_dir().join("config.toml")
    }

    /// Whether this working tree has a live store.
    pub fn is_initialized(&self) -> bool {
        self.store_path().is_file()
    }

    /// Whether `.resonance/` may use the earlier MVP layout: a store at `.resonance/graph.db`,
    /// no `local/` directory, and none of the stage-2 files (MVP-025). The migration confirms
    /// that the file is a live store and not a snapshot before moving it.
    pub fn has_legacy_layout(&self) -> bool {
        self.snapshot_path().is_file()
            && !self.local_dir().exists()
            && !self.resonance_dir().join(".gitignore").exists()
            && !self.deltas_dir().exists()
    }

    /// Whether committed Resonance data exists in this working tree (stage 2, FR-172).
    pub fn has_committed_data(&self) -> bool {
        self.git(&["ls-files", "--", ".resonance/graph.db", ".resonance/deltas"])
            .is_ok_and(|s| !s.trim().is_empty())
    }

    /// The repository's directory name, for messages and export file names.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repository".into())
    }

    /// Run git in the repository root and return its trimmed standard output.
    pub fn git(&self, args: &[&str]) -> Result<String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(|e| Error::Git(format!("could not run git: {e}")))?;
        if !out.status.success() {
            return Err(Error::Git(format!(
                "`git {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Run git and return its raw standard output.
    pub fn git_bytes(&self, args: &[&str]) -> Result<Vec<u8>> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(|e| Error::Git(format!("could not run git: {e}")))?;
        if !out.status.success() {
            return Err(Error::Git(format!(
                "`git {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(out.stdout)
    }

    /// Run git with `input` on its standard input; returns trimmed standard output.
    pub fn git_stdin(&self, args: &[&str], input: &[u8]) -> Result<String> {
        use std::io::Write;
        use std::process::Stdio;
        let mut child = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Git(format!("could not run git: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input);
        }
        let out = child
            .wait_with_output()
            .map_err(|e| Error::Git(format!("could not run git: {e}")))?;
        if !out.status.success() {
            return Err(Error::Git(format!(
                "`git {}` failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// The commit checked out, if there is one.
    pub fn head_commit(&self) -> Option<String> {
        self.git(&["rev-parse", "--verify", "--quiet", "HEAD"]).ok()
    }

    /// Resolve a path typed by a person (relative to `cwd`, or absolute) into a
    /// repository-relative path with `/` separators.
    pub fn relative_path(&self, cwd: &Path, input: &str) -> Result<String> {
        let joined = if Path::new(input).is_absolute() {
            PathBuf::from(input)
        } else {
            cwd.join(input)
        };
        let normalized = joined
            .canonicalize()
            .unwrap_or_else(|_| lexical_normalize(&joined));
        let root = &self.root;
        let lexical = lexical_normalize(&joined);
        let rel = normalized
            .strip_prefix(root)
            // `cwd` may be spelled through a symlink (for example /tmp on macOS).
            .or_else(|_| lexical.strip_prefix(root))
            .map_err(|_| Error::Invalid(format!("{input} is outside the repository")))?
            .to_path_buf();
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        Ok(parts.join("/"))
    }
}

/// Remove `.` and resolve `..` without touching the file system.
pub fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Join a repository-relative `/` path onto the root.
pub fn abs_path(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.split('/') {
        p.push(part);
    }
    p
}
