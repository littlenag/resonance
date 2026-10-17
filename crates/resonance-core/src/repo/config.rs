//! `.resonance/config.toml` and configuration precedence: flags, RESONANCE_* env, file, defaults (FR-253).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The configuration file `resonance init` writes.
pub const DEFAULT_CONFIG: &str = include_str!("../../assets/default-config.toml");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
#[derive(Default)]
pub struct Config {
    pub repository: RepositoryConfig,
    pub intent: IntentConfig,
    pub ingest: IngestConfig,
    pub test: TestConfig,
    pub daemon: DaemonConfig,
    pub compaction: CompactionConfig,
    pub governance: GovernanceConfig,
    pub changes: ChangesConfig,
}

/// `[repository]`: committed from stage 2 (MVP-033).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RepositoryConfig {
    /// The adoption stage: 1 offline trial, 2 shared (§8.16).
    pub stage: u8,
    /// The oldest binary version that may write this repository's data (FR-264).
    pub min_version: String,
}

impl Default for RepositoryConfig {
    fn default() -> Self {
        RepositoryConfig {
            stage: 1,
            min_version: String::new(),
        }
    }
}

/// `[compaction]` (FR-154, MVP-034).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CompactionConfig {
    /// Compact when the trunk has this many sealed deltas.
    pub max_deltas: usize,
    /// The trunk branch; empty means the remote's default branch, else `main`, else `master`.
    pub trunk: String,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        CompactionConfig {
            max_deltas: 100,
            trunk: String::new(),
        }
    }
}

/// `[governance]` (FR-178): what Resonance governs; empty means everything.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GovernanceConfig {
    pub paths: Vec<String>,
}

