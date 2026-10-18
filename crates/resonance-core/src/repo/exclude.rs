//! Adds and removes the `/.resonance/` line in `.git/info/exclude` (FR-025, FR-180).
//!
//! `.git/info/exclude` is local to the clone, so excluding `.resonance/` there changes no
//! tracked file and leaves `git status` unchanged (NFR-041).

use std::fs;
use std::path::PathBuf;

use super::{Repo, abs_path};
use crate::error::{Error, Result};

/// The exclusion line Resonance owns.
pub const EXCLUDE_LINE: &str = "/.resonance/";

/// The clone's exclude file, found through `git rev-parse --git-path info/exclude` (FR-025).
pub fn exclude_file(repo: &Repo) -> Result<PathBuf> {
    let p = repo.git(&["rev-parse", "--git-path", "info/exclude"])?;
    let path = PathBuf::from(&p);
    Ok(if path.is_absolute() {
        path
    } else {
        abs_path(&repo.root, &p)
    })
}

/// Add the exclusion if it is not there yet. Returns whether the file changed.
pub fn add(repo: &Repo) -> Result<bool> {
    add_line(repo, EXCLUDE_LINE)
}

/// Add one exclusion line if it is not there yet (local agent files, MVP-038).
pub fn add_line(repo: &Repo, line: &str) -> Result<bool> {
    let path = exclude_file(repo)?;
    let current = fs::read_to_string(&path).unwrap_or_default();
    if current.lines().any(|l| l.trim() == line) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| Error::io(format!("creating {}", dir.display()), e))?;
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(line);
    next.push('\n');
    fs::write(&path, next).map_err(|e| Error::io(format!("writing {}", path.display()), e))?;
    Ok(true)
}

/// Remove the exclusion line, leaving every other line as it was. Returns whether it was there.
pub fn remove(repo: &Repo) -> Result<bool> {
    let path = exclude_file(repo)?;
    let Ok(current) = fs::read_to_string(&path) else {
        return Ok(false);
    };
    if !current.lines().any(|l| l.trim() == EXCLUDE_LINE) {
        return Ok(false);
    }
    let mut next: String = current
        .lines()
        .filter(|l| l.trim() != EXCLUDE_LINE)
        .map(|l| format!("{l}\n"))
        .collect();
    if !current.ends_with('\n') && next.ends_with('\n') {
        next.pop();
    }
    fs::write(&path, next).map_err(|e| Error::io(format!("writing {}", path.display()), e))?;
    Ok(true)
}

/// Whether the exclusion is present.
pub fn is_present(repo: &Repo) -> bool {
    exclude_file(repo)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .is_some_and(|s| s.lines().any(|l| l.trim() == EXCLUDE_LINE))
}
