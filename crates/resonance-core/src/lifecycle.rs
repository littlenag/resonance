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
Resonance is running in offline mode: a private graph for you alone, kept in .resonance/
and excluded through .git/info/exclude. Nothing is committed, no tracked file changes, and
your teammates see no trace of it.

What offline mode cannot do: share links, annotations or requirements with teammates, or
follow their renames for you. When their changes move or rewrite code you annotated, those
annotations go on the repair list (`resonance status`) for you or your agent to re-attach.
That repair is the cost of trying Resonance privately; it is smallest on repositories with
few outside changes.";

/// Create `.resonance/` with its store and configuration, and exclude it locally. Safe to
/// run again: an existing setup is left as it is.
pub fn init(repo: &Repo, open_store: StoreFactory<'_>) -> Result<InitSetup> {
    let already = repo.is_initialized();
    // Exclude first, so that `.resonance/` is never visible to git, even for a moment.
    let exclusion_added = exclude::add(repo)?;
    let dir = repo.resonance_dir();
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
}

/// Remove `.resonance/` and the exclusion line. The only code in Resonance that deletes
/// `.resonance/` (FR-184); call it only after an export and the developer's confirmation.
pub fn remove(repo: &Repo) -> Result<Removal> {
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
    })
}
