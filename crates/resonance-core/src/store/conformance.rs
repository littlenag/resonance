//! The store conformance suite (FR-167): generic tests that define the store API's behavior.
//! Every adapter runs it, for example:
//!
//! ```ignore
//! resonance_core::store::conformance::run_all(|| MyStore::open_in_memory().unwrap());
//! ```

use serde_json::json;

use super::{
    Direction, EdgeQuery, FileRecord, GraphScope, GraphStore, GraphWrite, NodeQuery,
    SCHEMA_VERSION, Snapshot, SnapshotMeta, Traversal, meta,
};
use crate::error::{Error, Result};
use crate::model::{Edge, EdgeType, Layer, LinkStatus, Node, NodeType, Props};
use crate::ops::{Actor, Op};

/// One conformance check, run against a fresh, unmigrated store.
pub type Check = fn(&mut dyn GraphStore);

/// Every check, by name.
pub const CHECKS: &[(&str, Check)] = &[
    ("migrate", check_migrate),
    ("newer_schema_refused", check_newer_schema_refused),
    ("nodes", check_nodes),
    ("node_property_filter", check_node_property_filter),
    ("edges", check_edges),
    ("transactions_are_atomic", check_transactions_are_atomic),
    ("reads_see_own_writes", check_reads_see_own_writes),
    ("blobs", check_blobs),
    ("files", check_files),
    ("meta", check_meta),
    ("ops", check_ops),
    ("traversal", check_traversal),
    ("clear_layer", check_clear_layer),
    ("integrity", check_integrity),
    ("meta_prefix", check_meta_prefix),
    ("clear_graph", check_clear_graph),
    ("snapshots", check_snapshots),
];

/// Run every check, each against a fresh, unmigrated store from `make`.
pub fn run_all<S: GraphStore + 'static>(mut make: impl FnMut() -> S) {
    for (name, check) in CHECKS {
        let mut store = make();
        eprintln!("conformance: {name}");
        check(&mut store);
    }
}

fn props(v: serde_json::Value) -> Props {
    v.as_object().cloned().unwrap_or_default()
}

fn node(id: &str, ntype: NodeType, layer: Layer, p: serde_json::Value) -> Node {
    Node::new(id, ntype, layer, props(p))
}

fn edge(id: &str, etype: EdgeType, src: &str, dst: &str, status: LinkStatus) -> Edge {
    Edge {
        id: id.into(),
        etype,
        src: src.into(),
        dst: dst.into(),
        layer: Layer::Authored,
        status,
        removed: false,
        props: props(json!({"provenance": "human"})),
    }
}

fn migrated(store: &mut dyn GraphStore) {
    store.migrate().expect("migrate");
}

fn write(store: &mut dyn GraphStore, mut f: impl FnMut(&mut dyn GraphWrite) -> Result<()>) {
    store
        .write(GraphScope::Primary, &mut |w| f(w))
        .expect("write transaction");
}

fn check_migrate(store: &mut dyn GraphStore) {
    assert_eq!(
        store.schema_version().unwrap(),
        0,
        "a new store has no schema yet"
    );
    assert_eq!(store.migrate().unwrap(), SCHEMA_VERSION);
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    assert_eq!(
        store.migrate().unwrap(),
        SCHEMA_VERSION,
        "migrate is idempotent"
    );
    assert_eq!(
        store.get_meta(meta::SCHEMA_VERSION).unwrap().as_deref(),
        Some(SCHEMA_VERSION.to_string().as_str()),
        "the schema version is stored in the graph (FR-006)"
    );
}

fn check_newer_schema_refused(store: &mut dyn GraphStore) {
    migrated(store);
    let newer = SCHEMA_VERSION + 1;
    write(store, |w| {
        w.set_meta(meta::SCHEMA_VERSION, &newer.to_string())
    });
    match store.migrate() {
        Err(Error::NewerSchema { found, supported }) => {
            assert_eq!(found, newer);
            assert_eq!(supported, SCHEMA_VERSION);
        }
        other => panic!("a newer schema must be refused (FR-258), got {other:?}"),
    }
}

