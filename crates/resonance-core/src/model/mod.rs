//! The graph model: node and edge types, link statuses, layers and IDs (§6, MVP-008).

pub mod edge;
pub mod ids;
pub mod node;
pub mod status;

pub use edge::{Edge, EdgeType};
pub use ids::{content_hash, new_id};
pub use node::{Node, NodeType};
pub use status::{Layer, LinkStatus};

/// JSON object used for node, edge and operation properties.
pub type Props = serde_json::Map<String, serde_json::Value>;
