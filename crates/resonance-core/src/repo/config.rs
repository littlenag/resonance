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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct IngestConfig {
    pub ignore: Vec<String>,
    pub max_blob_bytes: u64,
}

impl Default for IngestConfig {
    fn default() -> Self {
        IngestConfig {
            ignore: Vec::new(),
            max_blob_bytes: 5 * 1024 * 1024,
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

/// Set one value in the configuration file, keeping every other line (and comments). `value`
/// is a TOML literal, for example `2` or `"0.1.0"`.
pub fn set_value(path: &Path, table: &str, key: &str, value: &str) -> Result<()> {
    let text = fs::read_to_string(path).unwrap_or_else(|_| DEFAULT_CONFIG.to_string());
    let rendered = format!("{key} = {value}");
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut done = false;
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with('[') {
            if current == table && !done {
                out.push(rendered.clone());
                done = true;
            }
            current = t.trim_matches(|c| c == '[' || c == ']').trim().to_string();
        }
        let is_key = t.split_once('=').is_some_and(|(k, _)| k.trim() == key);
        if current == table && !done && is_key {
            out.push(rendered.clone());
            done = true;
            continue;
        }
        out.push(line.to_string());
        if i + 1 == lines.len() && current == table && !done {
            out.push(rendered.clone());
            done = true;
        }
    }
    if !done {
        out.push(String::new());
        out.push(format!("[{table}]"));
        out.push(rendered);
    }
    let mut next = out.join("\n");
    next.push('\n');
    toml::from_str::<Config>(&next)
        .map_err(|e| Error::Config(format!("could not update {}: {e}", path.display())))?;
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
}
