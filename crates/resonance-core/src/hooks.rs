//! Git hooks (FR-069, FR-177, MVP-035).
//!
//! Each hook gets a block between `# >>> resonance >>>` and `# <<< resonance <<<` that runs
//! `resonance hook <name> "$@"`. A missing hook file is created as a POSIX shell script; an
//! existing shell script gets the block appended after its own content. If the existing hook
//! is not a shell script, or a hook manager is in charge, nothing is written and the lines to
//! add are returned instead. Removal takes out only the marked block.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::repo::{Repo, abs_path};

pub const BEGIN: &str = "# >>> resonance >>>";
pub const END: &str = "# <<< resonance <<<";

/// Hooks for a developer who has opted in: the checks may block (FR-203).
pub const OPTED_IN: &[&str] = &[
    "pre-commit",
    "pre-push",
    "post-checkout",
    "post-merge",
    "post-rewrite",
];
/// Hooks for offline mode: notification only, never blocking (FR-188).
pub const OFFLINE: &[&str] = &["post-checkout", "post-merge", "post-rewrite"];
/// Every hook Resonance may install.
pub const ALL: &[&str] = &[
    "pre-commit",
    "pre-push",
    "post-checkout",
    "post-merge",
    "post-rewrite",
];

/// What happened to one hook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookChange {
    pub hook: String,
    /// `created`, `appended`, `updated`, `unchanged`, `removed`, or `manual` (instructions).
    pub action: String,
    pub path: String,
    /// For `manual`: the lines to add by hand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// The hooks directory, honoring `core.hooksPath`.
pub fn hooks_dir(repo: &Repo) -> Result<PathBuf> {
    let p = repo.git(&["rev-parse", "--git-path", "hooks"])?;
    let path = PathBuf::from(&p);
    Ok(if path.is_absolute() {
        path
    } else {
        abs_path(&repo.root, &p)
    })
}

/// A hook manager that owns the hooks (husky, pre-commit, lefthook), if any.
pub fn manager(repo: &Repo) -> Option<String> {
    for (file, name) in [
        (".husky", "husky"),
        (".pre-commit-config.yaml", "pre-commit"),
        ("lefthook.yml", "lefthook"),
        ("lefthook.yaml", "lefthook"),
    ] {
        if repo.root.join(file).exists() {
            return Some(name.to_string());
        }
    }
    let hooks_path = repo.git(&["config", "--get", "core.hooksPath"]).ok()?;
    let p = PathBuf::from(&hooks_path);
    let abs = if p.is_absolute() {
        p
    } else {
        repo.root.join(p)
    };
    abs.starts_with(&repo.root)
        .then(|| "core.hooksPath".to_string())
}

/// The block for one hook.
pub fn block(exe: &Path, hook: &str, blocking: bool) -> String {
    let fail = if blocking { "|| exit $?" } else { "|| true" };
    format!(
        "{BEGIN}\n# Added by Resonance; `resonance leave` or `resonance eject` removes it.\nif [ -x \"{exe}\" ]; then\n  \"{exe}\" hook {hook} \"$@\" {fail}\nelif command -v resonance >/dev/null 2>&1; then\n  resonance hook {hook} \"$@\" {fail}\nfi\n{END}\n",
        exe = exe.display()
    )
}

fn is_shell(text: &str) -> bool {
    let first = text.lines().next().unwrap_or_default();
    first.starts_with("#!")
        && ["sh", "bash", "zsh", "dash"]
            .iter()
            .any(|s| first.ends_with(&format!("/{s}")) || first.ends_with(&format!(" {s}")))
}

/// Remove the marked block from `text`.
fn strip(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in text.lines() {
        if line.trim() == BEGIN {
            inside = true;
            continue;
        }
        if line.trim() == END {
            inside = false;
            continue;
        }
        if !inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Install `names` (blocking or not), and remove Resonance's block from the other hooks.
pub fn install(
    repo: &Repo,
    exe: &Path,
    names: &[&str],
    blocking: bool,
    dry_run: bool,
) -> Result<Vec<HookChange>> {
    let dir = hooks_dir(repo)?;
    let manager = manager(repo);
    let mut out = Vec::new();
    for hook in ALL {
        let path = dir.join(hook);
        let wanted = names.contains(hook);
        let current = fs::read_to_string(&path).ok();
        let has_block = current.as_deref().is_some_and(|t| t.contains(BEGIN));
        let text = block(
            exe,
            hook,
            blocking && (*hook == "pre-commit" || *hook == "pre-push"),
        );
        let change = |action: &str, instructions: Option<String>| HookChange {
            hook: hook.to_string(),
            action: action.to_string(),
            path: path.display().to_string(),
            instructions,
        };
        if !wanted {
            if has_block {
                if !dry_run {
                    let rest = strip(current.as_deref().unwrap_or_default());
                    write_hook(&path, &rest)?;
                }
                out.push(change("removed", None));
            }
            continue;
        }
        if let Some(m) = &manager {
            out.push(change(
                "manual",
                Some(format!(
                    "{m} manages the git hooks here; add this to its {hook} hook:\n{text}"
                )),
            ));
            continue;
        }
        match current {
            None => {
                if !dry_run {
                    write_hook(&path, &format!("#!/bin/sh\n{text}"))?;
                }
                out.push(change("created", None));
            }
            Some(t) if has_block => {
                let next = format!("{}{text}", strip(&t));
                if next == t {
                    out.push(change("unchanged", None));
                } else {
                    if !dry_run {
                        write_hook(&path, &next)?;
                    }
                    out.push(change("updated", None));
                }
            }
            Some(t) if is_shell(&t) => {
                if !dry_run {
                    let mut next = t.clone();
                    if !next.ends_with('\n') {
                        next.push('\n');
                    }
                    next.push_str(&text);
                    write_hook(&path, &next)?;
                }
                out.push(change("appended", None));
            }
            Some(_) => out.push(change(
                "manual",
                Some(format!(
                    "{} is not a shell script; add this to it by hand:\n{text}",
                    path.display()
                )),
            )),
        }
    }
    Ok(out)
}

/// Remove Resonance's block from every hook.
pub fn uninstall(repo: &Repo) -> Result<Vec<HookChange>> {
    let dir = hooks_dir(repo)?;
    let mut out = Vec::new();
    for hook in ALL {
        let path = dir.join(hook);
        let Ok(t) = fs::read_to_string(&path) else {
            continue;
        };
        if !t.contains(BEGIN) {
            continue;
        }
        let rest = strip(&t);
        if rest.trim() == "#!/bin/sh" || rest.trim().is_empty() {
            fs::remove_file(&path)
                .map_err(|e| Error::io(format!("removing {}", path.display()), e))?;
        } else {
            write_hook(&path, &rest)?;
        }
        out.push(HookChange {
            hook: hook.to_string(),
            action: "removed".into(),
            path: path.display().to_string(),
            instructions: None,
        });
    }
    Ok(out)
}

fn write_hook(path: &Path, text: &str) -> Result<()> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(|e| Error::io(format!("creating {}", d.display()), e))?;
    }
    fs::write(path, text).map_err(|e| Error::io(format!("writing {}", path.display()), e))?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
    Ok(())
}

/// Which of Resonance's hooks are installed.
pub fn installed(repo: &Repo) -> Vec<String> {
    let Ok(dir) = hooks_dir(repo) else {
        return Vec::new();
    };
    ALL.iter()
        .filter(|h| fs::read_to_string(dir.join(h)).is_ok_and(|t| t.contains(BEGIN)))
        .map(|h| h.to_string())
        .collect()
}