fn check_nodes(store: &mut dyn GraphStore) {
    migrated(store);
    let a = node(
        "a",
        NodeType::Requirement,
        Layer::Authored,
        json!({"key": "FR-1", "text": "one"}),
    );
    let b = node(
        "b",
        NodeType::Decision,
        Layer::Authored,
        json!({"title": "two"}),
    );
    write(store, |w| {
        w.put_node(&a)?;
        w.put_node(&b)
    });
    assert_eq!(store.get_node("a").unwrap().as_ref(), Some(&a));
    assert_eq!(store.get_node("missing").unwrap(), None);

    let mut a2 = a.clone();
    a2.props.insert("text".into(), json!("one, revised"));
    write(store, |w| w.put_node(&a2));
    assert_eq!(
        store.get_node("a").unwrap(),
        Some(a2.clone()),
        "put_node replaces"
    );

    let reqs = store
        .find_nodes(&NodeQuery::of_type(NodeType::Requirement))
        .unwrap();
    assert_eq!(reqs, vec![a2.clone()]);
    let all = store.find_nodes(&NodeQuery::default()).unwrap();
    assert_eq!(
        all.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        ["a", "b"],
        "ordered by ID"
    );

    let mut retired = b.clone();
    retired.retired = true;
    write(store, |w| w.put_node(&retired));
    assert_eq!(
        store.find_nodes(&NodeQuery::default()).unwrap().len(),
        1,
        "retired nodes are hidden"
    );
    assert_eq!(
        store
            .find_nodes(&NodeQuery::default().including_retired())
            .unwrap()
            .len(),
        2,
        "but still there"
    );
    assert!(store.get_node("b").unwrap().unwrap().retired);
}

fn check_node_property_filter(store: &mut dyn GraphStore) {
    migrated(store);
    write(store, |w| {
        w.put_node(&node(
            "u1",
            NodeType::CodeUnit,
            Layer::Authored,
            json!({"path": "src/a.rs", "n": 1}),
        ))?;
        w.put_node(&node(
            "u2",
            NodeType::CodeUnit,
            Layer::Authored,
            json!({"path": "src/b.rs", "n": 2}),
        ))?;
        w.put_node(&node(
            "t1",
            NodeType::Test,
            Layer::Authored,
            json!({"path": "src/a.rs", "flag": true}),
        ))
    });
    let in_a = store
        .find_nodes(&NodeQuery::default().with_prop("path", "src/a.rs"))
        .unwrap();
    assert_eq!(
        in_a.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        ["t1", "u1"]
    );
    let units_in_a = store
        .find_nodes(&NodeQuery::of_type(NodeType::CodeUnit).with_prop("path", "src/a.rs"))
        .unwrap();
    assert_eq!(units_in_a.len(), 1);
    assert_eq!(
        store
            .find_nodes(&NodeQuery::default().with_prop("n", 2))
            .unwrap()[0]
            .id,
        "u2"
    );
    assert_eq!(
        store
            .find_nodes(&NodeQuery::default().with_prop("flag", true))
            .unwrap()[0]
            .id,
        "t1"
    );
}

