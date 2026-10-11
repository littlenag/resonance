//! The operation envelope (§9.1, MVP-010).

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who made a change (§9.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorKind {
    Human,
    Agent,
    Tool,
}

impl ActorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActorKind::Human => "human",
            ActorKind::Agent => "agent",
            ActorKind::Tool => "tool",
        }
    }
}

impl Actor {
    pub fn human(id: impl Into<String>) -> Self {
        Actor {
            kind: ActorKind::Human,
            id: id.into(),
        }
    }
    pub fn agent(id: impl Into<String>) -> Self {
        Actor {
            kind: ActorKind::Agent,
            id: id.into(),
        }
    }
    /// Resonance itself, for automatic changes made during reconcile.
    pub fn resonance() -> Self {
        Actor {
            kind: ActorKind::Tool,
            id: "resonance".into(),
        }
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind.as_str(), self.id)
    }
}

/// One edit-algebra operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Op {
    pub op_id: String,
    /// Operation type, for example `node.create` or `identity.move` (§9.2–§9.5).
    pub kind: String,
    pub actor: Actor,
    pub hlc: String,
    pub change_set: String,
    pub schema: u32,
    pub payload: Value,
}

/// Operation kinds (§9.2–§9.5).
pub mod kind {
    pub const NODE_CREATE: &str = "node.create";
    pub const NODE_SET: &str = "node.set";
    pub const NODE_UNSET: &str = "node.unset";
    pub const NODE_RETIRE: &str = "node.retire";
    pub const EDGE_ADD: &str = "edge.add";
    pub const EDGE_REMOVE: &str = "edge.remove";
    pub const EDGE_SET: &str = "edge.set";
    pub const IDENTITY_MOVE: &str = "identity.move";
    pub const IDENTITY_RETARGET: &str = "identity.retarget";
    pub const EVIDENCE_RECORD: &str = "evidence.record";
    pub const FILE_PUT: &str = "file.put";
    pub const FILE_DELETE: &str = "file.delete";
}
