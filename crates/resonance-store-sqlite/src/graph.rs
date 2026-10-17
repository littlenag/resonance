//! Nodes, edges, file records and metadata.

use resonance_core::error::{Error, Result};
use resonance_core::model::{Edge, Node};
use resonance_core::store::{EdgeQuery, FileRecord, NodeQuery};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde_json::Value;

use crate::db;

const NODE_COLS: &str = "id, type, layer, retired, props";
const EDGE_COLS: &str = "id, type, src, dst, layer, status, removed, props";

fn parse_enum<T: serde::de::DeserializeOwned>(s: &str) -> rusqlite::Result<T> {
    serde_json::from_value(Value::String(s.to_string())).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn parse_props(s: &str) -> rusqlite::Result<serde_json::Map<String, Value>> {
    serde_json::from_str(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn node_from_row(r: &Row<'_>) -> rusqlite::Result<Node> {
    Ok(Node {
        id: r.get(0)?,
        ntype: parse_enum(&r.get::<_, String>(1)?)?,
        layer: parse_enum(&r.get::<_, String>(2)?)?,
        retired: r.get::<_, i64>(3)? != 0,
        props: parse_props(&r.get::<_, String>(4)?)?,
    })
}

fn edge_from_row(r: &Row<'_>) -> rusqlite::Result<Edge> {
    Ok(Edge {
        id: r.get(0)?,
        etype: parse_enum(&r.get::<_, String>(1)?)?,
        src: r.get(2)?,
        dst: r.get(3)?,
        layer: parse_enum(&r.get::<_, String>(4)?)?,
        status: parse_enum(&r.get::<_, String>(5)?)?,
        removed: r.get::<_, i64>(6)? != 0,
        props: parse_props(&r.get::<_, String>(7)?)?,
    })
}

fn sql_value(v: &Value) -> Result<SqlValue> {
    Ok(match v {
        Value::String(s) => SqlValue::Text(s.clone()),
        Value::Bool(b) => SqlValue::Integer(*b as i64),
        Value::Number(n) if n.is_i64() => SqlValue::Integer(n.as_i64().unwrap_or_default()),
        Value::Number(n) => SqlValue::Real(n.as_f64().unwrap_or_default()),
        Value::Null => SqlValue::Null,
        _ => {
            return Err(Error::Store(
                "property filters support only scalar values".into(),
            ));
        }
    })
}

fn valid_key(key: &str) -> Result<()> {
    if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(())
    } else {
        Err(Error::Store(format!(
            "invalid property name `{key}` in a filter"
        )))
    }
}

pub fn get_node(conn: &Connection, graph: &str, id: &str) -> Result<Option<Node>> {
    db(conn
        .query_row(
            &format!("SELECT {NODE_COLS} FROM nodes WHERE graph = ?1 AND id = ?2"),
            params![graph, id],
            node_from_row,
        )
        .optional())
}

pub fn find_nodes(conn: &Connection, graph: &str, q: &NodeQuery) -> Result<Vec<Node>> {
    let mut sql = format!("SELECT {NODE_COLS} FROM nodes WHERE graph = ?");
    let mut args: Vec<SqlValue> = vec![SqlValue::Text(graph.to_string())];
    if let Some(t) = q.ntype {
        sql.push_str(" AND type = ?");
        args.push(SqlValue::Text(t.as_str().to_string()));
    }
    if let Some((key, value)) = &q.prop_eq {
        valid_key(key)?;
        // Inlined so that `json_extract(props, '$.path')` can use the `nodes_path` index.
        sql.push_str(&format!(" AND json_extract(props, '$.{key}') = ?"));
        args.push(sql_value(value)?);
    }
    if !q.include_retired {
        sql.push_str(" AND retired = 0");
    }
    sql.push_str(" ORDER BY id");
    let mut stmt = db(conn.prepare_cached(&sql))?;
    let rows = db(stmt.query_map(params_from_iter(args), node_from_row))?;
    db(rows.collect())
}

pub fn put_node(conn: &Connection, graph: &str, node: &Node) -> Result<()> {
    db(conn
        .prepare_cached(
            "INSERT INTO nodes (graph, id, type, layer, retired, props) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (graph, id) DO UPDATE SET type = excluded.type, layer = excluded.layer,
             retired = excluded.retired, props = excluded.props",
        )
        .and_then(|mut s| {
            s.execute(params![
                graph,
                node.id,
                node.ntype.as_str(),
                node.layer.as_str(),
                node.retired as i64,
                serde_json::to_string(&node.props).unwrap_or_default(),
            ])
        }))?;
    Ok(())
}

pub fn get_edge(conn: &Connection, graph: &str, id: &str) -> Result<Option<Edge>> {
    db(conn
        .query_row(
            &format!("SELECT {EDGE_COLS} FROM edges WHERE graph = ?1 AND id = ?2"),
            params![graph, id],
            edge_from_row,
        )
        .optional())
}

pub fn find_edges(conn: &Connection, graph: &str, q: &EdgeQuery) -> Result<Vec<Edge>> {
    let mut sql = format!("SELECT {EDGE_COLS} FROM edges WHERE graph = ?");
    let mut args: Vec<SqlValue> = vec![SqlValue::Text(graph.to_string())];
    let mut add = |clause: &str, v: String| {
        sql.push_str(clause);
        args.push(SqlValue::Text(v));
    };
    if let Some(src) = &q.src {
        add(" AND src = ?", src.clone());
    }
    if let Some(dst) = &q.dst {
        add(" AND dst = ?", dst.clone());
    }
    if let Some(t) = q.etype {
        add(" AND type = ?", t.as_str().to_string());
    }
    if let Some(s) = q.status {
        add(" AND status = ?", s.as_str().to_string());
    }
    if let Some(l) = q.layer {
        add(" AND layer = ?", l.as_str().to_string());
    }
    if !q.include_removed {
        sql.push_str(" AND removed = 0");
    }
    sql.push_str(" ORDER BY id");
    let mut stmt = db(conn.prepare_cached(&sql))?;
    let rows = db(stmt.query_map(params_from_iter(args), edge_from_row))?;
    db(rows.collect())
}

pub fn put_edge(conn: &Connection, graph: &str, edge: &Edge) -> Result<()> {
    db(conn
        .prepare_cached(
            "INSERT INTO edges (graph, id, type, src, dst, layer, status, removed, props)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (graph, id) DO UPDATE SET type = excluded.type, src = excluded.src,
             dst = excluded.dst, layer = excluded.layer, status = excluded.status,
             removed = excluded.removed, props = excluded.props",
        )
        .and_then(|mut s| {
            s.execute(params![
                graph,
                edge.id,
                edge.etype.as_str(),
                edge.src,
                edge.dst,
                edge.layer.as_str(),
                edge.status.as_str(),
                edge.removed as i64,
                serde_json::to_string(&edge.props).unwrap_or_default(),
            ])
        }))?;
    Ok(())
}

pub fn delete_edge(conn: &Connection, graph: &str, id: &str) -> Result<()> {
    db(conn.execute(
        "DELETE FROM edges WHERE graph = ?1 AND id = ?2",
        params![graph, id],
    ))?;
    Ok(())
}

pub fn clear_layer(conn: &Connection, graph: &str, layer: &str) -> Result<()> {
    db(conn.execute(
        "DELETE FROM edges WHERE graph = ?1 AND layer = ?2",
        params![graph, layer],
    ))?;
    db(conn.execute(
        "DELETE FROM nodes WHERE graph = ?1 AND layer = ?2",
        params![graph, layer],
    ))?;
    Ok(())
}

fn file_from_row(r: &Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        path: r.get(0)?,
        blob: r.get(1)?,
        size: r.get::<_, i64>(2)? as u64,
        mtime_ns: r.get(3)?,
        recorded_ns: r.get(4)?,
    })
}

