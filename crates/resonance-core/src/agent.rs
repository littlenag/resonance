//! The agent kit (FR-210–FR-214, MVP-038): `agent install`, `agent status`, `agent update`.
//!
//! For Claude Code the kit is: the MCP server registered at local scope; a user-level skill
//! (the static part); harness hooks and the `RESONANCE_ACTOR` setting in
//! `.claude/settings.local.json`; and an instructions section, in `CLAUDE.local.md` offline or,
//! from stage 2 and with confirmation, in the shared `CLAUDE.md` or `AGENTS.md`. The dynamic
//! part is served by the MCP adapter: its `instructions` and `resonance_conventions`, specific to
//! the stage, the participation and the binary's version. The installed kit is recorded in
//! `.resonance/local/agent-kit.json`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::VERSION;
use crate::error::{Error, Result};
use crate::history::Mode;
use crate::repo::{Repo, exclude};

/// The part of the agent instructions common to every mode (MVP-014, FR-212).
pub const INSTRUCTIONS: &str = include_str!("../assets/agent-instructions.md");
const OFFLINE: &str = include_str!("../assets/agent-offline.md");
const SHARED: &str = include_str!("../assets/agent-shared.md");
const HOLDOUT: &str = include_str!("../assets/agent-holdout.md");
/// The skill: the kit's static part (MVP-038).
pub const SKILL: &str = include_str!("../assets/agent-skill.md");
/// The instructions section for `CLAUDE.local.md`, `CLAUDE.md` or `AGENTS.md` (FR-213).
pub const SECTION: &str = include_str!("../assets/agent-section.md");
const SECTION_BEGIN: &str = "<!-- >>> resonance >>> -->";
const SECTION_END: &str = "<!-- <<< resonance <<< -->";

/// The environment setting that tells Resonance the agent is acting (MVP-040).
pub const ACTOR_ENV: &str = "RESONANCE_ACTOR";
pub const CLAUDE_ACTOR: &str = "agent:claude-code";
/// What marks the harness hook as Resonance's.
const HOOK_MARK: &str = " agent-hook ";

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
pub fn kit_installed(repo: &Repo) -> bool {
    kit_path(repo).is_file()
}

fn kit_path(repo: &Repo) -> PathBuf {
    repo.local_dir().join("agent-kit.json")
}

/// The dynamic part of the kit: the conventions for this mode (FR-212).
pub fn conventions(mode: Mode) -> String {
    let variant = if mode.opted_in() {
        SHARED
    } else if mode.shared() {
        HOLDOUT
    } else {
        OFFLINE
    };
    let variant = variant
        .replace("{stage}", &mode.stage.to_string())
        .replace("{participation}", mode.participation.as_str());
    format!(
        "{}\n{variant}\n_Resonance {VERSION}._\n",
        INSTRUCTIONS.trim_end()
    )
}

/// The orientation an agent gives at the end of onboarding (FR-225).
pub fn orientation(mode: Mode, kit: &KitStatus) -> Vec<String> {
    let mut out = vec![format!(
        "Resonance {VERSION} is reachable; stage {}, participation {}.",
        mode.stage,
        mode.participation.as_str()
    )];
    if mode.opted_in() {
        out.push("The graph is shared and is this developer's source of truth: I check before every commit (`resonance check --staged`), stage the open delta with the code, keep the change log empty before commits, pushes and pulls, and include the `resonance_review` summary in pull requests.".into());
    } else if mode.shared() {
        out.push("The team shares a graph, but this developer has not opted in: I work normally with git and never touch `.resonance/`.".into());
    } else {
        out.push("Offline: the graph is private to this developer. I keep Resonance invisible in commits and pull requests, and repair the graph after git operations.".into());
        out.push("Offline cannot share links and annotations with teammates or check their commits; `resonance share` does that when the team is ready.".into());
    }
    out.push("I bring your spec documents into the graph when you ask, after showing you a preview, and keep them in step with it afterwards.".into());
    out.push("Before changing code I read why it exists (`resonance_why`, `resonance_context`) and honor its annotations; after writing code and tests I declare, link and test them, so I can show you how what I built traces back to what you asked for.".into());
    if !kit.installed {
        out.push(
            "The agent kit is not installed for this working tree: ask the developer to run `resonance init`."
                .into(),
        );
    } else if kit.stale {
        out.push(format!(
            "The agent kit is from Resonance {}; running `resonance init` again refreshes it.",
            kit.version.as_deref().unwrap_or("?")
        ));
    }
    out
}

