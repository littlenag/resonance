//! Walks the working tree, honoring .gitignore, built-in ignores and config (FR-045, FR-046).

use std::path::Path;
use std::time::UNIX_EPOCH;

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::error::{Error, Result};

/// Generated and build-output directories, never ingested (FR-046, NFR-004). Matched by
/// directory name at any depth, as a `.gitignore` entry such as `build/` would be.
pub const DEFAULT_IGNORED_DIRS: &[&str] = &[
    "target",
    ".bsp",
    ".bloop",
    ".metals",
    "build",
    "dist",
    "out",
    ".gradle",
    "node_modules",
    ".venv",
    "__pycache__",
];

/// A file found on disk.
#[derive(Debug, Clone)]
pub struct WalkedFile {
    /// Repository-relative, `/`-separated.
    pub path: String,
    pub size: u64,
    pub mtime_ns: i64,
}

/// Editor scratch files, always ignored (FR-045).
pub fn is_editor_scratch(name: &str) -> bool {
    name.ends_with(".swp") || name.ends_with('~') || name.starts_with(".#") || name == "4913"
}

/// The compiled extra ignore patterns from `ingest.ignore`.
pub fn compile_ignores(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let glob = Glob::new(p)
            .map_err(|e| Error::Config(format!("ingest.ignore pattern `{p}` is not valid: {e}")))?;
        b.add(glob);
        // `vendor` should also ignore everything under it.
        if !p.ends_with("/**")
            && let Ok(g) = Glob::new(&format!("{}/**", p.trim_end_matches('/')))
        {
            b.add(g);
        }
    }
    b.build()
        .map_err(|e| Error::Config(format!("ingest.ignore is not valid: {e}")))
}

/// Whether a repository-relative path is one Resonance ingests, judged by the built-in
/// rules and `ingest.ignore` alone (git's own ignore rules are applied by the walk).
pub fn is_ignored_path(path: &str, extra: &GlobSet) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    if parts
        .first()
        .is_some_and(|p| *p == ".git" || *p == ".resonance")
    {
        return true;
    }
    let (dirs, name) = parts.split_at(parts.len().saturating_sub(1));
    if dirs
        .iter()
        .any(|d| *d == ".git" || DEFAULT_IGNORED_DIRS.contains(d))
    {
        return true;
    }
    if name.first().is_some_and(|n| is_editor_scratch(n)) {
        return true;
    }
    extra.is_match(path)
}

/// Every regular file Resonance should ingest, sorted by path.
pub fn walk(root: &Path, extra_ignores: &[String]) -> Result<Vec<WalkedFile>> {
    let extra = compile_ignores(extra_ignores)?;
    let root_buf = root.to_path_buf();
    let filter_extra = extra.clone();
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(false)
        .parents(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            let Ok(rel) = entry.path().strip_prefix(&root_buf) else {
                return true;
            };
            if rel.as_os_str().is_empty() {
                return true;
            }
            let rel = to_slash(rel);
            let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
            if is_dir {
                let name = entry.file_name().to_string_lossy();
                if name == ".git" || DEFAULT_IGNORED_DIRS.contains(&name.as_ref()) {
                    return false;
                }
                if rel == ".resonance" {
                    return false;
                }
                // A nested repository (or submodule) is a separate project.
                if entry.path().join(".git").exists() {
                    return false;
                }
                return !filter_extra.is_match(&rel);
            }
            true
        });
    let mut files = Vec::new();
    for result in builder.build() {
        let entry = result.map_err(|e| Error::Io {
            context: "walking the working tree".into(),
            source: std::io::Error::other(e.to_string()),
        })?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        let path = to_slash(rel);
        if is_ignored_path(&path, &extra) {
            continue;
        }
        let Ok(md) = entry.metadata() else {
            continue; // Vanished during the walk; the next reconcile will see it.
        };
        files.push(WalkedFile {
            path,
            size: md.len(),
            mtime_ns: mtime_ns(&md),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Modification time in nanoseconds since the epoch.
pub fn mtime_ns(md: &std::fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

fn to_slash(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_ignores() {
        let none = compile_ignores(&[]).unwrap();
        for ignored in [
            ".git/config",
            ".resonance/graph.db",
            "target/debug/x",
            "web/node_modules/a.js",
            "src/a.rs.swp",
            "src/a.rs~",
            "src/.#a.rs",
            "src/4913",
            "py/__pycache__/m.pyc",
        ] {
            assert!(is_ignored_path(ignored, &none), "{ignored}");
        }
        for kept in [
            "src/main.rs",
            "README.md",
            ".github/workflows/ci.yml",
            "src/builder.rs",
        ] {
            assert!(!is_ignored_path(kept, &none), "{kept}");
        }
        let extra = compile_ignores(&["vendor".into(), "*.gen.go".into()]).unwrap();
        assert!(is_ignored_path("vendor/lib/a.c", &extra));
        assert!(is_ignored_path("api.gen.go", &extra));
        assert!(!is_ignored_path("src/vendor.rs", &extra));
    }
}
