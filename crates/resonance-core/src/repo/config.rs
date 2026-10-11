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
    pub intent: IntentConfig,
    pub ingest: IngestConfig,
    pub test: TestConfig,
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
