//! Persistence for `activity_log` — one row per hook event received from
//! Claude (PostToolUse / Stop / Notification). Read by the workspace's
//! "Activity" tab. We cap at ~200 rows per workspace via `prune_keep_n`
//! so a long-running agent doesn't grow the DB without bound.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use tessera_core::{ActivityEntry, ActivityKind};
use uuid::Uuid;

pub fn insert(conn: &Connection, entry: &ActivityEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO activity_log (id, workspace_id, kind, summary, payload, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            entry.id.to_string(),
            entry.workspace_id.to_string(),
            entry.kind.as_str(),
            entry.summary,
            entry.payload,
            entry.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn list_recent(
    conn: &Connection,
    workspace_id: Uuid,
    limit: i64,
) -> Result<Vec<ActivityEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, kind, summary, payload, created_at \
         FROM activity_log \
         WHERE workspace_id = ?1 \
         ORDER BY created_at DESC \
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![workspace_id.to_string(), limit], row_to_entry)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Keep the newest `n` rows for `workspace_id`; delete everything older.
/// Done as a single `DELETE … WHERE id NOT IN (SELECT id … LIMIT n)` so
/// we don't need to materialise all rows in Rust to figure out which to drop.
pub fn prune_keep_n(conn: &Connection, workspace_id: Uuid, n: i64) -> Result<usize> {
    let affected = conn.execute(
        "DELETE FROM activity_log \
         WHERE workspace_id = ?1 \
         AND id NOT IN (\
            SELECT id FROM activity_log \
            WHERE workspace_id = ?1 \
            ORDER BY created_at DESC \
            LIMIT ?2\
         )",
        params![workspace_id.to_string(), n],
    )?;
    Ok(affected)
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActivityEntry> {
    let id_s: String = row.get(0)?;
    let ws_s: String = row.get(1)?;
    let kind_s: String = row.get(2)?;
    let created_s: String = row.get(5)?;
    let kind = ActivityKind::parse(&kind_s).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            format!("unknown activity kind {kind_s:?}").into(),
        )
    })?;
    Ok(ActivityEntry {
        id: Uuid::parse_str(&id_s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        workspace_id: Uuid::parse_str(&ws_s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
        })?,
        kind,
        summary: row.get(3)?,
        payload: row.get(4)?,
        created_at: DateTime::parse_from_rfc3339(&created_s)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
            .with_timezone(&Utc),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;
    use std::path::PathBuf;
    use tessera_core::{SetupStatus, Workspace};

    fn seed_workspace(conn: &Connection) -> Uuid {
        let ws = Workspace {
            id: Uuid::new_v4(),
            name: "w".into(),
            repo_path: PathBuf::from("/tmp/r"),
            worktree_path: PathBuf::from("/tmp/wt"),
            branch: String::new(),
            created_at: Utc::now(),
            setup_status: SetupStatus::Ok,
            detected_worktree: None,
            detected_branch: None,
            dangerous_skip_permissions: false,
            has_prior_session: false,
            project_id: None,
            sort_order: 0,
            claude_session_id: None,
            archived_at: None,
        };
        crate::workspaces::insert(conn, &ws).unwrap();
        ws.id
    }

    fn make(ws_id: Uuid, kind: ActivityKind, summary: &str) -> ActivityEntry {
        ActivityEntry {
            id: Uuid::new_v4(),
            workspace_id: ws_id,
            kind,
            summary: summary.into(),
            payload: "{}".into(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn insert_then_list_orders_newest_first() {
        let conn = open_in_memory().unwrap();
        let ws_id = seed_workspace(&conn);
        let a = make(ws_id, ActivityKind::PostToolUse, "a");
        insert(&conn, &a).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = make(ws_id, ActivityKind::Stop, "b");
        insert(&conn, &b).unwrap();
        let all = list_recent(&conn, ws_id, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].summary, "b");
        assert_eq!(all[1].summary, "a");
    }

    #[test]
    fn prune_keeps_only_the_newest_n() {
        let conn = open_in_memory().unwrap();
        let ws_id = seed_workspace(&conn);
        for i in 0..5 {
            insert(
                &conn,
                &make(ws_id, ActivityKind::PostToolUse, &format!("e{i}")),
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let dropped = prune_keep_n(&conn, ws_id, 2).unwrap();
        assert_eq!(dropped, 3);
        let remaining = list_recent(&conn, ws_id, 10).unwrap();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].summary, "e4");
        assert_eq!(remaining[1].summary, "e3");
    }
}
