//! Resonance core library: everything except the storage engine and the CLI front end.
//!
//! Depends on no SQLite crate and no source-code parser (FR-110, FR-161).

pub mod agent;
pub mod anchors;
pub mod api;
pub mod changes;
pub mod daemon;
pub mod diag;
pub mod error;
pub mod export;
pub mod history;
pub mod hooks;
pub mod ingest;
pub mod lifecycle;
pub mod links;
pub mod logging;
pub mod mcp;
pub mod model;
pub mod ops;
pub mod query;
pub mod recovery;
pub mod registry;
pub mod repo;
pub mod service;
pub mod store;
pub mod verify;