fn check_edges(store: &mut dyn GraphStore) {
    migrated(store);
    let e1 = edge("e1", EdgeType::Satisfies, "u", "r", LinkStatus::Asserted);
    let e2 = edge("e2", EdgeType::Verifies, "t", "r", LinkStatus::Verified);
    let mut e3 = edge("e3", EdgeType::Satisfies, "u", "r2", LinkStatus::Stale);
    e3.layer = Layer::Derived;
    write(store, |w| {
        w.put_edge(&e1)?;
        w.put_edge(&e2)?;
        w.put_edge(&e3)
    });
    assert_eq!(store.get_edge("e1").unwrap().as_ref(), Some(&e1));
    let into_r = store.find_edges(&EdgeQuery::to("r")).unwrap();
    assert_eq!(
        into_r.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        ["e1", "e2"]
    );
    assert_eq!(store.find_edges(&EdgeQuery::from("u")).unwrap().len(), 2);
    assert_eq!(
        store
            .find_edges(&EdgeQuery::to("r").of_type(EdgeType::Verifies))
            .unwrap(),
        vec![e2.clone()]
    );
    assert_eq!(
        store
            .find_edges(&EdgeQuery::all().with_status(LinkStatus::Stale))
            .unwrap(),
        vec![e3.clone()]
    );
    assert_eq!(
        store
            .find_edges(&EdgeQuery::all().in_layer(Layer::Derived))
            .unwrap(),
        vec![e3.clone()]
    );

    let mut removed = e1.clone();
    removed.removed = true;
    removed.status = LinkStatus::Orphaned;
    write(store, |w| w.put_edge(&removed));
    assert_eq!(
        store.find_edges(&EdgeQuery::from("u")).unwrap().len(),
        1,
        "removed edges are hidden"
    );
    assert_eq!(
        store
            .find_edges(&EdgeQuery::from("u").including_removed())
            .unwrap()
            .len(),
        2
    );
    assert_eq!(store.get_edge("e1").unwrap(), Some(removed));

    write(store, |w| w.delete_edge("e3"));
    assert_eq!(store.get_edge("e3").unwrap(), None);
}

fn check_transactions_are_atomic(store: &mut dyn GraphStore) {
    migrated(store);
    let result = store.write(GraphScope::Primary, &mut |w| {
        assert_eq!(w.scope(), GraphScope::Primary);
        w.put_node(&node("x", NodeType::Decision, Layer::Authored, json!({})))?;
        w.set_meta("k", "v")?;
        Err(Error::Invalid("abort".into()))
    });
    assert!(result.is_err());
    assert_eq!(
        store.get_node("x").unwrap(),
        None,
        "a failed change set leaves no trace (FR-022)"
    );
    assert_eq!(store.get_meta("k").unwrap(), None);
}

fn check_reads_see_own_writes(store: &mut dyn GraphStore) {
    migrated(store);
    write(store, |w| {
        w.put_node(&node(
            "y",
            NodeType::Decision,
            Layer::Authored,
            json!({"title": "t"}),
        ))?;
        assert!(w.get_node("y")?.is_some());
        w.put_edge(&edge(
            "ey",
            EdgeType::Constrains,
            "y",
            "z",
            LinkStatus::Asserted,
        ))?;
        assert_eq!(w.find_edges(&EdgeQuery::from("y"))?.len(), 1);
        w.put_file(&FileRecord {
            path: "p".into(),
            blob: "h".into(),
            size: 1,
            mtime_ns: 1,
            recorded_ns: 2,
        })?;
        assert!(w.get_file("p")?.is_some());
        Ok(())
    });
}

fn check_blobs(store: &mut dyn GraphStore) {
    migrated(store);
    write(store, |w| {
        w.put_blob("h1", 3, Some(b"abc"))?;
        w.put_blob("h2", 9_000_000, None)
    });
    assert_eq!(store.get_blob("h1").unwrap().as_deref(), Some(&b"abc"[..]));
    assert!(
        store.has_blob("h2").unwrap(),
        "hash-and-size-only blobs exist (MVP-016)"
    );
    assert_eq!(store.get_blob("h2").unwrap(), None);
    assert!(!store.has_blob("h3").unwrap());
    write(store, |w| w.put_blob("h1", 3, None));
    assert_eq!(
        store.get_blob("h1").unwrap().as_deref(),
        Some(&b"abc"[..]),
        "putting a known hash never drops its bytes"
    );
    write(store, |w| w.put_blob("h2", 9_000_000, Some(b"late")));
    assert_eq!(store.get_blob("h2").unwrap().as_deref(), Some(&b"late"[..]));
}

