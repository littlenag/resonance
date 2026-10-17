//! The delta file format (FR-075, FR-076, MVP-026).
//!
//! A delta is UTF-8 JSON Lines named `<ulid>.delta`. Line 1 is the header, each following line
//! is one operation in canonical form, and the last line is the file manifest: the content
//! hashes of the files whose changes Resonance observed while the delta was open (FR-064).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::canonical;
use crate::error::{Error, Result};
use crate::ops::{Op, kind};

/// The delta format this build reads and writes.
pub const DELTA_FORMAT: u32 = 1;

/// The file extension of deltas.
pub const EXTENSION: &str = "delta";

/// Operation kinds that never go into a delta: file content travels in git (FR-064), and the
/// manifest records it instead.
pub fn is_file_content_op(op_kind: &str) -> bool {
    matches!(op_kind, kind::FILE_PUT | kind::FILE_DELETE)
}

/// Line 1 of a delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaHeader {
    /// Always `"delta"`.
    pub resonance: String,
    pub format: u32,
    pub id: String,
    pub schema: u32,
    /// Who wrote it (the actor of the working tree), and with which Resonance version.
    pub writer: String,
    pub version: String,
    /// The snapshot epoch it was written against (for information only).
    pub epoch: String,
    /// The branch it was started on (for information only).
    pub branch: String,
}

/// One manifest entry: a file's content hash before and after the delta (`None` when the
/// file did not exist).
pub type ManifestEntry = (Option<String>, Option<String>);

/// A parsed delta.
#[derive(Debug, Clone, PartialEq)]
pub struct Delta {
    pub header: DeltaHeader,
    pub ops: Vec<Op>,
    pub manifest: BTreeMap<String, ManifestEntry>,
}

impl Delta {
    /// The delta's bytes: canonical operations, so equal operations give equal bytes (FR-153).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = canonical::to_string(&self.header)?;
        out.push('\n');
        for op in &self.ops {
            out.push_str(&canonical::to_string(op)?);
            out.push('\n');
        }
        let manifest: serde_json::Map<String, Value> = self
            .manifest
            .iter()
            .map(|(p, (a, b))| (p.clone(), json!([a, b])))
            .collect();
        out.push_str(&canonical::to_string(&json!({ "manifest": manifest }))?);
        out.push('\n');
        Ok(out.into_bytes())
    }

    /// Parse a delta. Unknown header fields and unknown operation kinds are kept (FR-008).
    pub fn parse(bytes: &[u8]) -> Result<Delta> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| Error::Invalid("a delta file is not UTF-8".into()))?;
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let header: DeltaHeader = serde_json::from_str(
            lines
                .next()
                .ok_or_else(|| Error::Invalid("an empty delta file".into()))?,
        )
        .map_err(|e| Error::Invalid(format!("a delta header is not valid: {e}")))?;
        if header.resonance != "delta" {
            return Err(Error::Invalid("not a Resonance delta".into()));
        }
        if header.format > DELTA_FORMAT {
            return Err(Error::Invalid(format!(
                "delta {} uses format {}, newer than this Resonance understands ({DELTA_FORMAT}); upgrade resonance",
                header.id, header.format
            )));
        }
        let mut ops = Vec::new();
        let mut manifest = BTreeMap::new();
        for line in lines {
            let v: Value = serde_json::from_str(line).map_err(|e| {
                Error::Invalid(format!("delta {}: a line is not JSON: {e}", header.id))
            })?;
            if let Some(m) = v.get("manifest").and_then(Value::as_object) {
                for (path, entry) in m {
                    let pair = entry.as_array().cloned().unwrap_or_default();
                    let get = |i: usize| pair.get(i).and_then(Value::as_str).map(String::from);
                    manifest.insert(path.clone(), (get(0), get(1)));
                }
                continue;
            }
            ops.push(serde_json::from_value(v).map_err(|e| {
                Error::Invalid(format!(
                    "delta {}: an operation is not valid: {e}",
                    header.id
                ))
            })?);
        }
        Ok(Delta {
            header,
            ops,
            manifest,
        })
    }

    pub fn read(path: &Path) -> Result<Delta> {
        let bytes =
            fs::read(path).map_err(|e| Error::io(format!("reading {}", path.display()), e))?;
        Delta::parse(&bytes).map_err(|e| match e {
            Error::Invalid(m) => Error::Invalid(format!("{}: {m}", path.display())),
            other => other,
        })
    }
}

/// The delta ID of a file name such as `01J….delta`.
pub fn id_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let id = name.strip_suffix(&format!(".{EXTENSION}"))?;
    crate::model::ids::is_ulid(id).then(|| id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Actor;

    fn op(id: &str) -> Op {
        Op {
            op_id: id.into(),
            kind: kind::NODE_SET.into(),
            actor: Actor::human("dev@example.com"),
            hlc: "1700000000000-0".into(),
            change_set: "cs".into(),
            schema: 1,
            payload: json!({"value": 0.5, "key": "text", "id": "n"}),
        }
    }

    #[test]
    fn round_trip_and_canonical_bytes() {
        let d = Delta {
            header: DeltaHeader {
                resonance: "delta".into(),
                format: DELTA_FORMAT,
                id: "01JA0000000000000000000000".into(),
                schema: 1,
                writer: "human:dev@example.com".into(),
                version: "0.1.0".into(),
                epoch: "e".into(),
                branch: "main".into(),
            },
            ops: vec![op("a"), op("b")],
            manifest: BTreeMap::from([("src/a.rs".into(), (None, Some("h1".into())))]),
        };
        let bytes = d.to_bytes().unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(
            text.lines().nth(1).unwrap().starts_with(r#"{"actor":"#),
            "{text}"
        );
        assert!(text.ends_with("{\"manifest\":{\"src/a.rs\":[null,\"h1\"]}}\n"));
        let back = Delta::parse(&bytes).unwrap();
        assert_eq!(back, d);
        assert_eq!(back.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn newer_formats_are_refused() {
        let text = r#"{"resonance":"delta","format":99,"id":"x","schema":1,"writer":"","version":"","epoch":"","branch":""}"#;
        assert!(Delta::parse(text.as_bytes()).is_err());
        assert!(Delta::parse(b"").is_err());
    }
}
