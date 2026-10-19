//! The agent-side harness hook (FR-222, FR-277, MVP-038): before the agent's own `git commit`,
//! `push`, `pull`, `merge` and `rebase`, run the relevant checks and stop the command when they
//! fail. An explicit instruction from the developer overrides the gate (the agent prefixes the
//! command with `RESONANCE_OVERRIDE=1`); the result is still checked by the git hooks.

use serde::{Deserialize, Serialize};

use super::Engine;
use super::shared::Finding;
use crate::error::Result;
use crate::recovery::Level;

/// The git subcommands the gate looks at.
pub const GATED: &[&str] = &["commit", "push", "pull", "merge", "rebase"];
/// The prefix that carries the developer's override.
pub const OVERRIDE: &str = "RESONANCE_OVERRIDE=1";

/// The gate's decision.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GateDecision {
    /// `allow` or `deny`.
    pub decision: String,
    pub reason: String,
    /// The gated git subcommands found in the command.
    pub git: Vec<String>,
    pub findings: Vec<Finding>,
    /// The developer overrode a denial.
    pub overridden: bool,
}

/// The git subcommands in a shell command line, and whether it carries the override.
pub fn git_subcommands(command: &str) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let mut overridden = false;
    let normalized = command
        .replace("&&", "\n")
        .replace("||", "\n")
        .replace([';', '|', '(', ')'], "\n");
    for segment in normalized.lines() {
        let tokens: Vec<String> = segment
            .split_whitespace()
            .map(|t| t.trim_matches(|c| c == '"' || c == '\'').to_string())
            .collect();
        let mut i = 0;
        // Leading environment assignments and wrappers.
        while i < tokens.len() {
            let t = &tokens[i];
            let assignment = t.contains('=') && !t.starts_with('-') && !t.starts_with('=');
            let wrapper = matches!(
                t.as_str(),
                "env" | "command" | "exec" | "sudo" | "time" | "nohup"
            );
            if !assignment && !wrapper {
                break;
            }
            overridden |= t == OVERRIDE;
            i += 1;
        }
        let Some(program) = tokens.get(i) else {
            continue;
        };
        if program != "git" && !program.ends_with("/git") {
            continue;
        }
        i += 1;
        // Global options before the subcommand.
        while i < tokens.len() {
            let t = tokens[i].as_str();
            if matches!(t, "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace") {
                i += 2;
            } else if t.starts_with('-') {
                i += 1;
            } else {
                break;
            }
        }
        if let Some(sub) = tokens.get(i)
            && GATED.contains(&sub.as_str())
            && !out.contains(sub)
        {
            out.push(sub.clone());
        }
    }
    (out, overridden)
}