fn check_files(store: &mut dyn GraphStore) {
    migrated(store);
    let rec = |path: &str, blob: &str| FileRecord {
        path: path.into(),
        blob: blob.into(),
        size: 10,
        mtime_ns: 100,
        recorded_ns: 200,
    };
    write(store, |w| {
        w.put_file(&rec("b.txt", "h1"))?;
        w.put_file(&rec("a.txt", "h2"))
    });
    assert_eq!(store.get_file("a.txt").unwrap(), Some(rec("a.txt", "h2")));
    assert_eq!(
        store
            .list_files()
            .unwrap()
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["a.txt", "b.txt"]
    );
    write(store, |w| w.put_file(&rec("a.txt", "h3")));
    assert_eq!(store.get_file("a.txt").unwrap().unwrap().blob, "h3");
    write(store, |w| w.delete_file("a.txt"));
    assert_eq!(store.get_file("a.txt").unwrap(), None);
    write(store, |w| w.clear_files());
    assert!(store.list_files().unwrap().is_empty());
}

fn check_meta(store: &mut dyn GraphStore) {
    migrated(store);
    assert_eq!(store.get_meta(meta::STAGE).unwrap(), None);
    write(store, |w| {
        w.set_meta(meta::STAGE, "1")?;
        w.set_meta(meta::PARTICIPATION, "offline")
    });
    assert_eq!(store.get_meta(meta::STAGE).unwrap().as_deref(), Some("1"));
    write(store, |w| w.set_meta(meta::STAGE, "2"));
    assert_eq!(store.get_meta(meta::STAGE).unwrap().as_deref(), Some("2"));
}

fn check_ops(store: &mut dyn GraphStore) {
    migrated(store);
    let op = |id: &str| Op {
        op_id: id.into(),
        kind: "node.create".into(),
        actor: Actor::human("someone"),
        hlc: "0000000000001.00000".into(),
        change_set: "cs".into(),
        schema: SCHEMA_VERSION,
        payload: json!({"id": id, "nested": {"b": 1, "a": [1, 2]}}),
    };
    write(store, |w| {
        w.append_op(&op("o1"))?;
        w.append_op(&op("o2"))?;
        w.append_op(&op("o1")) // idempotent by opId (FR-193)
    });
    let all = store.ops_after(0, 100).unwrap();
    assert_eq!(
        all.iter().map(|r| r.op.op_id.as_str()).collect::<Vec<_>>(),
        ["o1", "o2"]
    );
    assert_eq!(all[0].op, op("o1"), "operations round-trip exactly");
    assert!(all[0].seq < all[1].seq);
    let rest = store.ops_after(all[0].seq, 100).unwrap();
    assert_eq!(rest.len(), 1);
    assert_eq!(store.ops_after(0, 1).unwrap().len(), 1, "limit is honored");
    assert_eq!(store.last_op_seq().unwrap(), all[1].seq);
}

fn check_meta_prefix(store: &mut dyn GraphStore) {
    migrated(store);
    write(store, |w| {
        w.set_meta("delta:b", "2")?;
        w.set_meta("delta:a", "1")?;
        w.set_meta("deltaz", "x")?;
        w.set_meta("other", "y")
    });
    let found = store.meta_prefix("delta:").unwrap();
    assert_eq!(
        found,
        vec![
            ("delta:a".into(), "1".into()),
            ("delta:b".into(), "2".into())
        ],
        "only matching keys, in key order"
    );
    write(store, |w| w.delete_meta("delta:a"));
    assert_eq!(store.meta_prefix("delta:").unwrap().len(), 1);
    assert_eq!(store.get_meta("delta:a").unwrap(), None);
    write(store, |w| w.delete_meta("never-set"));
}

