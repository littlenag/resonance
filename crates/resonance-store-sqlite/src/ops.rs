//! The operation log, written in the same transaction as each change set (MVP-010).

use resonance_core::error::Result;
use resonance_core::ops::Op;
use resonance_core::store::OpRecord;
use rusqlite::{Connection, params};

use crate::db;

pub fn append(conn: &Connection, graph: &str, op: &Op) -> Result<()> {
    db(conn
        .prepare_cached(
            "INSERT INTO ops (graph, op_id, kind, actor, hlc, change_set, schema, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (op_id) DO NOTHING",
        )
        .and_then(|mut s| {
            s.execute(params![
                graph,
                op.op_id,
                op.kind,
                serde_json::to_string(&op.actor).unwrap_or_default(),
                op.hlc,
                op.change_set,
                op.schema,
                serde_json::to_string(&op.payload).unwrap_or_default(),
            ])
        }))?;
    Ok(())
}

pub fn after(conn: &Connection, graph: &str, after: u64, limit: usize) -> Result<Vec<OpRecord>> {
    let mut stmt = db(conn.prepare_cached(
        "SELECT seq, op_id, kind, actor, hlc, change_set, schema, payload FROM ops
         WHERE graph = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
    ))?;
    let rows = db(
        stmt.query_map(params![graph, after as i64, limit as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, u32>(6)?,
                r.get::<_, String>(7)?,
            ))
        }),
    )?;
    let mut out = Vec::new();
    for row in rows {
        let (seq, op_id, kind, actor, hlc, change_set, schema, payload) = db(row)?;
        out.push(OpRecord {
            seq: seq as u64,
            op: Op {
                op_id,
                kind,
                actor: serde_json::from_str(&actor)?,
                hlc,
                change_set,
                schema,
                payload: serde_json::from_str(&payload)?,
            },
        });
    }
    Ok(out)
}
