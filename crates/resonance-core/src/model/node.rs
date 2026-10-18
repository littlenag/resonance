//! Core node types (§6.2, narrowed for the MVP).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::{Layer, Props};

/// The core node types (§6.2), plus namespaced extensions (FR-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NodeType {
    Requirement,
    Decision,
    /// A design element: component, interface, flow.
    Design,
    /// A rule code must obey (performance, compatibility, policy).
    Constraint,
    File,
    /// A declared region of a file (MVP-013).
    CodeUnit,
    /// A code unit with a command that runs it alone (MVP-017).
    Test,
    Evidence,
    Annotation,
    /// An unresolved semantic conflict found while folding concurrent changes (§9.4).
    Conflict,
    /// A namespaced extension type such as `acme:Service`; the name is in the `extType`
    /// property (FR-007). Preserved by every version (FR-008).
    Extension,
}

impl NodeType {
    pub const ALL: [NodeType; 11] = [
        NodeType::Requirement,
        NodeType::Decision,
        NodeType::Design,
        NodeType::Constraint,
        NodeType::File,
        NodeType::CodeUnit,
        NodeType::Test,
        NodeType::Evidence,
        NodeType::Annotation,
        NodeType::Conflict,
        NodeType::Extension,
    ];

    /// Spec items: the intent the TUI lists and edits (§8.22).
    pub const SPEC_ITEMS: [NodeType; 4] = [
        NodeType::Requirement,
        NodeType::Decision,
        NodeType::Design,
        NodeType::Constraint,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            NodeType::Requirement => "Requirement",
            NodeType::Decision => "Decision",
            NodeType::Design => "Design",
            NodeType::Constraint => "Constraint",
            NodeType::Conflict => "Conflict",
            NodeType::Extension => "Extension",
            NodeType::File => "File",
            NodeType::CodeUnit => "CodeUnit",
            NodeType::Test => "Test",
            NodeType::Evidence => "Evidence",
            NodeType::Annotation => "Annotation",
        }
    }

    /// Code units and tests carry anchors.
    pub fn is_anchored(self) -> bool {
        matches!(self, NodeType::CodeUnit | NodeType::Test)
    }
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for NodeType {
    type Err = String;
    /// Accepts the type name in any case, plus a few friendly aliases (`unit`, `req`, `test`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();
        let alias = match lower.as_str() {
            "requirement" | "req" | "requirements" => Some(NodeType::Requirement),
            "decision" | "decisions" => Some(NodeType::Decision),
            "design" | "designs" => Some(NodeType::Design),
            "constraint" | "constraints" => Some(NodeType::Constraint),
            "conflict" | "conflicts" => Some(NodeType::Conflict),
            "extension" | "extensions" => Some(NodeType::Extension),
            "file" | "files" => Some(NodeType::File),
            "codeunit" | "unit" | "units" | "code-unit" => Some(NodeType::CodeUnit),
            "test" | "tests" => Some(NodeType::Test),
            "evidence" => Some(NodeType::Evidence),
            "annotation" | "annotations" => Some(NodeType::Annotation),
            _ => None,
        };
        alias.ok_or_else(|| {
            format!(
                "unknown node type `{s}` (expected one of: {})",
                NodeType::ALL.map(|t| t.as_str()).join(", ")
            )
        })
    }
}

/// A node: an ID, a type, a layer, a tombstone flag, and properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub ntype: NodeType,
    pub layer: Layer,
    /// Tombstone (`node.retire`): the node stays for history and link resolution (§9.3).
    pub retired: bool,
    pub props: Props,
}

impl Node {
    pub fn new(id: impl Into<String>, ntype: NodeType, layer: Layer, props: Props) -> Self {
        Node {
            id: id.into(),
            ntype,
            layer,
            retired: false,
            props,
        }
    }

    /// A string property, if present.
    pub fn str_prop(&self, key: &str) -> Option<&str> {
        self.props.get(key).and_then(|v| v.as_str())
    }

    /// An integer property, if present.
    pub fn u64_prop(&self, key: &str) -> Option<u64> {
        self.props.get(key).and_then(|v| v.as_u64())
    }
}
