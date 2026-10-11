//! Content-addressed blob storage.

use resonance_core::error::Result;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db;

pub fn get(conn: &Connection, hash: &str) -> Result<Option<Vec<u8>>> {
    let bytes: Option<Option<Vec<u8>>> = db(conn
        .query_row("SELECT bytes FROM blobs WHERE hash = ?1", [hash], |r| {
            r.get(0)
        })
        .optional())?;
    Ok(bytes.flatten())
}

pub fn has(conn: &Connection, hash: &str) -> Result<bool> {
    Ok(db(conn
        .query_row("SELECT 1 FROM blobs WHERE hash = ?1", [hash], |_| Ok(()))
        .optional())?
    .is_some())
}

pub fn put(conn: &Connection, hash: &str, size: u64, bytes: Option<&[u8]>) -> Result<()> {
    // Content-addressed: an existing blob is never replaced, except to add missing bytes.
    db(conn.execute(
        "INSERT INTO blobs (hash, size, bytes) VALUES (?1, ?2, ?3)
         ON CONFLICT (hash) DO UPDATE SET bytes = coalesce(blobs.bytes, excluded.bytes)",
        params![hash, size as i64, bytes],
    ))?;
    Ok(())
}
