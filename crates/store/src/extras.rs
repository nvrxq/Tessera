//! CRUD for workspace extras: links, daily tasks, pomodoro state.
//!
//! All three tables key on `workspace_id` with `ON DELETE CASCADE` so the
//! Rust side never has to manually clean up when a workspace is removed.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tessera_core::{LinkKind, PomodoroMode, PomodoroState, WorkspaceLink, WorkspaceTask};
use uuid::Uuid;

// ---- links ----

pub fn insert_link(conn: &Connection, link: &WorkspaceLink) -> Result<()> {
    conn.execute(
        "INSERT INTO workspace_links (id, workspace_id, label, url, kind, created_at, sort_order) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            link.id.to_string(),
            link.workspace_id.to_string(),
            link.label,
            link.url,
            link.kind.as_str(),
            link.created_at.to_rfc3339(),
            link.sort_order,
        ],
    )?;
    Ok(())
}

pub fn list_links(conn: &Connection, workspace_id: Uuid) -> Result<Vec<WorkspaceLink>> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, label, url, kind, created_at, sort_order \
         FROM workspace_links WHERE workspace_id = ?1 \
         ORDER BY sort_order ASC, created_at ASC",
    )?;
    let rows = stmt.query_map(params![workspace_id.to_string()], row_to_link)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn delete_link(conn: &Connection, id: Uuid) -> Result<()> {
    let n = conn.execute(
        "DELETE FROM workspace_links WHERE id = ?1",
        params![id.to_string()],
    )?;
    anyhow::ensure!(n == 1, "link {id} not found");
    Ok(())
}

