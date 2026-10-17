//! Snapshot files (FR-074, FR-152, FR-165, MVP-027).
//!
//! A snapshot is a separate SQLite file holding its header and the operations it folded, in
//! fold order. It is written deterministically, so that two clones compacting the same history
//! produce byte-identical files: a fresh in-memory database with a fixed page size and the
//! rollback journal, rows inserted in a fixed order, then `VACUUM INTO` a temporary file that
//! is renamed into place. It is never the live store (FR-169).

use std::fs;
use std::path::Path;

use resonance_core::error::{Error, Result};
use resonance_core::history::canonical;
use resonance_core::ops::Op;
use resonance_core::store::{Snapshot, SnapshotMeta};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::db;

/// The format identifier of snapshots written by this adapter (FR-165).
pub const FORMAT: &str = "resonance-snapshot-sqlite";
/// The format version this build writes, and the newest it reads.
pub const FORMAT_VERSION: u32 = 1;

const SCHEMA: &str = "
CREATE TABLE snapshot_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE ops (seq INTEGER PRIMARY KEY, op_id TEXT NOT NULL UNIQUE, op TEXT NOT NULL);
";

pub fn write(path: &Path, snapshot: &Snapshot) -> Result<()> {
    let conn = db(Connection::open_in_memory())?;
    db(conn.pragma_update(None, "page_size", 4096))?;
    db(conn.execute_batch(SCHEMA))?;
    let m = &snapshot.meta;
    let entries = [
        ("epoch", m.epoch.clone()),
        ("folded_deltas", canonical::to_string(&m.folded_deltas)?),
        ("format", FORMAT.to_string()),
        ("format_version", FORMAT_VERSION.to_string()),
        ("schema_version", m.schema_version.to_string()),
    ];
    let tx = db(conn.unchecked_transaction())?;
    for (k, v) in entries {
        db(tx.execute(
            "INSERT INTO snapshot_meta (key, value) VALUES (?1, ?2)",
            params![k, v],
        ))?;
    }
    {
        let mut stmt = db(tx.prepare("INSERT INTO ops (seq, op_id, op) VALUES (?1, ?2, ?3)"))?;
        for (i, op) in snapshot.ops.iter().enumerate() {
            db(stmt.execute(params![i as i64 + 1, op.op_id, canonical::to_string(op)?]))?;
        }
    }
    db(tx.commit())?;
    let tmp = path.with_extension("db.tmp");
    let _ = fs::remove_file(&tmp);
    let target = tmp
        .to_str()
        .ok_or_else(|| Error::Invalid("the snapshot path is not valid UTF-8".into()))?;
    db(conn.execute("VACUUM INTO ?1", [target]))?;
    fs::rename(&tmp, path).map_err(|e| Error::io(format!("writing {}", path.display()), e))
}

pub fn read(path: &Path) -> Result<Snapshot> {
    let conn = db(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    ))?;
    let has_meta: Option<i64> = db(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'snapshot_meta'",
            [],
            |r| r.get(0),
        )
        .optional())?;
    if has_meta.is_none() {
        return Err(Error::Invalid(format!(
            "{} is not a Resonance snapshot",
            path.display()
        )));
    }
    let get = |k: &str| -> Result<String> {
        db(conn
            .query_row("SELECT value FROM snapshot_meta WHERE key = ?1", [k], |r| {
                r.get(0)
            })
            .optional())?
        .ok_or_else(|| Error::Invalid(format!("the snapshot has no `{k}`")))
    };
    let format = get("format")?;
    if format != FORMAT {
        return Err(Error::Invalid(format!(
            "{} has format `{format}`, which this adapter does not read",
            path.display()
        )));
    }
    let format_version: u32 = get("format_version")?
        .parse()
        .map_err(|_| Error::Invalid("unreadable snapshot format version".into()))?;
    if format_version > FORMAT_VERSION {
        return Err(Error::Invalid(format!(
            "the snapshot uses format version {format_version}, newer than this Resonance reads ({FORMAT_VERSION}); upgrade resonance"
        )));
    }
    let meta = SnapshotMeta {
        format,
        format_version,
        schema_version: get("schema_version")?
            .parse()
            .map_err(|_| Error::Invalid("unreadable snapshot schema version".into()))?,
        epoch: get("epoch")?,
        folded_deltas: serde_json::from_str(&get("folded_deltas")?)?,
    };
    let mut stmt = db(conn.prepare("SELECT op FROM ops ORDER BY seq"))?;
    let rows = db(stmt.query_map([], |r| r.get::<_, String>(0)))?;
    let mut ops: Vec<Op> = Vec::new();
    for row in rows {
        ops.push(serde_json::from_str(&db(row)?)?);
    }
    Ok(Snapshot { meta, ops })
}
