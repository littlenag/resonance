//! Importers for existing specification formats (FR-122): Markdown with requirement IDs
//! (this spec's conventions, and GitHub Spec Kit's `**FR-001**: text`), Kiro
//! `requirements.md` and `design.md`, EARS-style statements, and ADR directories.
//!
//! Each importer recognizes its tool's documented default layout. Items without a stable
//! identifier in the source are reported, never imported under guessed keys.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::markdown;
use crate::model::NodeType;
use crate::model::ids::is_requirement_key;

/// A source format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    /// Requirement IDs in lists and tables (MVP-011), including Spec Kit's colon form.
    Markdown,
    /// Kiro `requirements.md`: `### Requirement N` sections.
    Kiro,
    /// Kiro `design.md`: one design element per `##` section.
    KiroDesign,
    /// EARS-style statements: `KEY: When …, the system shall …`.
    Ears,
    /// A directory of architecture decision records, `NNNN-title.md`.
    Adr,
}

impl Format {
    pub fn parse(s: &str) -> Option<Format> {
        Some(match s {
            "markdown" | "md" | "speckit" | "spec-kit" => Format::Markdown,
            "kiro" => Format::Kiro,
            "kiro-design" => Format::KiroDesign,
            "ears" => Format::Ears,
            "adr" => Format::Adr,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Format::Markdown => "markdown",
            Format::Kiro => "kiro",
            Format::KiroDesign => "kiro-design",
            Format::Ears => "ears",
            Format::Adr => "adr",
        }
    }
}

/// A configured source: `path`, or `format:path` when the format is given explicitly.
pub fn split_source(entry: &str) -> (Option<Format>, &str) {
    if let Some((f, p)) = entry.split_once(':')
        && let Some(format) = Format::parse(f)
    {
        return (Some(format), p);
    }
    (None, entry)
}

/// The format of a source, from its path (FR-122).
pub fn detect(path: &str, is_dir: bool) -> Format {
    let lower = path.to_lowercase();
    if is_dir {
        return Format::Adr;
    }
    if lower.contains(".kiro/specs/") && lower.ends_with("/requirements.md") {
        return Format::Kiro;
    }
    if lower.contains(".kiro/specs/") && lower.ends_with("/design.md") {
        return Format::KiroDesign;
    }
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if name.contains("ears") {
        return Format::Ears;
    }
    Format::Markdown
}

/// One item found in a source.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub ntype: NodeType,
    pub key: String,
    /// Properties to set: `text` for requirements; `title`, `decision`, `context`,
    /// `consequences`, `status` for decisions; `title`, `text` for designs.
    pub fields: serde_json::Map<String, Value>,
    pub spec_status: Option<String>,
    pub path: String,
    pub line: usize,
}

/// A statement that looks like intent but has no stable identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unidentified {
    pub path: String,
    pub line: usize,
    pub text: String,
}

/// What reading one source found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parsed {
    pub items: Vec<Item>,
    pub unidentified: Vec<Unidentified>,
    /// The source's content fingerprint input (file contents, or a directory's files).
    pub fingerprint: String,
}

fn requirement(key: &str, text: &str, status: Option<String>, path: &str, line: usize) -> Item {
    let mut fields = serde_json::Map::new();
    fields.insert("text".into(), json!(text));
    Item {
        ntype: NodeType::Requirement,
        key: key.to_string(),
        fields,
        spec_status: status,
        path: path.to_string(),
        line,
    }
}

/// Parse one text source.
pub fn parse_text(format: Format, path: &str, text: &str) -> Parsed {
    let mut p = Parsed::default();
    match format {
        Format::Markdown => {
            for r in markdown::parse(text) {
                p.items
                    .push(requirement(&r.key, &r.text, r.spec_status, path, r.line));
            }
        }
        Format::Kiro => parse_kiro(path, text, &mut p),
        Format::KiroDesign => parse_kiro_design(path, text, &mut p),
        Format::Ears => parse_ears(path, text, &mut p),
        Format::Adr => {}
    }
    p
}

