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
    pub ui: UiConfig,
    pub log: LogConfig,
    pub diagnostics: DiagnosticsConfig,
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

/// `[ui]`: the web UI each daemon serves (MVP-043).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiConfig {
    pub enabled: bool,
    /// A port to prefer; 0 lets the daemon choose (FR-286).
    pub port: u16,
}

impl Default for UiConfig {
    fn default() -> Self {
        UiConfig {
            enabled: true,
            port: 0,
        }
    }
}

/// `[log]`: the daemon's log (FR-297, MVP-044).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LogConfig {
    /// The log file; empty means `.resonance/local/daemon.log`.
    pub file: String,
    /// `error`, `warn`, `info`, `debug` or `trace`, optionally with per-component directives
    /// such as `info,ingest=debug`.
    pub level: String,
    /// `text` or `json`.
    pub format: String,
    /// The size past which the file moves to `<file>.1` and a new one starts.
    pub max_bytes: u64,
    /// Records kept in memory for the web UI.
    pub buffer_lines: usize,
}

impl Default for LogConfig {
    fn default() -> Self {
        LogConfig {
            file: String::new(),
            level: "info".into(),
            format: "text".into(),
            max_bytes: 1 << 20,
            buffer_lines: 5000,
        }
    }
}

/// `[diagnostics]`: what the daemon remembers of its own activity (FR-294, MVP-044).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DiagnosticsConfig {
    /// Events kept in memory; the oldest are dropped first.
    pub buffer_events: usize,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        DiagnosticsConfig {
            buffer_events: 10_000,
        }
    }
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
        if let Some(v) = get("RESONANCE_LOG").filter(|v| !v.trim().is_empty()) {
            self.log.level = v.trim().to_string();
        }
        if let Some(v) = get("RESONANCE_LOG_FILE").filter(|v| !v.trim().is_empty()) {
            self.log.file = v.trim().to_string();
        }
        Ok(())
    }

    /// Every setting in effect, with where its value came from (FR-293): `default`, `file`
    /// or `env`.
    pub fn sources(path: &Path, get: impl Fn(&str) -> Option<String>) -> Result<Vec<Source>> {
        let text = fs::read_to_string(path).unwrap_or_default();
        let file_value: toml::Value =
            toml::from_str(&text).unwrap_or(toml::Value::Table(Default::default()));
        let from_file: Config = toml::from_str(&text).unwrap_or_default();
        let mut effective = from_file.clone();
        effective.apply_env(get)?;
        let mut file_flat = Vec::new();
        flatten("", &serde_json::to_value(&from_file)?, &mut file_flat);
        let mut flat = Vec::new();
        flatten("", &serde_json::to_value(&effective)?, &mut flat);
        Ok(flat
            .into_iter()
            .map(|(key, value)| {
                let in_file = key
                    .split('.')
                    .try_fold(&file_value, |v, part| v.get(part))
                    .is_some();
                let from_env = file_flat
                    .iter()
                    .find(|(k, _)| *k == key)
                    .is_some_and(|(_, v)| *v != value);
                let source = if from_env {
                    "env"
                } else if in_file {
                    "file"
                } else {
                    "default"
                };
                Source {
                    key,
                    value,
                    source: source.into(),
                }
            })
            .collect())
    }
}

/// One setting and where its value came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub key: String,
    pub value: serde_json::Value,
    pub source: String,
}

fn flatten(prefix: &str, v: &serde_json::Value, out: &mut Vec<(String, serde_json::Value)>) {
    match v {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, out);
            }
        }
        other => out.push((prefix.to_string(), other.clone())),
    }
}

/// Notes for configuration this build no longer reads (the removed document importer).
pub fn legacy_config_notes(text: &str) -> Vec<String> {
    let Ok(value) = toml::from_str::<toml::Value>(text) else {
        return Vec::new();
    };
    let mut notes = Vec::new();
    if value.get("intent").and_then(|t| t.get("sources")).is_some() {
        notes.push(
            "`.resonance/config.toml` still has `intent.sources`, which Resonance no longer reads. Re-import those documents with `resonance_import_items`."
                .into(),
        );
    }
    if value
        .get("ingest")
        .is_some_and(|t| t.get("sourceTags").is_some() || t.get("source_tags").is_some())
    {
        notes.push(
            "`.resonance/config.toml` still has `ingest.sourceTags`, which Resonance no longer reads. Re-import those documents with `resonance_import_items`."
                .into(),
        );
    }
    notes
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

    #[test]
    fn sources_name_file_env_and_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[daemon]\nidleSeconds = 60\n").unwrap();
        let s = Config::sources(&path, |k| (k == "RESONANCE_LOG").then(|| "debug".into())).unwrap();
        let of = |key: &str| s.iter().find(|x| x.key == key).unwrap().clone();
        assert_eq!(of("daemon.idleSeconds").source, "file");
        assert_eq!(of("log.level").source, "env");
        assert_eq!(of("log.level").value, serde_json::json!("debug"));
        assert_eq!(of("ui.enabled").source, "default");
    }
}
