//! The in-memory adapter passes the same conformance suite as SQLite (FR-167, S21 AC1).

use resonance_core::store::conformance;
use resonance_store_memory::MemoryStore;

#[test]
fn memory_store_conforms() {
    conformance::run_all(MemoryStore::new);
}
