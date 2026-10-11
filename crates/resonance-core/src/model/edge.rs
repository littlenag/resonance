//! Core edge types (§6.3, narrowed for the MVP).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::{Layer, LinkStatus, Props};

/// The edge types of the MVP (§6.3, partial).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeType {
    /// CodeUnit/File/Test → Requirement.
    Satisfies,
    /// Test → Requirement or CodeUnit.
    Verifies,
    /// Decision → any.
    Constrains,
    /// Annotation → any.
    Annotates,
    /// File/CodeUnit → CodeUnit, derived from anchor ranges.
    Contains,
    /// Evidence → edge.
    Evidences,
}

impl EdgeType {
    pub const ALL: [EdgeType; 6] = [
        EdgeType::Satisfies,
        EdgeType::Verifies,
        EdgeType::Constrains,
        EdgeType::Annotates,
        EdgeType::Contains,
        EdgeType::Evidences,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            EdgeType::Satisfies => "satisfies",
            EdgeType::Verifies => "verifies",
            EdgeType::Constrains => "constrains",
            EdgeType::Annotates => "annotates",
            EdgeType::Contains => "contains",
            EdgeType::Evidences => "evidences",
        }
    }

    /// The name used when reading the edge backward (§6.3): "satisfied by", "verified by", ...
    pub fn reverse_name(self) -> &'static str {
        match self {
            EdgeType::Satisfies => "satisfied by",
            EdgeType::Verifies => "verified by",
            EdgeType::Constrains => "constrained by",
            EdgeType::Annotates => "annotated by",
            EdgeType::Contains => "contained by",
            EdgeType::Evidences => "evidenced by",
        }
    }
}

impl fmt::Display for EdgeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EdgeType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();
        EdgeType::ALL
            .into_iter()
            .find(|t| t.as_str() == lower)
            .ok_or_else(|| {
                format!(
                    "unknown edge type `{s}` (expected one of: {})",
                    EdgeType::ALL.map(|t| t.as_str()).join(", ")
                )
            })
    }
}

/// An edge. Provenance, author, creation time, confidence and endpoint hashes live in `props`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    #[serde(rename = "type")]
    pub etype: EdgeType,
    pub src: String,
    pub dst: String,
    pub layer: Layer,
    pub status: LinkStatus,
    /// Removed by `edge.remove`; kept for history.
    pub removed: bool,
    pub props: Props,
}

impl Edge {
    pub fn str_prop(&self, key: &str) -> Option<&str> {
        self.props.get(key).and_then(|v| v.as_str())
    }

    /// The other endpoint, seen from `node`.
    pub fn other(&self, node: &str) -> &str {
        if self.src == node {
            &self.dst
        } else {
            &self.src
        }
    }
}