/// The key prefix of a Kiro spec: the letters of its directory name, upper-cased.
fn kiro_prefix(path: &str) -> String {
    let dir = path
        .rsplit('/')
        .nth(1)
        .unwrap_or("kiro")
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_uppercase();
    if dir.is_empty() { "KIRO".into() } else { dir }
}

fn parse_kiro(path: &str, text: &str, p: &mut Parsed) {
    let prefix = kiro_prefix(path);
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        let number = t
            .strip_prefix("### Requirement")
            .map(str::trim)
            .and_then(|n| n.split(|c: char| !c.is_ascii_digit()).next())
            .filter(|n| !n.is_empty());
        if let Some(n) = number {
            let start = i;
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() {
                let l = lines[i].trim();
                if l.starts_with("### ") || l.starts_with("## ") {
                    break;
                }
                if !l.is_empty() {
                    body.push(l.to_string());
                }
                i += 1;
            }
            let key = format!("{prefix}-{}", n.trim_start_matches('0').max("0"));
            p.items
                .push(requirement(&key, &body.join("\n"), None, path, start + 1));
            continue;
        }
        i += 1;
    }
}

fn parse_kiro_design(path: &str, text: &str, p: &mut Parsed) {
    let prefix = kiro_prefix(path);
    let lines: Vec<&str> = text.lines().collect();
    let mut n = 0;
    let mut i = 0;
    while i < lines.len() {
        if let Some(title) = lines[i].trim().strip_prefix("## ") {
            let start = i;
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("## ") {
                body.push(lines[i]);
                i += 1;
            }
            n += 1;
            let mut fields = serde_json::Map::new();
            fields.insert("title".into(), json!(title.trim()));
            let text: String = body.join("\n").trim().chars().take(4000).collect();
            fields.insert("text".into(), json!(text));
            p.items.push(Item {
                ntype: NodeType::Design,
                key: format!("{prefix}-DESIGN-{n}"),
                fields,
                spec_status: None,
                path: path.to_string(),
                line: start + 1,
            });
            continue;
        }
        i += 1;
    }
}

/// An EARS-style statement: one with "shall", starting with one of the EARS keywords.
fn is_ears(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.contains(" shall ")
        && ["when ", "while ", "where ", "if ", "the "]
            .iter()
            .any(|k| lower.starts_with(k))
}

fn parse_ears(path: &str, text: &str, p: &mut Parsed) {
    for (i, raw) in text.lines().enumerate() {
        let mut t = raw.trim();
        for m in ["- ", "* ", "+ "] {
            if let Some(rest) = t.strip_prefix(m) {
                t = rest.trim_start();
            }
        }
        // `**KEY**: …`, `KEY: …` or `KEY …`.
        let (key, rest) = if let Some(inner) = t.strip_prefix("**") {
            match inner.split_once("**") {
                Some((k, r)) => (Some(k), r),
                None => (None, t),
            }
        } else {
            match t.split_once(|c: char| c == ':' || c.is_whitespace()) {
                Some((k, r)) if is_requirement_key(k) => (Some(k), r),
                _ => (None, t),
            }
        };
        let rest = rest.trim_start_matches(':').trim();
        match key.filter(|k| is_requirement_key(k)) {
            Some(k) if !rest.is_empty() => {
                let (text, status) = markdown::split_status(rest);
                p.items.push(requirement(k, &text, status, path, i + 1));
            }
            _ if is_ears(t) => p.unidentified.push(Unidentified {
                path: path.to_string(),
                line: i + 1,
                text: t.chars().take(120).collect(),
            }),
            _ => {}
        }
    }
}