/// The installed kit (`local/agent-kit.json`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KitRecord {
    pub version: String,
    pub installed_at: String,
    /// `claude-code` or `generic`.
    pub harness: String,
    pub stage: u8,
    pub participation: String,
    /// Files the kit wrote, for `leave` and `agent update`.
    pub files: Vec<String>,
}

/// `agent status`, and the kit part of `status` and `doctor`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KitStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub current: String,
    /// Installed by another version, or for another stage or participation.
    pub stale: bool,
    pub harness: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

pub fn kit_record(repo: &Repo) -> Option<KitRecord> {
    fs::read_to_string(kit_path(repo))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
}

pub fn kit_status(repo: &Repo, mode: Mode) -> KitStatus {
    match kit_record(repo) {
        None => KitStatus {
            current: VERSION.to_string(),
            ..Default::default()
        },
        Some(r) => KitStatus {
            installed: true,
            stale: r.version != VERSION
                || r.stage != mode.stage
                || r.participation != mode.participation.as_str(),
            version: Some(r.version),
            current: VERSION.to_string(),
            harness: Some(r.harness),
            files: r.files,
        },
    }
}

/// How to install the kit.
#[derive(Debug, Clone)]
pub struct KitOptions {
    pub exe: PathBuf,
    pub home: PathBuf,
    /// Register the MCP server with `claude mcp add` (when Claude Code is installed).
    pub register_mcp: bool,
    /// Other agents: an `AGENTS.md` section and the MCP command line.
    pub generic: bool,
    pub dry_run: bool,
}

/// One thing the kit wrote, or would write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KitStep {
    pub what: String,
    pub path: String,
    /// `created`, `updated`, `unchanged`, `removed`, `excluded`, or `would …` in a dry run.
    pub action: String,
}

/// What `agent install` did.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KitReport {
    pub harness: String,
    pub version: String,
    pub stage: u8,
    pub participation: String,
    pub dry_run: bool,
    pub steps: Vec<KitStep>,
    pub mcp: Option<AgentInstall>,
    /// For `--generic`: the section to add to `AGENTS.md`, and the MCP command line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_command: Option<String>,
}

fn rel(repo: &Repo, p: &Path) -> String {
    p.strip_prefix(&repo.root)
        .map(|r| r.display().to_string())
        .unwrap_or_else(|_| p.display().to_string())
}

/// Write `text` to `path` unless it is already there; returns the action.
fn put(path: &Path, text: &str, dry_run: bool) -> Result<String> {
    let current = fs::read_to_string(path).ok();
    let action = match &current {
        None => "created",
        Some(c) if c == text => return Ok("unchanged".into()),
        Some(_) => "updated",
    };
    if dry_run {
        return Ok(format!("would be {action}"));
    }
    if let Some(d) = path.parent() {
        fs::create_dir_all(d).map_err(|e| Error::io(format!("creating {}", d.display()), e))?;
    }
    fs::write(path, text).map_err(|e| Error::io(format!("writing {}", path.display()), e))?;
    Ok(action.into())
}

