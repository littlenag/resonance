//! Core edge types (§6.3, narrowed for the MVP).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::{Layer, LinkStatus, Props};

/// The core edge types of the primary graph (§6.3), plus namespaced extensions (FR-007).
/// `references` lives only in parallel graphs (§6.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    /// Requirement/Design → Requirement: refinement or decomposition.
    DerivedFrom,
    /// Same type → same type: replaces an earlier version.
    Supersedes,
    /// CodeUnit → Design: the current realization of a design element.
    Realizes,
    /// A namespaced extension edge such as `acme:deploys`; the name is in `extType` (FR-007).
    Extension,
}

impl EdgeType {
    pub const ALL: [EdgeType; 10] = [
        EdgeType::Satisfies,
        EdgeType::Verifies,
        EdgeType::Constrains,
        EdgeType::Annotates,
        EdgeType::Contains,
        EdgeType::Evidences,
        EdgeType::DerivedFrom,
        EdgeType::Supersedes,
        EdgeType::Realizes,
        EdgeType::Extension,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            EdgeType::Satisfies => "satisfies",
            EdgeType::Verifies => "verifies",
            EdgeType::Constrains => "constrains",
            EdgeType::Annotates => "annotates",
            EdgeType::Contains => "contains",
            EdgeType::Evidences => "evidences",
            EdgeType::DerivedFrom => "derivedFrom",
            EdgeType::Supersedes => "supersedes",
            EdgeType::Realizes => "realizes",
            EdgeType::Extension => "extension",
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
            EdgeType::DerivedFrom => "refined by",
            EdgeType::Supersedes => "superseded by",
            EdgeType::Realizes => "realized by",
            EdgeType::Extension => "extended by",
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
            .find(|t| t.as_str().to_ascii_lowercase() == lower)
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