/// The sections of an ADR: lower-cased heading → body.
fn sections(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix("## ") {
            out.push((h.trim().to_lowercase(), String::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push_str(line);
            last.1.push('\n');
        }
    }
    for s in &mut out {
        s.1 = s.1.trim().to_string();
    }
    out
}

/// Parse a directory of ADRs: every `NNNN-title.md` becomes a decision keyed `ADR-NNNN`.
pub fn parse_adr_dir(root: &Path, dir: &str) -> Parsed {
    let mut p = Parsed::default();
    let abs = crate::repo::abs_path(root, dir);
    let mut files: Vec<String> = fs::read_dir(&abs)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n.ends_with(".md"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    for name in files {
        let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rel = format!("{}/{name}", dir.trim_end_matches('/'));
        let Ok(text) = fs::read_to_string(abs.join(&name)) else {
            continue;
        };
        p.fingerprint.push_str(&format!(
            "{rel}={};",
            crate::model::content_hash(text.as_bytes())
        ));
        if digits.is_empty() {
            continue; // A README or template, not a decision record.
        }
        let number: u64 = digits.parse().unwrap_or(0);
        let mut title = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("# "))
            .unwrap_or(&name)
            .trim()
            .to_string();
        // "# 3. Use Postgres" or "# 0003 Use Postgres".
        let stripped = title
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(['.', ':', '-', ' ']);
        if stripped.len() < title.len() && !stripped.is_empty() {
            title = stripped.to_string();
        }
        let secs = sections(&text);
        let get = |name: &str| {
            secs.iter()
                .find(|(h, _)| h.starts_with(name))
                .map(|(_, b)| b.clone())
                .filter(|b| !b.is_empty())
        };
        let status = get("status")
            .and_then(|s| s.lines().next().map(|l| l.trim().to_lowercase()))
            .or_else(|| {
                text.lines().find_map(|l| {
                    let l = l.trim().trim_start_matches(['*', '-', ' ']);
                    l.strip_prefix("Status:")
                        .or_else(|| l.strip_prefix("status:"))
                        .map(|s| s.trim().to_lowercase())
                })
            });
        let mut fields = serde_json::Map::new();
        fields.insert("title".into(), json!(title));
        fields.insert(
            "decision".into(),
            json!(get("decision").unwrap_or_else(|| title.clone())),
        );
        fields.insert("context".into(), json!(get("context")));
        fields.insert("consequences".into(), json!(get("consequences")));
        fields.insert(
            "status".into(),
            json!(status.unwrap_or_else(|| "accepted".into())),
        );
        p.items.push(Item {
            ntype: NodeType::Decision,
            key: format!("ADR-{number:04}"),
            fields,
            spec_status: None,
            path: rel,
            line: 1,
        });
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection() {
        assert_eq!(
            detect(".kiro/specs/user-auth/requirements.md", false),
            Format::Kiro
        );
        assert_eq!(
            detect(".kiro/specs/user-auth/design.md", false),
            Format::KiroDesign
        );
        assert_eq!(detect("docs/adr", true), Format::Adr);
        assert_eq!(detect("spec/ears-requirements.md", false), Format::Ears);
        assert_eq!(detect("specs/001-photos/spec.md", false), Format::Markdown);
        assert_eq!(
            split_source("adr:docs/decisions"),
            (Some(Format::Adr), "docs/decisions")
        );
        assert_eq!(split_source("spec/a.md"), (None, "spec/a.md"));
    }

    #[test]
    fn kiro_requirements() {
        let doc = "# Requirements\n\n## Requirements\n\n### Requirement 1\n\n**User Story:** As a user, I want to sign in.\n\n#### Acceptance Criteria\n\n1. WHEN I submit valid credentials THEN the system SHALL sign me in\n\n### Requirement 2\n\n**User Story:** As an admin, I want audits.\n";
        let p = parse_text(Format::Kiro, ".kiro/specs/user-auth/requirements.md", doc);
        let keys: Vec<&str> = p.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["USERAUTH-1", "USERAUTH-2"]);
        let text = p.items[0].fields["text"].as_str().unwrap();
        assert!(text.contains("As a user") && text.contains("SHALL sign me in"));
    }

    #[test]
    fn ears_statements_need_an_identifier() {
        let doc = "- REQ-1: When the user saves, the editor shall write the file.\n- **REQ-2** While offline, the client shall queue changes.\n- When the disk is full, the system shall warn the user.\nPlain prose.\n";
        let p = parse_text(Format::Ears, "spec/ears.md", doc);
        let keys: Vec<&str> = p.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["REQ-1", "REQ-2"]);
        assert_eq!(p.unidentified.len(), 1);
        assert_eq!(p.unidentified[0].line, 3);
    }
}