fn check_clear_graph(store: &mut dyn GraphStore) {
    migrated(store);
    let op = Op {
        op_id: "o1".into(),
        kind: "node.create".into(),
        actor: Actor::human("someone"),
        hlc: "0000000000001.00000".into(),
        change_set: "cs".into(),
        schema: SCHEMA_VERSION,
        payload: json!({}),
    };
    write(store, |w| {
        w.put_node(&node(
            "a",
            NodeType::Requirement,
            Layer::Authored,
            json!({}),
        ))?;
        w.put_node(&node("b", NodeType::File, Layer::Authored, json!({})))?;
        w.put_edge(&edge(
            "e",
            EdgeType::Satisfies,
            "b",
            "a",
            LinkStatus::Asserted,
        ))?;
        w.put_blob("h", 1, Some(b"x"))?;
        w.put_file(&FileRecord {
            path: "x".into(),
            blob: "h".into(),
            size: 1,
            mtime_ns: 1,
            recorded_ns: 2,
        })?;
        w.set_meta("k", "v")?;
        w.append_op(&op)
    });
    let before = store.last_op_seq().unwrap();
    write(store, |w| w.clear_graph());
    assert!(store.get_node("a").unwrap().is_none());
    assert!(store.get_edge("e").unwrap().is_none());
    assert!(store.ops_after(0, 10).unwrap().is_empty());
    assert!(store.has_blob("h").unwrap(), "blobs are kept");
    assert!(
        store.get_file("x").unwrap().is_some(),
        "file records are kept"
    );
    assert_eq!(
        store.get_meta("k").unwrap().as_deref(),
        Some("v"),
        "metadata is kept"
    );
    write(store, |w| {
        w.append_op(&Op {
            op_id: "o2".into(),
            ..op.clone()
        })
    });
    assert!(
        store.last_op_seq().unwrap() > before,
        "sequence numbers are never reused"
    );
}