impl Engine {
    /// `resonance agent-hook pre-git` (FR-222): decide whether the agent's git command runs.
    pub fn agent_gate(&mut self, command: &str) -> Result<GateDecision> {
        let (git, overridden) = git_subcommands(command);
        let mut d = GateDecision {
            decision: "allow".into(),
            git: git.clone(),
            ..Default::default()
        };
        if git.is_empty() {
            return Ok(d);
        }
        let mut blocking: Vec<Finding> = Vec::new();
        // Action-required divergences stop every gated command (FR-215, FR-217).
        let status = self.status()?;
        let action: Vec<String> = status
            .divergences
            .iter()
            .filter(|x| x.level == Level::ActionRequired)
            .map(|x| x.summary.clone())
            .collect();
        if !action.is_empty() {
            blocking.push(
                Finding::new(
                    "R008",
                    "error",
                    format!(
                        "{} divergence(s) need a decision before git operations",
                        action.len()
                    ),
                    "resolve them with `resonance recover` (ask the developer when unsure)",
                )
                .with_paths(action),
            );
        }
        // The change log is empty before commits, pushes and pulls (FR-277).
        let pending = crate::changes::pending_count(self.reader())?;
        if pending > 0
            && git
                .iter()
                .any(|g| matches!(g.as_str(), "commit" | "push" | "pull"))
        {
            blocking.push(Finding::new(
                "R009",
                "error",
                format!("{pending} change-log entr(ies) are pending"),
                "read them (resonance_changes), act on each, and clear them with resonance_changes_done",
            ));
        }
        if self.mode.opted_in() {
            if git.iter().any(|g| g == "commit") {
                let report = self.check_staged(true)?;
                blocking.extend(
                    report
                        .findings
                        .into_iter()
                        .filter(|f| f.severity == "error"),
                );
            }
            if git.iter().any(|g| g == "push")
                && let Some(range) = self.push_range()
            {
                let report = self.check_range(&range)?;
                blocking.extend(
                    report
                        .findings
                        .into_iter()
                        .filter(|f| f.severity == "error"),
                );
            }
        }
        // Keep one finding per code.
        let mut seen = std::collections::BTreeSet::new();
        blocking.retain(|f| seen.insert(f.code.clone()));
        let commit = git.iter().any(|g| g == "commit");
        if blocking.is_empty() {
            if commit {
                let first = !self.gate_flag()?;
                self.count(&[("agent.commits", 1), ("agent.firstPass", u64::from(first))])?;
                self.set_gate_flag(false)?;
            }
            return Ok(d);
        }
        d.findings = blocking;
        if overridden {
            d.overridden = true;
            d.reason = "the developer overrode the gate".into();
            self.count(&[("agent.overrides", 1), ("agent.commits", u64::from(commit))])?;
            if commit {
                self.set_gate_flag(false)?;
            }
            return Ok(d);
        }
        if commit {
            self.set_gate_flag(true)?;
        }
        self.count(&[("agent.denials", 1)])?;
        d.decision = "deny".into();
        let mut reason = format!(
            "Resonance stopped `git {}`: fix these first, then retry.",
            git.join("`, `git ")
        );
        for f in &d.findings {
            reason.push_str(&format!("\n- {} {}; fix: {}", f.code, f.message, f.fix));
            for p in f.paths.iter().take(5) {
                reason.push_str(&format!("\n    {p}"));
            }
        }
        reason.push_str(&format!(
            "\nOnly if the developer explicitly tells you to proceed anyway, prefix the command with `{OVERRIDE}`."
        ));
        d.reason = reason;
        Ok(d)
    }

    /// The range a push sends: from the upstream (or the trunk on the remote) to `HEAD`.
    fn push_range(&self) -> Option<String> {
        if let Ok(u) = self
            .repo
            .git(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
            && !u.is_empty()
        {
            return Some(format!("{u}..HEAD"));
        }
        let trunk = crate::history::compact::trunk(&self.repo, &self.config);
        let remote = format!("origin/{trunk}");
        self.repo
            .git(&["rev-parse", "--verify", "--quiet", &remote])
            .ok()
            .map(|_| format!("{remote}..HEAD"))
    }

    /// Whether the last commit gate denied (so the next pass is not a first attempt).
    fn gate_flag(&self) -> Result<bool> {
        Ok(self.reader().get_meta("agent.commitDenied")?.is_some())
    }

    fn set_gate_flag(&mut self, on: bool) -> Result<()> {
        self.store
            .write(crate::store::GraphScope::Primary, &mut |w| {
                if on {
                    w.set_meta("agent.commitDenied", "1")
                } else {
                    w.delete_meta("agent.commitDenied")
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_gated_git_subcommands() {
        let (g, o) = git_subcommands("git add -A && git commit -m 'x; y'");
        assert_eq!(g, ["commit"]);
        assert!(!o);
        let (g, _) = git_subcommands("cd sub; git -C .. -c a=b push origin main");
        assert_eq!(g, ["push"]);
        let (g, o) = git_subcommands("RESONANCE_OVERRIDE=1 git pull --rebase");
        assert_eq!(g, ["pull"]);
        assert!(o);
        let (g, _) = git_subcommands("git status && git log | head");
        assert!(g.is_empty());
        let (g, _) = git_subcommands("echo git commit");
        assert!(g.is_empty());
        let (g, _) = git_subcommands("/usr/bin/git merge feature\ngit rebase main");
        assert_eq!(g, ["merge", "rebase"]);
    }
}
