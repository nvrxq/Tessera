use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

struct Migration {
    version: i64,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../migrations/0001_init.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("../migrations/0002_task_slot.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("../migrations/0003_dangerous_flag.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("../migrations/0004_session_continue.sql"),
    },
    Migration {
        version: 5,
        sql: include_str!("../migrations/0005_projects_and_ordering.sql"),
    },
    Migration {
        version: 6,
        sql: include_str!("../migrations/0006_workspace_extras.sql"),
    },
    Migration {
        version: 7,
        sql: include_str!("../migrations/0007_global_pomodoro.sql"),
    },
    Migration {
        version: 8,
        sql: include_str!("../migrations/0008_activity_log.sql"),
    },
    Migration {
        version: 9,
        sql: include_str!("../migrations/0009_claude_session_and_archive.sql"),
    },
    Migration {
        version: 10,
        sql: include_str!("../migrations/0010_pomodoro_paused_from.sql"),
    },
    Migration {
        version: 11,
        sql: include_str!("../migrations/0011_reset_claude_session_pins.sql"),
    },
    Migration {
        version: 12,
        sql: include_str!("../migrations/0012_pomodoro_paused_from_backfill.sql"),
    },
];

pub fn apply(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (\
            version INTEGER PRIMARY KEY,\
            applied_at TEXT NOT NULL\
        );",
    )?;

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    for m in MIGRATIONS {
        if m.version <= current {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(m.sql)?;
        tx.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)",
            (m.version, Utc::now().to_rfc3339()),
        )?;
        tx.commit()?;
        tracing::info!(version = m.version, "migration applied");
    }

    Ok(())
}