/// `[changes]`: the change log (FR-274, MVP-040).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChangesConfig {
    /// Also log changes made by agents (OQ-27).
    pub include_agents: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DaemonConfig {
    /// Seconds without requests after which the daemon exits (FR-084).
    pub idle_seconds: u64,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        DaemonConfig { idle_seconds: 1800 }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IntentConfig {
    /// Repository-relative paths of imported requirement documents (MVP-011).
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IngestConfig {
    pub ignore: Vec<String>,
    pub max_blob_bytes: u64,
    /// Read `@satisfies` / `@verifies` tags from file text (FR-123).
    pub source_tags: bool,
}

impl Default for IngestConfig {
    fn default() -> Self {
        IngestConfig {
            ignore: Vec::new(),
            max_blob_bytes: 5 * 1024 * 1024,
            source_tags: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TestConfig {
    pub command: String,
    pub zero_tests_pattern: String,
    pub timeout_seconds: u64,
}

impl Default for TestConfig {
    fn default() -> Self {
        TestConfig {
            command: String::new(),
            zero_tests_pattern: "running 0 tests".into(),
            timeout_seconds: 600,
        }
    }
}

impl Config {
    /// Load the file (defaults if it is missing), then apply `RESONANCE_*` environment variables.
    pub fn load(path: &Path) -> Result<Config> {
        let mut config = match fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|e| Error::Config(format!("{} is not valid: {e}", path.display())))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(Error::io(format!("reading {}", path.display()), e)),
        };
        config.apply_env(|k| std::env::var(k).ok())?;
        Ok(config)
    }

    /// Environment overrides, between the file and command-line flags in precedence (FR-253).
    pub fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) -> Result<()> {
        let num = |key: &str, v: String| -> Result<u64> {
            v.trim()
                .parse()
                .map_err(|_| Error::Config(format!("{key} must be a whole number, not `{v}`")))
        };
        if let Some(v) = get("RESONANCE_INGEST_MAX_BLOB_BYTES") {
            self.ingest.max_blob_bytes = num("RESONANCE_INGEST_MAX_BLOB_BYTES", v)?;
        }
        if let Some(v) = get("RESONANCE_INGEST_IGNORE") {
            self.ingest.ignore = v
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
        }
        if let Some(v) = get("RESONANCE_TEST_COMMAND") {
            self.test.command = v;
        }
        if let Some(v) = get("RESONANCE_TEST_ZERO_TESTS_PATTERN") {
            self.test.zero_tests_pattern = v;
        }
        if let Some(v) = get("RESONANCE_TEST_TIMEOUT_SECONDS") {
            self.test.timeout_seconds = num("RESONANCE_TEST_TIMEOUT_SECONDS", v)?;
        }
        if let Some(v) = get("RESONANCE_DAEMON_IDLE_SECONDS") {
            self.daemon.idle_seconds = num("RESONANCE_DAEMON_IDLE_SECONDS", v)?;
        }
        Ok(())
    }
}

/// Rewrite `intent.sources` in the configuration file, keeping every other line (and its
/// comments) as it was.
pub fn write_intent_sources(path: &Path, sources: &[String]) -> Result<()> {
    let text = fs::read_to_string(path).unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    let rendered = format!(
        "sources = [{}]",
        sources
            .iter()
            .map(|s| toml::Value::String(s.clone()).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 2);
    let mut table = String::new();
    let mut replaced = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if table == "intent" && !replaced {
                out.push(rendered.clone());
                replaced = true;
            }
            table = trimmed
                .trim_matches(|c| c == '[' || c == ']')
                .trim()
                .to_string();
        }
        if table == "intent" && !replaced && trimmed.starts_with("sources") {
            // Skip the old value, which may span several lines.
            let mut depth = 0i32;
            loop {
                let l = lines[i];
                depth += l.matches('[').count() as i32 - l.matches(']').count() as i32;
                if depth <= 0 || i + 1 >= lines.len() {
                    break;
                }
                i += 1;
            }
            out.push(rendered.clone());
            replaced = true;
            i += 1;
            continue;
        }
        out.push(line.to_string());
        i += 1;
    }
    if !replaced {
        if table != "intent" {
            out.push(String::new());
            out.push("[intent]".into());
        }
        out.push(rendered);
    }
    let mut next = out.join("\n");
    next.push('\n');
    // Never write a file that no longer parses.
    let parsed: Config = toml::from_str(&next)
        .map_err(|e| Error::Config(format!("could not update {}: {e}", path.display())))?;
    if parsed.intent.sources != sources {
        return Err(Error::Config(format!(
            "could not update intent.sources in {}; edit it by hand",
            path.display()
        )));
    }
    fs::write(path, next).map_err(|e| Error::io(format!("writing {}", path.display()), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_matches_default_struct() {
        let parsed: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn env_overrides_file() {
        let mut c = Config::default();
        c.apply_env(|k| match k {
            "RESONANCE_TEST_TIMEOUT_SECONDS" => Some("7".into()),
            "RESONANCE_INGEST_IGNORE" => Some("a/**, b".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(c.test.timeout_seconds, 7);
        assert_eq!(c.ingest.ignore, ["a/**", "b"]);
        assert!(c.apply_env(|_| Some("x".into())).is_err());
    }

    #[test]
    fn rewriting_sources_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, DEFAULT_CONFIG).unwrap();
        write_intent_sources(&path, &["spec/a.md".into(), "docs/b c.md".into()]).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# Requirement documents imported"));
        assert!(text.contains(r#"sources = ["spec/a.md", "docs/b c.md"]"#));
        assert_eq!(Config::load(&path).unwrap().intent.sources.len(), 2);

        fs::write(
            &path,
            "[intent]\nsources = [\n  \"x.md\",\n]\n[test]\ncommand = \"t\"\n",
        )
        .unwrap();
        write_intent_sources(&path, &["y.md".into()]).unwrap();
        let c = Config::load(&path).unwrap();
        assert_eq!(c.intent.sources, ["y.md"]);
        assert_eq!(c.test.command, "t");
    }
}
