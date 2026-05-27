use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use tessera_core::{SetupStatus, Workspace};
use uuid::Uuid;

pub fn insert(conn: &Connection, ws: &Workspace) -> Result<()> {
    let setup_json = serde_json::to_string(&ws.setup_status)?;
    conn.execute(
        "INSERT INTO workspaces \
            (id, name, repo_path, worktree_path, branch, created_at, setup_status, \
             detected_worktree, detected_branch, dangerous_skip_permissions, has_prior_session, \
             project_id, sort_order) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            ws.id.to_string(),
            ws.name,
            ws.repo_path.to_string_lossy(),
            ws.worktree_path.to_string_lossy(),
            ws.branch,
            ws.created_at.to_rfc3339(),
            setup_json,
            ws.detected_worktree
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            ws.detected_branch,
            ws.dangerous_skip_permissions as i64,
            ws.has_prior_session as i64,
            ws.project_id.map(|p| p.to_string()),
            ws.sort_order,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: Uuid) -> Result<Option<Workspace>> {
    conn.query_row(
        "SELECT id, name, repo_path, worktree_path, branch, created_at, setup_status, \
                detected_worktree, detected_branch, dangerous_skip_permissions, has_prior_session, \
                project_id, sort_order \
         FROM workspaces WHERE id = ?1",
        params![id.to_string()],
        row_to_workspace,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list(conn: &Connection) -> Result<Vec<Workspace>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, repo_path, worktree_path, branch, created_at, setup_status, \
                detected_worktree, detected_branch, dangerous_skip_permissions, has_prior_session, \
                project_id, sort_order \
         FROM workspaces ORDER BY sort_order ASC, created_at ASC",
    )?;
    let rows = stmt.query_map([], row_to_workspace)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Apply a batch of `(workspace_id, sort_order)` updates atomically. Unknown
/// IDs are silently skipped so a stale frontend reorder doesn't poison the
/// whole transaction.
pub fn update_sort_orders(conn: &Connection, updates: &[(Uuid, i64)]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare("UPDATE workspaces SET sort_order = ?1 WHERE id = ?2")?;
        for (id, order) in updates {
            stmt.execute(params![order, id.to_string()])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Set or clear the project a workspace belongs to. `None` un-assigns.
pub fn update_project(conn: &Connection, ws_id: Uuid, project_id: Option<Uuid>) -> Result<()> {
    let n = conn.execute(
        "UPDATE workspaces SET project_id = ?1 WHERE id = ?2",
        params![project_id.map(|p| p.to_string()), ws_id.to_string()],
    )?;
    anyhow::ensure!(n == 1, "workspace {ws_id} not found");
    Ok(())
}

pub fn mark_session_started(conn: &Connection, id: Uuid) -> Result<()> {
    let n = conn.execute(
        "UPDATE workspaces SET has_prior_session = 1 WHERE id = ?1",
        params![id.to_string()],
    )?;
    anyhow::ensure!(n == 1, "workspace {id} not found");
    Ok(())
}

pub fn update_setup_status(conn: &Connection, id: Uuid, status: &SetupStatus) -> Result<()> {
    let j = serde_json::to_string(status)?;
    let n = conn.execute(
        "UPDATE workspaces SET setup_status = ?1 WHERE id = ?2",
        params![j, id.to_string()],
    )?;
    anyhow::ensure!(n == 1, "workspace {id} not found");
    Ok(())
}

pub fn update_detected_worktree(
    conn: &Connection,
    id: Uuid,
    worktree: Option<&std::path::Path>,
    branch: Option<&str>,
) -> Result<()> {
    let n = conn.execute(
        "UPDATE workspaces SET detected_worktree = ?1, detected_branch = ?2 WHERE id = ?3",
        params![
            worktree.map(|p| p.to_string_lossy().into_owned()),
            branch,
            id.to_string()
        ],
    )?;
    anyhow::ensure!(n == 1, "workspace {id} not found");
    Ok(())
}

pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
    let n = conn.execute(
        "DELETE FROM workspaces WHERE id = ?1",
        params![id.to_string()],
    )?;
    anyhow::ensure!(n == 1, "workspace {id} not found");
    Ok(())
}

fn row_to_workspace(row: &rusqlite::Row<'_>) -> rusqlite::Result<Workspace> {
    let id_s: String = row.get(0)?;
    let created_s: String = row.get(5)?;
    let setup_s: String = row.get(6)?;
    let detected_wt: Option<String> = row.get(7)?;
    let detected_br: Option<String> = row.get(8)?;
    let dangerous: i64 = row.get(9)?;
    let has_prior: i64 = row.get(10)?;
    let project_id_s: Option<String> = row.get(11)?;
    let sort_order: i64 = row.get(12)?;
    let project_id = match project_id_s {
        Some(s) => Some(Uuid::parse_str(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, Box::new(e))
        })?),
        None => None,
    };
    Ok(Workspace {
        id: Uuid::parse_str(&id_s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        name: row.get(1)?,
        repo_path: PathBuf::from(row.get::<_, String>(2)?),
        worktree_path: PathBuf::from(row.get::<_, String>(3)?),
        branch: row.get(4)?,
        created_at: DateTime::parse_from_rfc3339(&created_s)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
            .with_timezone(&Utc),
        setup_status: serde_json::from_str(&setup_s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
        })?,
        detected_worktree: detected_wt.map(PathBuf::from),
        detected_branch: detected_br,
        dangerous_skip_permissions: dangerous != 0,
        has_prior_session: has_prior != 0,
        project_id,
        sort_order,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;

    fn sample(name: &str) -> Workspace {
        Workspace {
            id: Uuid::new_v4(),
            name: name.into(),
            repo_path: PathBuf::from("/tmp/repo"),
            worktree_path: PathBuf::from(format!("/tmp/wt-{name}")),
            branch: name.into(),
            created_at: Utc::now(),
            setup_status: SetupStatus::Pending,
            detected_worktree: None,
            detected_branch: None,
            dangerous_skip_permissions: false,
            has_prior_session: false,
            project_id: None,
            sort_order: 0,
        }
    }

    #[test]
    fn insert_then_get() {
        let conn = open_in_memory().unwrap();
        let ws = sample("a");
        insert(&conn, &ws).unwrap();
        let got = get(&conn, ws.id).unwrap().unwrap();
        assert_eq!(got.id, ws.id);
        assert_eq!(got.branch, "a");
    }

    #[test]
    fn list_returns_inserted_in_order() {
        let conn = open_in_memory().unwrap();
        insert(&conn, &sample("a")).unwrap();
        insert(&conn, &sample("b")).unwrap();
        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn update_status_persists() {
        let conn = open_in_memory().unwrap();
        let ws = sample("a");
        insert(&conn, &ws).unwrap();
        update_setup_status(&conn, ws.id, &SetupStatus::Ok).unwrap();
        let got = get(&conn, ws.id).unwrap().unwrap();
        assert_eq!(got.setup_status, SetupStatus::Ok);
    }

    #[test]
    fn delete_removes_row() {
        let conn = open_in_memory().unwrap();
        let ws = sample("a");
        insert(&conn, &ws).unwrap();
        delete(&conn, ws.id).unwrap();
        assert!(get(&conn, ws.id).unwrap().is_none());
    }

    #[test]
    fn update_detected_worktree_persists() {
        let conn = open_in_memory().unwrap();
        let ws = sample("p");
        insert(&conn, &ws).unwrap();
        update_detected_worktree(
            &conn,
            ws.id,
            Some(&PathBuf::from("/tmp/wt-x")),
            Some("feat/x"),
        )
        .unwrap();
        let got = get(&conn, ws.id).unwrap().unwrap();
        assert_eq!(got.detected_worktree, Some(PathBuf::from("/tmp/wt-x")));
        assert_eq!(got.detected_branch, Some("feat/x".to_string()));
    }

    #[test]
    fn dangerous_flag_round_trips() {
        let conn = open_in_memory().unwrap();
        let mut ws = sample("d");
        ws.dangerous_skip_permissions = true;
        insert(&conn, &ws).unwrap();
        let got = get(&conn, ws.id).unwrap().unwrap();
        assert!(got.dangerous_skip_permissions);
    }

    #[test]
    fn list_orders_by_sort_order_then_created_at() {
        let conn = open_in_memory().unwrap();
        let mut a = sample("a");
        a.sort_order = 200;
        let mut b = sample("b");
        b.sort_order = 100;
        let mut c = sample("c");
        c.sort_order = 100;
        // a inserted first; b and c tie on sort_order, b inserted before c
        // so b.created_at < c.created_at and b should come first.
        insert(&conn, &a).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert(&conn, &b).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        insert(&conn, &c).unwrap();
        let all = list(&conn).unwrap();
        let names: Vec<&str> = all.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["b", "c", "a"]);
    }

    #[test]
    fn update_sort_orders_applies_atomically() {
        let conn = open_in_memory().unwrap();
        let a = sample("a");
        let b = sample("b");
        insert(&conn, &a).unwrap();
        insert(&conn, &b).unwrap();
        update_sort_orders(&conn, &[(a.id, 500), (b.id, 250)]).unwrap();
        let got_a = get(&conn, a.id).unwrap().unwrap();
        let got_b = get(&conn, b.id).unwrap().unwrap();
        assert_eq!(got_a.sort_order, 500);
        assert_eq!(got_b.sort_order, 250);
    }

    #[test]
    fn update_project_sets_and_clears() {
        let conn = open_in_memory().unwrap();
        let ws = sample("w");
        insert(&conn, &ws).unwrap();
        let project = tessera_core::Project {
            id: Uuid::new_v4(),
            name: "p".into(),
            accent: None,
            created_at: Utc::now(),
        };
        crate::projects::insert(&conn, &project).unwrap();

        update_project(&conn, ws.id, Some(project.id)).unwrap();
        let got = get(&conn, ws.id).unwrap().unwrap();
        assert_eq!(got.project_id, Some(project.id));

        update_project(&conn, ws.id, None).unwrap();
        let got = get(&conn, ws.id).unwrap().unwrap();
        assert_eq!(got.project_id, None);
    }

    #[test]
    fn deleting_project_sets_workspace_project_id_to_null() {
        let conn = open_in_memory().unwrap();
        let ws = sample("w");
        insert(&conn, &ws).unwrap();
        let project = tessera_core::Project {
            id: Uuid::new_v4(),
            name: "p".into(),
            accent: None,
            created_at: Utc::now(),
        };
        crate::projects::insert(&conn, &project).unwrap();
        update_project(&conn, ws.id, Some(project.id)).unwrap();

        crate::projects::delete(&conn, project.id).unwrap();
        let got = get(&conn, ws.id).unwrap().unwrap();
        assert_eq!(got.project_id, None);
    }
}
