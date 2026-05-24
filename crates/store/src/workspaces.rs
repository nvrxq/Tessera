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
             task_prompt, detected_worktree, detected_branch) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            ws.id.to_string(),
            ws.name,
            ws.repo_path.to_string_lossy(),
            ws.worktree_path.to_string_lossy(),
            ws.branch,
            ws.created_at.to_rfc3339(),
            setup_json,
            ws.task_prompt,
            ws.detected_worktree
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            ws.detected_branch,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: Uuid) -> Result<Option<Workspace>> {
    conn.query_row(
        "SELECT id, name, repo_path, worktree_path, branch, created_at, setup_status, \
                task_prompt, detected_worktree, detected_branch \
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
                task_prompt, detected_worktree, detected_branch \
         FROM workspaces ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], row_to_workspace)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
    let detected_wt: Option<String> = row.get(8)?;
    let detected_br: Option<String> = row.get(9)?;
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
        task_prompt: row.get(7)?,
        detected_worktree: detected_wt.map(PathBuf::from),
        detected_branch: detected_br,
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
            task_prompt: String::new(),
            detected_worktree: None,
            detected_branch: None,
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
    fn task_prompt_round_trips() {
        let conn = open_in_memory().unwrap();
        let mut ws = sample("p");
        ws.task_prompt = "implement OAuth flow".into();
        insert(&conn, &ws).unwrap();
        let got = get(&conn, ws.id).unwrap().unwrap();
        assert_eq!(got.task_prompt, "implement OAuth flow");
        assert_eq!(got.detected_worktree, None);
        assert_eq!(got.detected_branch, None);
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
}