fn check_snapshots(store: &mut dyn GraphStore) {
    migrated(store);
    let dir = std::env::temp_dir().join(format!(
        "resonance-conformance-{}-{}",
        std::process::id(),
        crate::model::new_id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let op = |id: &str, v: f64| Op {
        op_id: id.into(),
        kind: "node.set".into(),
        actor: Actor::agent("claude"),
        hlc: "0000000000001.00000".into(),
        change_set: "cs".into(),
        schema: SCHEMA_VERSION,
        payload: json!({"id": "n", "key": "confidence", "value": v, "z": [1, {"b": 2, "a": 1}]}),
    };
    let snapshot = Snapshot {
        meta: SnapshotMeta {
            format: String::new(),
            format_version: 0,
            schema_version: SCHEMA_VERSION,
            epoch: "epoch-1".into(),
            folded_deltas: vec!["d1".into(), "d2".into()],
        },
        ops: vec![op("o1", 0.75), op("o2", 0.5)],
    };
    let a = dir.join("a.db");
    let b = dir.join("b.db");
    store.write_snapshot(&a, &snapshot).unwrap();
    store.write_snapshot(&b, &snapshot).unwrap();
    assert_eq!(
        std::fs::read(&a).unwrap(),
        std::fs::read(&b).unwrap(),
        "the same snapshot gives byte-identical files (FR-152)"
    );
    let back = store.read_snapshot(&a).unwrap();
    assert_eq!(back.ops, snapshot.ops, "operations round-trip in order");
    assert_eq!(back.meta.epoch, "epoch-1");
    assert_eq!(back.meta.folded_deltas, ["d1", "d2"]);
    assert_eq!(back.meta.schema_version, SCHEMA_VERSION);
    assert!(!back.meta.format.is_empty(), "the adapter names its format");
    let junk = dir.join("junk.db");
    std::fs::write(&junk, b"not a snapshot").unwrap();
    assert!(store.read_snapshot(&junk).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

fn check_traversal(store: &mut dyn GraphStore) {
    migrated(store);
    // a -satisfies-> b -verifies-> c -satisfies-> a (a cycle), plus a stale a -> d.
    write(store, |w| {
        for id in ["a", "b", "c", "d"] {
            w.put_node(&node(id, NodeType::CodeUnit, Layer::Authored, json!({})))?;
        }
        w.put_edge(&edge(
            "e1",
            EdgeType::Satisfies,
            "a",
            "b",
            LinkStatus::Asserted,
        ))?;
        w.put_edge(&edge(
            "e2",
            EdgeType::Verifies,
            "b",
            "c",
            LinkStatus::Verified,
        ))?;
        w.put_edge(&edge(
            "e3",
            EdgeType::Satisfies,
            "c",
            "a",
            LinkStatus::Asserted,
        ))?;
        w.put_edge(&edge(
            "e4",
            EdgeType::Satisfies,
            "a",
            "d",
            LinkStatus::Stale,
        ))
    });
    let reached = |t: Traversal| -> Vec<(usize, String)> {
        store
            .traverse(&t)
            .unwrap()
            .into_iter()
            .map(|h| (h.depth, h.node))
            .collect()
    };
    assert_eq!(
        reached(Traversal::new("a", Direction::Out, 1)),
        [(1, "b".to_string()), (1, "d".to_string())]
    );
    assert_eq!(
        reached(Traversal::new("a", Direction::Out, 5)),
        [
            (1, "b".to_string()),
            (1, "d".to_string()),
            (2, "c".to_string())
        ],
        "each node is reached once, and cycles terminate"
    );
    assert_eq!(
        reached(Traversal::new("a", Direction::In, 1)),
        [(1, "c".to_string())]
    );
    assert_eq!(
        reached(Traversal::new("a", Direction::Out, 5).types(&[EdgeType::Satisfies])),
        [(1, "b".to_string()), (1, "d".to_string())]
    );
    assert_eq!(
        reached(Traversal::new("a", Direction::Out, 5).statuses(&[LinkStatus::Stale])),
        [(1, "d".to_string())]
    );
    assert_eq!(
        reached(Traversal::new("b", Direction::Both, 1)),
        [(1, "a".to_string()), (1, "c".to_string())]
    );
}

fn check_clear_layer(store: &mut dyn GraphStore) {
    migrated(store);
    let mut derived = edge("d1", EdgeType::Contains, "f", "u", LinkStatus::Asserted);
    derived.layer = Layer::Derived;
    write(store, |w| {
        w.put_node(&node("f", NodeType::File, Layer::Authored, json!({})))?;
        w.put_node(&node("dn", NodeType::Decision, Layer::Derived, json!({})))?;
        w.put_edge(&derived)?;
        w.put_edge(&edge(
            "a1",
            EdgeType::Satisfies,
            "u",
            "r",
            LinkStatus::Asserted,
        ))
    });
    write(store, |w| w.clear_layer(Layer::Derived));
    assert_eq!(store.get_edge("d1").unwrap(), None);
    assert_eq!(store.get_node("dn").unwrap(), None);
    assert!(
        store.get_edge("a1").unwrap().is_some(),
        "authored data survives (FR-023)"
    );
    assert!(store.get_node("f").unwrap().is_some());
}

fn check_integrity(store: &mut dyn GraphStore) {
    migrated(store);
    write(store, |w| {
        w.put_node(&node("u", NodeType::CodeUnit, Layer::Authored, json!({})))?;
        w.put_node(&node(
            "r",
            NodeType::Requirement,
            Layer::Authored,
            json!({}),
        ))?;
        w.put_edge(&edge(
            "ok",
            EdgeType::Satisfies,
            "u",
            "r",
            LinkStatus::Asserted,
        ))?;
        w.put_node(&node("ev", NodeType::Evidence, Layer::Authored, json!({})))?;
        w.put_edge(&edge(
            "evok",
            EdgeType::Evidences,
            "ev",
            "ok",
            LinkStatus::Asserted,
        ))
    });
    assert_eq!(store.integrity_check().unwrap(), Vec::<String>::new());
    write(store, |w| {
        w.put_edge(&edge(
            "bad",
            EdgeType::Satisfies,
            "u",
            "nowhere",
            LinkStatus::Asserted,
        ))
    });
    let problems = store.integrity_check().unwrap();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("bad"));
}
