//! `init` and `eject`: offline setup and clean removal (FR-180, FR-173, FR-184).
//!
//! Offline mode (§8.17) modifies no tracked file: `.resonance/` is excluded through
//! `.git/info/exclude`, which is local to the clone, so `git status` is unchanged (NFR-041).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ops::hlc;
use crate::repo::config::DEFAULT_CONFIG;
use crate::repo::{Repo, exclude};
use crate::store::{GraphScope, GraphStore, meta};

/// Opens (creating if needed) the store at a path. Supplied by the binary, so that this
/// crate never names a storage engine (FR-161).
pub type StoreFactory<'a> = &'a dyn Fn(&Path) -> Result<Box<dyn GraphStore>>;

/// What `init` set up.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitSetup {
    pub already_initialized: bool,
    pub exclusion_added: bool,
    pub config_written: bool,
}

/// The few lines `init` prints about offline mode (FR-180).
pub const OFFLINE_EXPLANATION: &str = "\
Resonance is running in offline mode: a private graph for you and your agent, kept in
.resonance/ and excluded through .git/info/exclude. Nothing is committed, no tracked file
changes, and your teammates see no trace of it.

What offline mode cannot do: share links, annotations or requirements with teammates, or
follow their renames for you. When their changes move or rewrite code you annotated, those
annotations go on the repair list (`resonance status`) for your agent to re-attach.
That repair is the cost of trying Resonance privately; it is smallest on repositories with
few outside changes.";

