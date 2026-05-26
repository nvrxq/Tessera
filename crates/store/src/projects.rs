use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tessera_core::Project;
use uuid::Uuid;

pub fn insert(conn: &Connection, project: &Project) -> Result<()> {
    conn.execute(
        "INSERT INTO projects (id, name, accent, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            project.id.to_string(),
            project.name,
            project.accent,
            project.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: Uuid) -> Result<Option<Project>> {
    conn.query_row(
        "SELECT id, name, accent, created_at FROM projects WHERE id = ?1",
        params![id.to_string()],
        row_to_project,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list(conn: &Connection) -> Result<Vec<Project>> {
    let mut stmt =
        conn.prepare("SELECT id, name, accent, created_at FROM projects ORDER BY created_at ASC")?;
    let rows = stmt.query_map([], row_to_project)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
    let n = conn.execute(
        "DELETE FROM projects WHERE id = ?1",
        params![id.to_string()],
    )?;
    anyhow::ensure!(n == 1, "project {id} not found");
    Ok(())
}

fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let id_s: String = row.get(0)?;
    let created_s: String = row.get(3)?;
    Ok(Project {
        id: Uuid::parse_str(&id_s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        name: row.get(1)?,
        accent: row.get(2)?,
        created_at: DateTime::parse_from_rfc3339(&created_s)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
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

    fn sample(name: &str) -> Project {
        Project {
            id: Uuid::new_v4(),
            name: name.into(),
            accent: Some("#C8825B".into()),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn insert_then_get() {
        let conn = open_in_memory().unwrap();
        let p = sample("alpha");
        insert(&conn, &p).unwrap();
        let got = get(&conn, p.id).unwrap().unwrap();
        assert_eq!(got.id, p.id);
        assert_eq!(got.name, "alpha");
        assert_eq!(got.accent.as_deref(), Some("#C8825B"));
    }

    #[test]
    fn list_orders_by_created_at_asc() {
        let conn = open_in_memory().unwrap();
        let mut a = sample("a");
        a.created_at = Utc::now() - chrono::Duration::seconds(10);
        let b = sample("b");
        insert(&conn, &b).unwrap();
        insert(&conn, &a).unwrap();
        let all = list(&conn).unwrap();
        assert_eq!(all[0].name, "a");
        assert_eq!(all[1].name, "b");
    }

    #[test]
    fn delete_removes_row() {
        let conn = open_in_memory().unwrap();
        let p = sample("a");
        insert(&conn, &p).unwrap();
        delete(&conn, p.id).unwrap();
        assert!(get(&conn, p.id).unwrap().is_none());
    }

    #[test]
    fn delete_unknown_errors() {
        let conn = open_in_memory().unwrap();
        assert!(delete(&conn, Uuid::new_v4()).is_err());
    }

    #[test]
    fn accent_can_be_null() {
        let conn = open_in_memory().unwrap();
        let mut p = sample("no-accent");
        p.accent = None;
        insert(&conn, &p).unwrap();
        let got = get(&conn, p.id).unwrap().unwrap();
        assert_eq!(got.accent, None);
    }
}
