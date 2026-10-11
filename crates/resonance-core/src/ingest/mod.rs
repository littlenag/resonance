//! Keeps the graph in step with the file tree, reconciling before every answer (MVP-002).

pub mod blobs;
pub mod lineindex;
pub mod reconcile;
pub mod walk;

pub use reconcile::{ReconcileOptions, ReconcileReport, reconcile, reconcile_paths};