/// Create `.resonance/` with its store and configuration, and exclude it locally. Safe to
/// run again: an existing setup is left as it is.
pub fn init(repo: &Repo, open_store: StoreFactory<'_>) -> Result<InitSetup> {
    ensure_layout(repo, open_store)?;
    let already = repo.is_initialized();
    // Exclude first, so that `.resonance/` is never visible to git, even for a moment. A
    // repository with committed data shares `.resonance/`: its own .gitignore keeps `local/`
    // out of git, and excluding the directory would hide the deltas.
    let exclusion_added = if repo.has_committed_data() {
        false
    } else {
        exclude::add(repo)?
    };
    let dir = repo.local_dir();
    fs::create_dir_all(&dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
    let config = repo.config_path();
    let config_written = !config.exists();
    if config_written {
        fs::write(&config, DEFAULT_CONFIG)
            .map_err(|e| Error::io(format!("writing {}", config.display()), e))?;
    }
    let mut store = open_store(&repo.store_path())?;
    store.migrate()?;
    store.write(GraphScope::Primary, &mut |w| {
        if w.get_meta(meta::STAGE)?.is_none() {
            w.set_meta(meta::STAGE, "1")?;
        }
        if w.get_meta(meta::PARTICIPATION)?.is_none() {
            w.set_meta(meta::PARTICIPATION, "offline")?;
        }
        Ok(())
    })?;
    Ok(InitSetup {
        already_initialized: already,
        exclusion_added,
        config_written,
    })
}

/// The files of the earlier layout that move under `local/` (MVP-025).
const LEGACY_LOCAL: &[&str] = &[
    "daemon.lock",
    "daemon.json",
    "daemon.log",
    "cookies",
    "graph.db-wal",
    "graph.db-shm",
];

/// Migrate a repository initialized with the earlier MVP layout (MVP-025): the live store at
/// `.resonance/graph.db` moves to `.resonance/local/state.db` (after a backup), and the
/// daemon's files move under `local/`. Its operation log becomes the open delta, so nothing
/// is lost and nothing is rewritten. Returns a one-line report when it migrated.
pub fn ensure_layout(repo: &Repo, open_store: StoreFactory<'_>) -> Result<Option<String>> {
    if !repo.has_legacy_layout() {
        return Ok(None);
    }
    let path = repo.snapshot_path();
    {
        let probe = open_store(&path)?;
        if probe.read_snapshot(&path).is_ok() {
            return Ok(None); // A snapshot already: the new layout without local state yet.
        }
    }
    if crate::daemon::runtime::lock_file_held(&repo.resonance_dir().join("daemon.lock")) {
        return Err(Error::Invalid(
            "a resonance daemon from an earlier version is running; stop it with `resonance daemon stop` and try again".into(),
        ));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let saved = backup(repo, &home, "pre-migration")?;
    let local = repo.local_dir();
    fs::create_dir_all(&local)
        .map_err(|e| Error::io(format!("creating {}", local.display()), e))?;
    let res = repo.resonance_dir();
    for name in LEGACY_LOCAL {
        let from = res.join(name);
        if from.exists() {
            let to = match *name {
                "graph.db-wal" => local.join("state.db-wal"),
                "graph.db-shm" => local.join("state.db-shm"),
                other => local.join(other),
            };
            fs::rename(&from, &to)
                .map_err(|e| Error::io(format!("moving {}", from.display()), e))?;
        }
    }
    fs::rename(&path, repo.store_path())
        .map_err(|e| Error::io(format!("moving {}", path.display()), e))?;
    Ok(Some(format!(
        "Moved .resonance/ to the new layout (live store in .resonance/local/); a backup is in {}.",
        saved.display()
    )))
}

/// Copy `.resonance/` outside the repository (FR-184): to
/// `~/.resonance/backups/<repo>-<hash>/<stamp>[-label]/`. Sockets, locks and caches are skipped.
pub fn backup(repo: &Repo, home: &Path, label: &str) -> Result<PathBuf> {
    let stamp = hlc::rfc3339(hlc::now_ms())
        .replace([':', '.'], "-")
        .trim_end_matches('Z')
        .to_string();
    let name = if label.is_empty() {
        stamp
    } else {
        format!("{stamp}-{label}")
    };
    let dest = backups_dir(repo, home).join(name);
    copy_tree(&repo.resonance_dir(), &dest)?;
    Ok(dest)
}

/// Where this repository's backups go.
pub fn backups_dir(repo: &Repo, home: &Path) -> PathBuf {
    let id = &crate::model::content_hash(repo.root.as_os_str().as_encoded_bytes())[..8];
    home.join(".resonance")
        .join("backups")
        .join(format!("{}-{id}", repo.name()))
}

/// The newest backup of this repository, if any.
pub fn latest_backup(repo: &Repo, home: &Path) -> Option<PathBuf> {
    let mut all: Vec<PathBuf> = fs::read_dir(backups_dir(repo, home))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    all.sort();
    all.pop()
}

/// Restore `.resonance/` from a backup. Refuses when `.resonance/` exists.
pub fn restore_backup(repo: &Repo, from: &Path) -> Result<()> {
    if repo.resonance_dir().exists() {
        return Err(Error::Invalid(
            ".resonance/ exists; restoring a backup only replaces a missing .resonance/".into(),
        ));
    }
    copy_tree(from, &repo.resonance_dir())?;
    // Stage 2 already tracks `.resonance/`. An exclude line would hide new shared files
    // from this clone. Exclude only a tree git has never committed (stage 1).
    if !repo.has_committed_data() {
        exclude::add(repo)?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).map_err(|e| Error::io(format!("creating {}", to.display()), e))?;
    let entries =
        fs::read_dir(from).map_err(|e| Error::io(format!("reading {}", from.display()), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io("reading a directory entry", e))?;
        let name = entry.file_name();
        let n = name.to_string_lossy();
        if matches!(
            n.as_ref(),
            "cookies" | "cache" | "daemon.lock" | "daemon.json"
        ) {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        let ft = entry
            .file_type()
            .map_err(|e| Error::io("reading a file type", e))?;
        if ft.is_dir() {
            copy_tree(&src, &dst)?;
        } else if ft.is_file() {
            fs::copy(&src, &dst).map_err(|e| Error::io(format!("copying {}", src.display()), e))?;
        }
    }
    Ok(())
}

/// Where `eject` writes its export by default: outside the repository (FR-173).
pub fn default_export_path(repo: &Repo, home: &Path) -> PathBuf {
    let stamp = hlc::rfc3339(hlc::now_ms())
        .replace([':', '.'], "-")
        .trim_end_matches('Z')
        .to_string();
    home.join(".resonance")
        .join("exports")
        .join(format!("{}-{stamp}.jsonl", repo.name()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Removal {
    pub removed_directory: bool,
    pub exclusion_removed: bool,
    /// Resonance's blocks removed from git hooks.
    #[serde(default)]
    pub hooks: Vec<crate::hooks::HookChange>,
    /// The agent kit's working-tree parts removed.
    #[serde(default)]
    pub kit: Vec<crate::agent::KitStep>,
}

/// Remove the git hook blocks, the agent kit's working-tree parts, `.resonance/` and the
/// exclusion line. The only code in Resonance that deletes `.resonance/` (FR-184); call it
/// only after an export and the developer's confirmation.
pub fn remove(repo: &Repo) -> Result<Removal> {
    let hooks = crate::hooks::uninstall(repo)?;
    let kit = crate::agent::uninstall_kit(repo)?;
    let dir = repo.resonance_dir();
    let removed_directory = dir.exists();
    if removed_directory {
        fs::remove_dir_all(&dir)
            .map_err(|e| Error::io(format!("removing {}", dir.display()), e))?;
    }
    let exclusion_removed = exclude::remove(repo)?;
    Ok(Removal {
        removed_directory,
        exclusion_removed,
        hooks,
        kit,
    })
}