pub fn get_file(conn: &Connection, graph: &str, path: &str) -> Result<Option<FileRecord>> {
    db(conn
        .query_row(
            "SELECT path, blob, size, mtime_ns, recorded_ns FROM files WHERE graph = ?1 AND path = ?2",
            params![graph, path],
            file_from_row,
        )
        .optional())
}

pub fn list_files(conn: &Connection, graph: &str) -> Result<Vec<FileRecord>> {
    let mut stmt = db(conn.prepare_cached(
        "SELECT path, blob, size, mtime_ns, recorded_ns FROM files WHERE graph = ?1 ORDER BY path",
    ))?;
    let rows = db(stmt.query_map([graph], file_from_row))?;
    db(rows.collect())
}

pub fn put_file(conn: &Connection, graph: &str, f: &FileRecord) -> Result<()> {
    db(conn
        .prepare_cached(
            "INSERT INTO files (graph, path, blob, size, mtime_ns, recorded_ns) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (graph, path) DO UPDATE SET blob = excluded.blob, size = excluded.size,
             mtime_ns = excluded.mtime_ns, recorded_ns = excluded.recorded_ns",
        )
        .and_then(|mut s| {
            s.execute(params![graph, f.path, f.blob, f.size as i64, f.mtime_ns, f.recorded_ns])
        }))?;
    Ok(())
}

