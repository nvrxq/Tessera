//! SQLite-backed persistence for Tessera.

pub mod migrations;
pub mod workspaces;

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    migrations::apply(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrations::apply(&conn)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_has_expected_tables() {
        let conn = open_in_memory().unwrap();
        let names: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"workspaces".to_string()));
        assert!(names.contains(&"agent_sessions".to_string()));
        assert!(names.contains(&"schema_version".to_string()));
    }

    #[test]
    fn migrations_are_idempotent() {
        let conn = open_in_memory().unwrap();
        // re-running migrations on an already-migrated DB must not fail
        migrations::apply(&conn).unwrap();
        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 3);
    }
}
