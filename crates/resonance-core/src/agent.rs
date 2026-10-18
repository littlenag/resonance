//! `agent install`: registers the MCP server with the agent harness at local scope (FR-132, FR-210).
//!
//! In offline mode the agent kit goes only into local agent configuration: Claude Code's
//! *local* scope is private to this developer and this project, and writes nothing to
//! tracked files. The kit's content is served as MCP server instructions (MVP-014).

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The agent instructions served in the MCP `initialize` result (MVP-014, FR-212).
pub const INSTRUCTIONS: &str = include_str!("../assets/agent-instructions.md");

/// The name the MCP server is registered under.
pub const SERVER_NAME: &str = "resonance";

/// Find an executable on `PATH`.
pub fn which(program: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|p| p.is_file())
}

/// The arguments of the registration command.
pub fn claude_args(resonance_exe: &Path) -> Vec<String> {
    vec![
        "mcp".into(),
        "add".into(),
        "--scope".into(),
        "local".into(),
        SERVER_NAME.into(),
        "--".into(),
        resonance_exe.display().to_string(),
        "mcp".into(),
    ]
}

/// A shell-quoted rendering of a command, for previews.
pub fn render_command(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().map(|a| {
            if a.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./:=@".contains(c))
            {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', r"'\''"))
            }
        }))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstall {
    /// The command that registers (or would register) the server.
    pub command: String,
    pub claude_found: bool,
    pub ran: bool,
    pub succeeded: bool,
    pub output: String,
    /// What to do by hand when it did not run.
    pub instructions: Option<String>,
}

/// Plan the registration, and run it when `run` is true and Claude Code is installed.
pub fn install(root: &Path, resonance_exe: &Path, run: bool) -> Result<AgentInstall> {
    let args = claude_args(resonance_exe);
    let command = render_command("claude", &args);
    let claude = which("claude");
    let manual = format!(
        "Register the MCP server with your agent by running, in {}:\n  {command}\nOther MCP clients: run `{} mcp` as a stdio server in this directory.",
        root.display(),
        resonance_exe.display()
    );
    let Some(claude) = claude else {
        return Ok(AgentInstall {
            command,
            claude_found: false,
            ran: false,
            succeeded: false,
            output: String::new(),
            instructions: Some(format!(
                "Claude Code (`claude`) was not found on PATH.\n{manual}"
            )),
        });
    };
    if !run {
        return Ok(AgentInstall {
            command,
            claude_found: true,
            ran: false,
            succeeded: false,
            output: String::new(),
            instructions: Some(manual),
        });
    }
    let out = Command::new(&claude)
        .args(&args)
        .current_dir(root)
        .output()
        .map_err(|e| Error::Invalid(format!("could not run {}: {e}", claude.display())))?;
    let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(AgentInstall {
        command,
        claude_found: true,
        ran: true,
        succeeded: out.status.success(),
        output: output.trim().to_string(),
        instructions: (!out.status.success()).then_some(manual),
    })
}

/// Whether the agent kit is installed for this working tree (FR-210).
pub fn kit_installed(repo: &crate::repo::Repo) -> bool {
    repo.local_dir().join("agent-kit.json").is_file()
}