/// `text` with Resonance's marked section replaced by `section` (or removed, when empty).
fn with_section(text: &str, section: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    let mut placed = false;
    for line in text.lines() {
        if line.trim() == SECTION_BEGIN {
            inside = true;
            if !section.is_empty() {
                out.push_str(section);
                placed = true;
            }
            continue;
        }
        if line.trim() == SECTION_END {
            inside = false;
            continue;
        }
        if !inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !placed && !section.is_empty() {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(section);
    }
    out
}

/// The harness hook command.
fn hook_command(exe: &Path, event: &str) -> String {
    format!("\"{}\"{HOOK_MARK}{event}", exe.display())
}

/// The harness hooks the kit installs: the gate on the agent's git commands (FR-222), and the
/// pending change log put in front of the agent with every prompt (FR-283).
fn kit_hooks(exe: &Path) -> Vec<(&'static str, Value)> {
    vec![
        (
            "PreToolUse",
            json!({"matcher": "Bash", "hooks": [{"type": "command", "command": hook_command(exe, "pre-git")}]}),
        ),
        (
            "UserPromptSubmit",
            json!({"hooks": [{"type": "command", "command": hook_command(exe, "prompt")}]}),
        ),
    ]
}

/// `settings` with Resonance's harness hooks and actor setting (or without them).
fn with_settings(mut settings: Value, exe: Option<&Path>) -> Value {
    if !settings.is_object() {
        settings = json!({});
    }
    let obj = settings.as_object_mut().expect("an object");
    // Drop Resonance's own hook entries from every event, then add the current ones.
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    if let Some(h) = hooks.as_object_mut() {
        for list in h.values_mut() {
            if let Some(list) = list.as_array_mut() {
                list.retain(|entry| {
                    !entry["hooks"].as_array().is_some_and(|hs| {
                        hs.iter()
                            .any(|x| x["command"].as_str().is_some_and(|c| c.contains(HOOK_MARK)))
                    })
                });
            }
        }
        if let Some(exe) = exe {
            for (event, entry) in kit_hooks(exe) {
                if let Some(list) = h.entry(event).or_insert_with(|| json!([])).as_array_mut() {
                    list.push(entry);
                }
            }
        }
        h.retain(|_, v| v.as_array().is_none_or(|l| !l.is_empty()));
        if h.is_empty() {
            obj.remove("hooks");
        }
    }
    let env = obj.entry("env").or_insert_with(|| json!({}));
    if let Some(e) = env.as_object_mut() {
        if exe.is_some() {
            e.insert(ACTOR_ENV.into(), json!(CLAUDE_ACTOR));
        } else if e.get(ACTOR_ENV).and_then(Value::as_str) == Some(CLAUDE_ACTOR) {
            e.remove(ACTOR_ENV);
        }
        if e.is_empty() {
            obj.remove("env");
        }
    }
    settings
}

fn pretty(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.push('\n');
    s
}

/// Make sure a local-only file is not picked up by git (MVP-038).
fn keep_local(repo: &Repo, rel_path: &str, dry_run: bool, steps: &mut Vec<KitStep>) -> Result<()> {
    let ignored = Command::new("git")
        .args(["check-ignore", "-q", rel_path])
        .current_dir(&repo.root)
        .status()
        .is_ok_and(|s| s.success());
    if ignored {
        return Ok(());
    }
    let line = format!("/{rel_path}");
    if !dry_run && !exclude::add_line(repo, &line)? {
        return Ok(());
    }
    steps.push(KitStep {
        what: format!("git exclusion for {rel_path}"),
        path: ".git/info/exclude".into(),
        action: if dry_run {
            "would be updated".into()
        } else {
            "updated".into()
        },
    });
    Ok(())
}

/// The shared instructions file (FR-213): `AGENTS.md` when the repository uses it and has no
/// `CLAUDE.md`, else `CLAUDE.md`.
pub fn shared_section_path(repo: &Repo) -> PathBuf {
    let claude = repo.root.join("CLAUDE.md");
    let agents = repo.root.join("AGENTS.md");
    if agents.is_file() && !claude.is_file() {
        agents
    } else {
        claude
    }
}

/// Put the kit's section in the shared instructions file (`share`, from stage 2, after its
/// preview, MVP-037). The section is worded for every kind of participation.
pub fn write_shared_section(repo: &Repo, dry_run: bool) -> Result<KitStep> {
    let path = shared_section_path(repo);
    let text = with_section(&fs::read_to_string(&path).unwrap_or_default(), SECTION);
    Ok(KitStep {
        what: "shared instructions section".into(),
        path: rel(repo, &path),
        action: put(&path, &text, dry_run)?,
    })
}

/// Install (or refresh) the agent kit (MVP-038).
pub fn install_kit(repo: &Repo, mode: Mode, opts: &KitOptions) -> Result<KitReport> {
    let mut report = KitReport {
        harness: if opts.generic {
            "generic"
        } else {
            "claude-code"
        }
        .into(),
        version: VERSION.into(),
        stage: mode.stage,
        participation: mode.participation.as_str().into(),
        dry_run: opts.dry_run,
        ..Default::default()
    };
    let mut files = Vec::new();
    if opts.generic {
        let mcp = format!(
            "{} mcp",
            render_command(&opts.exe.display().to_string(), &[])
        );
        report.section = Some(SECTION.to_string());
        report.mcp_command = Some(mcp);
    } else {
        // 1. The MCP server, at local scope.
        report.mcp = Some(install(
            &repo.root,
            &opts.exe,
            opts.register_mcp && !opts.dry_run,
        )?);
        // 2. The skill.
        let skill = opts.home.join(".claude/skills/resonance/SKILL.md");
        report.steps.push(KitStep {
            what: "skill".into(),
            path: skill.display().to_string(),
            action: put(&skill, SKILL, opts.dry_run)?,
        });
        // 3. Harness hooks and the actor setting.
        let settings_path = repo.root.join(".claude/settings.local.json");
        let current: Value = fs::read_to_string(&settings_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_else(|| json!({}));
        let next = pretty(&with_settings(current, Some(&opts.exe)));
        report.steps.push(KitStep {
            what: "harness hooks and RESONANCE_ACTOR".into(),
            path: rel(repo, &settings_path),
            action: put(&settings_path, &next, opts.dry_run)?,
        });
        files.push(rel(repo, &settings_path));
        keep_local(
            repo,
            ".claude/settings.local.json",
            opts.dry_run,
            &mut report.steps,
        )?;
        // 4. The instructions section, local to this developer.
        let path = repo.root.join("CLAUDE.local.md");
        let text = with_section(&fs::read_to_string(&path).unwrap_or_default(), SECTION);
        report.steps.push(KitStep {
            what: "instructions section".into(),
            path: rel(repo, &path),
            action: put(&path, &text, opts.dry_run)?,
        });
        files.push(rel(repo, &path));
        keep_local(repo, "CLAUDE.local.md", opts.dry_run, &mut report.steps)?;
    }
    if !opts.dry_run {
        let record = KitRecord {
            version: VERSION.into(),
            installed_at: crate::history::now(),
            harness: report.harness.clone(),
            stage: mode.stage,
            participation: mode.participation.as_str().into(),
            files,
        };
        fs::create_dir_all(repo.local_dir())
            .map_err(|e| Error::io("creating .resonance/local", e))?;
        fs::write(kit_path(repo), pretty(&serde_json::to_value(&record)?))
            .map_err(|e| Error::io("writing the agent kit record", e))?;
    }
    Ok(report)
}

/// Remove the kit's working-tree parts (`leave`, MVP-037): the harness hook and actor setting,
/// the instructions section in local files, and the record. The user-level skill stays: it
/// serves every repository.
pub fn uninstall_kit(repo: &Repo) -> Result<Vec<KitStep>> {
    let mut steps = Vec::new();
    let settings_path = repo.root.join(".claude/settings.local.json");
    if let Some(current) = fs::read_to_string(&settings_path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
    {
        let next = with_settings(current.clone(), None);
        if next != current {
            if next.as_object().is_some_and(|o| o.is_empty()) {
                let _ = fs::remove_file(&settings_path);
            } else {
                fs::write(&settings_path, pretty(&next))
                    .map_err(|e| Error::io("writing .claude/settings.local.json", e))?;
            }
            steps.push(KitStep {
                what: "harness hook and RESONANCE_ACTOR".into(),
                path: rel(repo, &settings_path),
                action: "removed".into(),
            });
        }
    }
    let local = repo.root.join("CLAUDE.local.md");
    if let Ok(text) = fs::read_to_string(&local)
        && text.contains(SECTION_BEGIN)
    {
        let next = with_section(&text, "");
        if next.trim().is_empty() {
            let _ = fs::remove_file(&local);
        } else {
            fs::write(&local, next).map_err(|e| Error::io("writing CLAUDE.local.md", e))?;
        }
        steps.push(KitStep {
            what: "instructions section".into(),
            path: "CLAUDE.local.md".into(),
            action: "removed".into(),
        });
    }
    if kit_path(repo).is_file() {
        let _ = fs::remove_file(kit_path(repo));
    }
    // Exclusions the kit added for files that are now gone.
    for f in [".claude/settings.local.json", "CLAUDE.local.md"] {
        if !repo.root.join(f).exists() {
            exclude::remove_line(repo, &format!("/{f}"))?;
        }
    }
    Ok(steps)
}

/// The conventions, orientation and kit state, for `resonance_conventions`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConventionsView {
    pub version: String,
    pub stage: u8,
    pub participation: String,
    pub opted_in: bool,
    pub kit: KitStatus,
    pub orientation: Vec<String>,
    pub conventions: String,
}

pub fn conventions_view(repo: &Repo, mode: Mode) -> ConventionsView {
    let kit = kit_status(repo, mode);
    ConventionsView {
        version: VERSION.into(),
        stage: mode.stage,
        participation: mode.participation.as_str().into(),
        opted_in: mode.opted_in(),
        orientation: orientation(mode, &kit),
        kit,
        conventions: conventions(mode),
    }
}