pub fn delete_file(conn: &Connection, graph: &str, path: &str) -> Result<()> {
    db(conn.execute(
        "DELETE FROM files WHERE graph = ?1 AND path = ?2",
        params![graph, path],
    ))?;
    Ok(())
}

pub fn clear_files(conn: &Connection, graph: &str) -> Result<()> {
    db(conn.execute("DELETE FROM files WHERE graph = ?1", [graph]))?;
    Ok(())
}

pub fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    db(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional())
}

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    db(conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    ))?;
    Ok(())
}

pub fn meta_prefix(conn: &Connection, prefix: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = db(conn.prepare_cached(
        "SELECT key, value FROM meta WHERE substr(key, 1, length(?1)) = ?1 ORDER BY key",
    ))?;
    let rows = db(stmt.query_map([prefix], |r| Ok((r.get(0)?, r.get(1)?))))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(db(row)?);
    }
    Ok(out)
}

pub fn delete_meta(conn: &Connection, key: &str) -> Result<()> {
    db(conn.execute("DELETE FROM meta WHERE key = ?1", [key]))?;
    Ok(())
}

/// Delete every node, edge and operation of a graph (the first step of a re-fold).
pub fn clear_graph(conn: &Connection, graph: &str) -> Result<()> {
    db(conn.execute("DELETE FROM edges WHERE graph = ?1", [graph]))?;
    db(conn.execute("DELETE FROM nodes WHERE graph = ?1", [graph]))?;
    db(conn.execute("DELETE FROM ops WHERE graph = ?1", [graph]))?;
    Ok(())
}

/// Dangling references and engine-level corruption.
pub fn integrity_check(conn: &Connection, graph: &str) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let result: String = db(conn.query_row("PRAGMA integrity_check", [], |r| r.get(0)))?;
    if result != "ok" {
        problems.push(format!("database integrity check failed: {result}"));
    }
    let mut stmt = db(conn.prepare(
        "SELECT e.id, e.type FROM edges e WHERE e.graph = ?1 AND e.removed = 0 AND (
            NOT EXISTS (SELECT 1 FROM nodes n WHERE n.graph = e.graph AND n.id = e.src)
            OR (e.type != 'evidences' AND NOT EXISTS (SELECT 1 FROM nodes n WHERE n.graph = e.graph AND n.id = e.dst))
            OR (e.type = 'evidences' AND NOT EXISTS (SELECT 1 FROM edges x WHERE x.graph = e.graph AND x.id = e.dst))
         ) ORDER BY e.id",
    ))?;
    let dangling = db(stmt.query_map([graph], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    }))?;
    for row in dangling {
        let (id, etype) = db(row)?;
        problems.push(format!(
            "{etype} edge {id} refers to something that does not exist"
        ));
    }
    Ok(problems)
}
