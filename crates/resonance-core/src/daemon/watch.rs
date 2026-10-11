//! Native file events (FR-041, MVP-021): one recursive FSEvents stream on macOS; an inotify
//! watch per ingested directory on Linux, so that ignored and build-output directories
//! (FR-046) never consume watches.

use std::path::{Path, PathBuf};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{Error, Result};
use crate::ingest::walk::walk_dirs;

/// A running watcher.
pub struct Watch {
    watcher: RecommendedWatcher,
    root: PathBuf,
    per_directory: bool,
}

fn werr(e: notify::Error) -> Error {
    Error::Daemon(format!("file watching failed: {e}"))
}

impl Watch {
    /// Start watching `root`. Events go to `sink`. `extra` are directories outside the walk
    /// that must also be watched (the cookie directory, the git directory).
    pub fn start(
        root: &Path,
        ignores: &[String],
        extra: &[PathBuf],
        sink: impl Fn(notify::Result<Event>) + Send + 'static,
    ) -> Result<Watch> {
        let watcher = notify::recommended_watcher(sink).map_err(werr)?;
        let per_directory = cfg!(target_os = "linux");
        let mut w = Watch {
            watcher,
            root: root.to_path_buf(),
            per_directory,
        };
        if per_directory {
            for dir in walk_dirs(root, ignores)? {
                w.add_dir(&dir);
            }
            for p in extra {
                let _ = w.watcher.watch(p, RecursiveMode::NonRecursive);
            }
        } else {
            w.watcher
                .watch(root, RecursiveMode::Recursive)
                .map_err(werr)?;
            for p in extra.iter().filter(|p| !p.starts_with(root)) {
                let _ = w.watcher.watch(p, RecursiveMode::NonRecursive);
            }
        }
        Ok(w)
    }

    /// Watch a directory (repository-relative) that appeared. A no-op where one recursive
    /// stream covers the tree.
    pub fn add_dir(&mut self, rel: &str) {
        if !self.per_directory {
            return;
        }
        let path = if rel.is_empty() {
            self.root.clone()
        } else {
            crate::repo::abs_path(&self.root, rel)
        };
        let _ = self.watcher.watch(&path, RecursiveMode::NonRecursive);
    }

    /// Watch every ingested directory under `rel` (after a directory is created).
    pub fn add_tree(&mut self, rel: &str, ignores: &[String]) {
        if !self.per_directory {
            return;
        }
        if let Ok(dirs) = walk_dirs(&self.root, ignores) {
            for d in dirs
                .iter()
                .filter(|d| *d == rel || d.starts_with(&format!("{rel}/")))
            {
                self.add_dir(d);
            }
        }
    }
}
