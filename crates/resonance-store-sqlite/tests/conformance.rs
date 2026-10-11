//! Runs resonance-core's store conformance suite against `SqliteStore` (FR-167).

use resonance_core::store::conformance;
use resonance_store_sqlite::SqliteStore;

#[test]
fn sqlite_store_conforms_in_memory() {
    conformance::run_all(|| SqliteStore::open_in_memory().expect("open in-memory store"));
}

#[test]
fn sqlite_store_conforms_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let mut n = 0;
    conformance::run_all(|| {
        n += 1;
        SqliteStore::open(&dir.path().join(format!("graph-{n}.db"))).expect("open store file")
    });
}

#[test]
fn a_reopened_store_keeps_its_data() {
    use resonance_core::store::{GraphRead, GraphScope, GraphStore, meta};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.db");
    {
        let mut store = SqliteStore::open(&path).unwrap();
        store.migrate().unwrap();
        store
            .write(GraphScope::Primary, &mut |w| w.set_meta(meta::STAGE, "1"))
            .unwrap();
    }
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.get_meta(meta::STAGE).unwrap().as_deref(), Some("1"));
}