/// Apply a batch of `(link_id, sort_order)` updates atomically. Mirrors
/// `workspaces::update_sort_orders` — silently skips unknown ids.
pub fn update_link_sort_orders(conn: &Connection, updates: &[(Uuid, i64)]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt =
            tx.prepare("UPDATE workspace_links SET sort_order = ?1 WHERE id = ?2")?;
        for (id, order) in updates {
            stmt.execute(params![order, id.to_string()])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Next available `sort_order` value for a workspace's links — gapped by
/// 10 so single-row inserts can slot between neighbours.
pub fn next_link_sort_order(conn: &Connection, workspace_id: Uuid) -> Result<i64> {
    let v: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 10 FROM workspace_links WHERE workspace_id = ?1",
            params![workspace_id.to_string()],
            |r| r.get(0),
        )
        .unwrap_or(10);
    Ok(v)
}

fn row_to_link(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceLink> {
    let id_s: String = row.get(0)?;
    let ws_s: String = row.get(1)?;
    let kind_s: String = row.get(4)?;
    let created_s: String = row.get(5)?;
    Ok(WorkspaceLink {
        id: parse_uuid(&id_s, 0)?,
        workspace_id: parse_uuid(&ws_s, 1)?,
        label: row.get(2)?,
        url: row.get(3)?,
        kind: LinkKind::from_str(&kind_s),
        created_at: parse_dt(&created_s, 5)?,
        sort_order: row.get(6)?,
    })
}

// ---- tasks ----

pub fn insert_task(conn: &Connection, task: &WorkspaceTask) -> Result<()> {
    conn.execute(
        "INSERT INTO workspace_tasks (id, workspace_id, title, done, sort_order, due_date, created_at, completed_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            task.id.to_string(),
            task.workspace_id.to_string(),
            task.title,
            task.done as i64,
            task.sort_order,
            task.due_date,
            task.created_at.to_rfc3339(),
            task.completed_at.map(|d| d.to_rfc3339()),
        ],
    )?;
    Ok(())
}

pub fn list_tasks(
    conn: &Connection,
    workspace_id: Uuid,
    include_completed: bool,
) -> Result<Vec<WorkspaceTask>> {
    let sql = if include_completed {
        "SELECT id, workspace_id, title, done, sort_order, due_date, created_at, completed_at \
         FROM workspace_tasks WHERE workspace_id = ?1 \
         ORDER BY done ASC, sort_order ASC, created_at ASC"
    } else {
        "SELECT id, workspace_id, title, done, sort_order, due_date, created_at, completed_at \
         FROM workspace_tasks WHERE workspace_id = ?1 AND done = 0 \
         ORDER BY sort_order ASC, created_at ASC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![workspace_id.to_string()], row_to_task)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn get_task(conn: &Connection, id: Uuid) -> Result<Option<WorkspaceTask>> {
    conn.query_row(
        "SELECT id, workspace_id, title, done, sort_order, due_date, created_at, completed_at \
         FROM workspace_tasks WHERE id = ?1",
        params![id.to_string()],
        row_to_task,
    )
    .optional()
    .map_err(Into::into)
}

/// Toggle a task's `done` flag. Sets/clears `completed_at` accordingly.
pub fn toggle_task(conn: &Connection, id: Uuid) -> Result<bool> {
    let current = get_task(conn, id)?
        .ok_or_else(|| anyhow::anyhow!("task {id} not found"))?;
    let new_done = !current.done;
    let completed_at = if new_done {
        Some(Utc::now().to_rfc3339())
    } else {
        None
    };
    conn.execute(
        "UPDATE workspace_tasks SET done = ?1, completed_at = ?2 WHERE id = ?3",
        params![new_done as i64, completed_at, id.to_string()],
    )?;
    Ok(new_done)
}

pub fn delete_task(conn: &Connection, id: Uuid) -> Result<()> {
    let n = conn.execute(
        "DELETE FROM workspace_tasks WHERE id = ?1",
        params![id.to_string()],
    )?;
    anyhow::ensure!(n == 1, "task {id} not found");
    Ok(())
}

pub fn update_task_sort_orders(conn: &Connection, updates: &[(Uuid, i64)]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt =
            tx.prepare("UPDATE workspace_tasks SET sort_order = ?1 WHERE id = ?2")?;
        for (id, order) in updates {
            stmt.execute(params![order, id.to_string()])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn next_task_sort_order(conn: &Connection, workspace_id: Uuid) -> Result<i64> {
    let v: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 10 FROM workspace_tasks WHERE workspace_id = ?1",
            params![workspace_id.to_string()],
            |r| r.get(0),
        )
        .unwrap_or(10);
    Ok(v)
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceTask> {
    let id_s: String = row.get(0)?;
    let ws_s: String = row.get(1)?;
    let done: i64 = row.get(3)?;
    let due_date: Option<String> = row.get(5)?;
    let created_s: String = row.get(6)?;
    let completed_s: Option<String> = row.get(7)?;
    Ok(WorkspaceTask {
        id: parse_uuid(&id_s, 0)?,
        workspace_id: parse_uuid(&ws_s, 1)?,
        title: row.get(2)?,
        done: done != 0,
        sort_order: row.get(4)?,
        due_date,
        created_at: parse_dt(&created_s, 6)?,
        completed_at: match completed_s {
            Some(s) => Some(parse_dt(&s, 7)?),
            None => None,
        },
    })
}

// ---- pomodoro ----

/// Returns the pomodoro row for a workspace. If none exists, returns
/// `Ok(None)` so the caller can synthesise an `idle` state without
/// committing it to disk yet.
pub fn get_pomodoro(conn: &Connection, workspace_id: Uuid) -> Result<Option<PomodoroState>> {
    conn.query_row(
        "SELECT workspace_id, mode, started_at, paused_at, target_seconds, \
                elapsed_seconds_before_pause, cycles_completed, updated_at \
         FROM workspace_pomodoro WHERE workspace_id = ?1",
        params![workspace_id.to_string()],
        row_to_pomodoro,
    )
    .optional()
    .map_err(Into::into)
}

/// UPSERT the pomodoro row. We always write a full row so transient frontend
/// state never drifts from disk.
pub fn upsert_pomodoro(conn: &Connection, state: &PomodoroState) -> Result<()> {
    conn.execute(
        "INSERT INTO workspace_pomodoro \
            (workspace_id, mode, started_at, paused_at, target_seconds, \
             elapsed_seconds_before_pause, cycles_completed, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(workspace_id) DO UPDATE SET \
            mode = excluded.mode, \
            started_at = excluded.started_at, \
            paused_at = excluded.paused_at, \
            target_seconds = excluded.target_seconds, \
            elapsed_seconds_before_pause = excluded.elapsed_seconds_before_pause, \
            cycles_completed = excluded.cycles_completed, \
            updated_at = excluded.updated_at",
        params![
            state.workspace_id.to_string(),
            state.mode.as_str(),
            state.started_at.map(|d| d.to_rfc3339()),
            state.paused_at.map(|d| d.to_rfc3339()),
            state.target_seconds,
            state.elapsed_seconds_before_pause,
            state.cycles_completed,
            state.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn row_to_pomodoro(row: &rusqlite::Row<'_>) -> rusqlite::Result<PomodoroState> {
    let ws_s: String = row.get(0)?;
    let mode_s: String = row.get(1)?;
    let started_s: Option<String> = row.get(2)?;
    let paused_s: Option<String> = row.get(3)?;
    let updated_s: String = row.get(7)?;
    Ok(PomodoroState {
        workspace_id: parse_uuid(&ws_s, 0)?,
        mode: PomodoroMode::from_str(&mode_s),
        started_at: match started_s {
            Some(s) => Some(parse_dt(&s, 2)?),
            None => None,
        },
        paused_at: match paused_s {
            Some(s) => Some(parse_dt(&s, 3)?),
            None => None,
        },
        target_seconds: row.get(4)?,
        elapsed_seconds_before_pause: row.get(5)?,
        cycles_completed: row.get(6)?,
        updated_at: parse_dt(&updated_s, 7)?,
    })
}

// ---- helpers ----

fn parse_uuid(s: &str, col: usize) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn parse_dt(s: &str, col: usize) -> rusqlite::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(s)
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(e))
        })?
        .with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;
    use std::path::PathBuf;
    use tessera_core::{SetupStatus, Workspace};

    fn make_workspace(conn: &Connection) -> Workspace {
        let ws = Workspace {
            id: Uuid::new_v4(),
            name: "host".into(),
            repo_path: PathBuf::from("/tmp/r"),
            worktree_path: PathBuf::from("/tmp/wt"),
            branch: "main".into(),
            created_at: Utc::now(),
            setup_status: SetupStatus::Ok,
            detected_worktree: None,
            detected_branch: None,
            dangerous_skip_permissions: false,
            has_prior_session: false,
            project_id: None,
            sort_order: 0,
        };
        crate::workspaces::insert(conn, &ws).unwrap();
        ws
    }

    #[test]
    fn link_round_trip_and_list_is_sort_ordered() {
        let conn = open_in_memory().unwrap();
        let ws = make_workspace(&conn);

        let a = WorkspaceLink {
            id: Uuid::new_v4(),
            workspace_id: ws.id,
            label: Some("a".into()),
            url: "https://example.com/a".into(),
            kind: LinkKind::Url,
            created_at: Utc::now(),
            sort_order: 20,
        };
        let b = WorkspaceLink {
            id: Uuid::new_v4(),
            workspace_id: ws.id,
            label: None,
            url: "https://github.com/o/r/issues/1".into(),
            kind: LinkKind::GithubIssue,
            created_at: Utc::now(),
            sort_order: 10,
        };
        insert_link(&conn, &a).unwrap();
        insert_link(&conn, &b).unwrap();

        let listed = list_links(&conn, ws.id).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, b.id);
        assert!(matches!(listed[0].kind, LinkKind::GithubIssue));
    }

    #[test]
    fn link_cascade_delete_with_workspace() {
        let conn = open_in_memory().unwrap();
        let ws = make_workspace(&conn);
        let link = WorkspaceLink {
            id: Uuid::new_v4(),
            workspace_id: ws.id,
            label: None,
            url: "https://x".into(),
            kind: LinkKind::Url,
            created_at: Utc::now(),
            sort_order: 0,
        };
        insert_link(&conn, &link).unwrap();
        crate::workspaces::delete(&conn, ws.id).unwrap();
        assert!(list_links(&conn, ws.id).unwrap().is_empty());
    }

    #[test]
    fn link_reorder_persists() {
        let conn = open_in_memory().unwrap();
        let ws = make_workspace(&conn);
        let a = WorkspaceLink {
            id: Uuid::new_v4(),
            workspace_id: ws.id,
            label: None,
            url: "u1".into(),
            kind: LinkKind::Url,
            created_at: Utc::now(),
            sort_order: 10,
        };
        let b = WorkspaceLink {
            id: Uuid::new_v4(),
            workspace_id: ws.id,
            label: None,
            url: "u2".into(),
            kind: LinkKind::Url,
            created_at: Utc::now(),
            sort_order: 20,
        };
        insert_link(&conn, &a).unwrap();
        insert_link(&conn, &b).unwrap();
        update_link_sort_orders(&conn, &[(a.id, 50), (b.id, 5)]).unwrap();
        let listed = list_links(&conn, ws.id).unwrap();
        assert_eq!(listed[0].id, b.id);
    }

    #[test]
    fn task_lifecycle_insert_toggle_delete() {
        let conn = open_in_memory().unwrap();
        let ws = make_workspace(&conn);
        let task = WorkspaceTask {
            id: Uuid::new_v4(),
            workspace_id: ws.id,
            title: "ship feature".into(),
            done: false,
            sort_order: next_task_sort_order(&conn, ws.id).unwrap(),
            due_date: Some("2026-05-30".into()),
            created_at: Utc::now(),
            completed_at: None,
        };
        insert_task(&conn, &task).unwrap();

        // toggle to done
        assert!(toggle_task(&conn, task.id).unwrap());
        let got = get_task(&conn, task.id).unwrap().unwrap();
        assert!(got.done);
        assert!(got.completed_at.is_some());

        // include_completed = false hides it
        assert!(list_tasks(&conn, ws.id, false).unwrap().is_empty());
        assert_eq!(list_tasks(&conn, ws.id, true).unwrap().len(), 1);

        // toggle back
        assert!(!toggle_task(&conn, task.id).unwrap());
        let got = get_task(&conn, task.id).unwrap().unwrap();
        assert!(!got.done);
        assert!(got.completed_at.is_none());

        delete_task(&conn, task.id).unwrap();
        assert!(get_task(&conn, task.id).unwrap().is_none());
    }

    #[test]
    fn task_cascade_delete_with_workspace() {
        let conn = open_in_memory().unwrap();
        let ws = make_workspace(&conn);
        let t = WorkspaceTask {
            id: Uuid::new_v4(),
            workspace_id: ws.id,
            title: "x".into(),
            done: false,
            sort_order: 0,
            due_date: None,
            created_at: Utc::now(),
            completed_at: None,
        };
        insert_task(&conn, &t).unwrap();
        crate::workspaces::delete(&conn, ws.id).unwrap();
        assert!(list_tasks(&conn, ws.id, true).unwrap().is_empty());
    }

    #[test]
    fn pomodoro_upsert_updates_in_place() {
        let conn = open_in_memory().unwrap();
        let ws = make_workspace(&conn);

        assert!(get_pomodoro(&conn, ws.id).unwrap().is_none());

        let mut s = PomodoroState::idle(ws.id);
        s.mode = PomodoroMode::Work;
        s.target_seconds = 1500;
        s.started_at = Some(Utc::now());
        upsert_pomodoro(&conn, &s).unwrap();

        let got = get_pomodoro(&conn, ws.id).unwrap().unwrap();
        assert!(matches!(got.mode, PomodoroMode::Work));

        // Second upsert mutates rather than duplicating.
        s.mode = PomodoroMode::Paused;
        s.elapsed_seconds_before_pause = 120;
        s.paused_at = Some(Utc::now());
        upsert_pomodoro(&conn, &s).unwrap();
        let got = get_pomodoro(&conn, ws.id).unwrap().unwrap();
        assert!(matches!(got.mode, PomodoroMode::Paused));
        assert_eq!(got.elapsed_seconds_before_pause, 120);
    }

    /// End-to-end: ensures the 0006 migration runs cleanly on a DB at the
    /// 0001..0005 state and exposes all three new tables.
    #[test]
    fn migration_creates_all_three_tables() {
        let conn = open_in_memory().unwrap();
        let names: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(names.contains(&"workspace_links".to_string()));
        assert!(names.contains(&"workspace_tasks".to_string()));
        assert!(names.contains(&"workspace_pomodoro".to_string()));
    }
}
