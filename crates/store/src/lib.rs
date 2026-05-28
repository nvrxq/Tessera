//! SQLite-backed persistence for Tessera.

pub mod activity;
pub mod extras;
pub mod migrations;
pub mod projects;
pub mod workspaces;

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    apply_pragmas(&conn, /* on_disk */ true)?;
    migrations::apply(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    apply_pragmas(&conn, /* on_disk */ false)?;
    migrations::apply(&conn)?;
    Ok(conn)
}

/// One-shot connection tuning. WAL + relaxed sync + a generous mmap window
/// turn pomodoro/tasks/links UPSERTs from ~5–10 ms (default
/// rollback-journal plus FULL fsync) into ~200 µs without changing
/// durability guarantees in any way the user notices: NORMAL still survives
/// a crash, only an OS-level power loss can lose the last commit.
///
/// `on_disk = false` for `open_in_memory()` — WAL and mmap are no-ops there
/// and `busy_timeout` is moot, but the other knobs (cache, temp_store) still
/// help the test suite a little.
fn apply_pragmas(conn: &Connection, on_disk: bool) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    if on_disk {
        conn.pragma_update(None, "journal_mode", "WAL")?;
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    if on_disk {
        // 256 MiB mmap window — SQLite caps to file size, so this just sets
        // the ceiling. ~zero RSS overhead until pages are actually touched.
        conn.pragma_update(None, "mmap_size", 268_435_456_i64)?;
    }
    // Negative = KiB, so -20000 ≈ 20 MiB of page cache.
    conn.pragma_update(None, "cache_size", -20_000_i64)?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    if on_disk {
        conn.pragma_update(None, "busy_timeout", 5_000_i64)?;
    }
    Ok(())
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
        assert_eq!(version, 8);
    }

    /// On-disk `open()` must enable WAL + the rest of the tuning pragmas.
    /// In-memory DBs can't be WAL (SQLite silently keeps them in `memory`
    /// mode), so we exercise this with a real temp file.
    #[test]
    fn on_disk_open_sets_perf_pragmas() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.db");
        let conn = open(&path).unwrap();

        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");

        let sync: i64 = conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap();
        // NORMAL = 1
        assert_eq!(sync, 1);

        let temp_store: i64 = conn
            .query_row("PRAGMA temp_store", [], |r| r.get(0))
            .unwrap();
        // MEMORY = 2
        assert_eq!(temp_store, 2);

        let busy: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(busy, 5_000);

        // cache_size is stored as a signed number of pages; we set
        // -20000 (≈ 20 MiB). SQLite returns it unchanged.
        let cache: i64 = conn
            .query_row("PRAGMA cache_size", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cache, -20_000);
    }
}
