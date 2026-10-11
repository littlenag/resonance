//! Embedded migrations and schema versioning; refuses newer schemas (FR-006, FR-258).

use resonance_core::error::{Error, Result};
use resonance_core::store::SCHEMA_VERSION;
use rusqlite::{Connection, OptionalExtension};

use crate::db;

/// Migrations in order; migration `i` brings the schema from version `i` to `i + 1`.
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_initial.sql")];

const _: () = assert!(MIGRATIONS.len() == SCHEMA_VERSION as usize);

/// The schema version recorded in the database; 0 for an empty database.
pub fn version(conn: &Connection) -> Result<u32> {
    let has_meta: bool = db(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
            [],
            |_| Ok(true),
        )
        .optional())?
    .unwrap_or(false);
    if !has_meta {
        return Ok(0);
    }
    let v: Option<String> = db(conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .optional())?;
    match v {
        None => Ok(0),
        Some(v) => v
            .parse()
            .map_err(|_| Error::Store(format!("unreadable schema version `{v}`"))),
    }
}

/// Bring the database up to [`SCHEMA_VERSION`], refusing one that is newer.
pub fn migrate(conn: &mut Connection) -> Result<u32> {
    let current = version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(Error::NewerSchema {
            found: current,
            supported: SCHEMA_VERSION,
        });
    }
    if current == SCHEMA_VERSION {
        return Ok(current);
    }
    let tx = db(conn.transaction())?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        db(tx.execute_batch(sql))?;
        db(tx.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [(i as u32 + 1).to_string()],
        ))?;
    }
    db(tx.commit())?;
    Ok(SCHEMA_VERSION)
}
