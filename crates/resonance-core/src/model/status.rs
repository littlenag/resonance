//! Link statuses (FR-001, §6.5) and data layers (FR-023).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The status of an edge. Every edge has exactly one (FR-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkStatus {
    Asserted,
    Verified,
    Stale,
    Orphaned,
}

impl LinkStatus {
    pub const ALL: [LinkStatus; 4] = [
        LinkStatus::Asserted,
        LinkStatus::Verified,
        LinkStatus::Stale,
        LinkStatus::Orphaned,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            LinkStatus::Asserted => "asserted",
            LinkStatus::Verified => "verified",
            LinkStatus::Stale => "stale",
            LinkStatus::Orphaned => "orphaned",
        }
    }
}

impl fmt::Display for LinkStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LinkStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LinkStatus::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| {
                format!(
                    "unknown link status `{s}` (expected asserted, verified, stale or orphaned)"
                )
            })
    }
}

/// Where a piece of data comes from, which determines whether it can be dropped and rebuilt (FR-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
    /// Written by people and agents, or recorded from the files they wrote. Never dropped.
    Authored,
    /// Identity decisions: anchors and their history. Never dropped.
    Identity,
    /// Recomputable from file content plus the authored and identity layers.
    Derived,
}

impl Layer {
    pub fn as_str(self) -> &'static str {
        match self {
            Layer::Authored => "authored",
            Layer::Identity => "identity",
            Layer::Derived => "derived",
        }
    }
}

impl FromStr for Layer {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "authored" => Ok(Layer::Authored),
            "identity" => Ok(Layer::Identity),
            "derived" => Ok(Layer::Derived),
            _ => Err(format!("unknown layer `{s}`")),
        }
    }
}
