# Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the Cargo workspace, three foundation crates (`core`, `store`, `git`), a buildable Tauri 2 shell with a SolidJS frontend, and CI. After this plan, `cargo test` passes for all crates and `cargo tauri dev` opens an empty window with the app menu.

**Architecture:** Cargo workspace at the repo root. Library crates under `crates/{core,store,git}`. Tauri binary crate at `src-tauri/`. SolidJS + Vite project at `ui/`. Forward-only SQLite migrations embedded via `include_str!`. libgit2 used through the `git2` crate. No PTY, no IPC commands, no Claude hooks yet — those live in later plans.

**Tech Stack:** Rust 1.83+, Tauri 2.x, `rusqlite` (bundled), `git2`, `tokio`, `serde`, `uuid`, `chrono`, `anyhow`, `thiserror`, `tempfile` (tests). Frontend: SolidJS, Vite, TypeScript.

**Conventions:**
- All commands run from the repo root unless stated otherwise.
- Repo root = `/home/save/Work/Second/super-linux/` (current working dir for this plan).
- App placeholder name `super-linux` is used throughout until renaming (parking-lot item from spec §9.1).
- TDD: every behaviour change starts with a failing test.

---

## Task 1: Initialize Cargo Workspace and Toolchain

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `rust-toolchain.toml`
- Create: `.gitignore`
- Create: `crates/core/Cargo.toml`
- Create: `crates/store/Cargo.toml`
- Create: `crates/git/Cargo.toml`
- Create: `crates/core/src/lib.rs`
- Create: `crates/store/src/lib.rs`
- Create: `crates/git/src/lib.rs`

- [ ] **Step 1: Pin toolchain**

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.83"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 2: Root .gitignore**

Create `.gitignore`:

```
/target
/dist
/ui/node_modules
/ui/dist
/src-tauri/target
/src-tauri/gen
.DS_Store
*.swp
```

- [ ] **Step 3: Workspace Cargo.toml**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/core",
    "crates/store",
    "crates/git",
    "src-tauri",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"
repository = "https://github.com/<TBD>/super-linux"

[workspace.dependencies]
anyhow = "1"
thiserror = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tempfile = "3"
```

- [ ] **Step 4: core crate stub**

Create `crates/core/Cargo.toml`:

```toml
[package]
name = "super-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
uuid.workspace = true
chrono.workspace = true
thiserror.workspace = true
```

Create `crates/core/src/lib.rs`:

```rust
//! Domain types shared across the app.
```

- [ ] **Step 5: store crate stub**

Create `crates/store/Cargo.toml`:

```toml
[package]
name = "super-store"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
super-core = { path = "../core" }
rusqlite = { version = "0.34", features = ["bundled", "chrono", "uuid", "serde_json"] }
anyhow.workspace = true
thiserror.workspace = true
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
chrono.workspace = true
tracing.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

Create `crates/store/src/lib.rs`:

```rust
//! SQLite-backed persistence for super-linux.
```

- [ ] **Step 6: git crate stub**

Create `crates/git/Cargo.toml`:

```toml
[package]
name = "super-git"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
super-core = { path = "../core" }
git2 = "0.20"
anyhow.workspace = true
thiserror.workspace = true
tracing.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

Create `crates/git/src/lib.rs`:

```rust
//! libgit2-backed worktree and diff operations.
```

- [ ] **Step 7: Verify workspace builds**

Run: `cargo build --workspace`
Expected: build succeeds, no warnings beyond `unused_*` from empty stubs.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .gitignore crates/
git commit -m "build: cargo workspace skeleton with core/store/git stubs"
```

---

## Task 2: Domain Types in `core`

**Files:**
- Modify: `crates/core/src/lib.rs`
- Create: `crates/core/src/workspace.rs`
- Create: `crates/core/src/session.rs`
- Create: `crates/core/src/error.rs`

- [ ] **Step 1: Write failing tests**

Replace `crates/core/src/lib.rs` with:

```rust
//! Domain types shared across the app.

pub mod error;
pub mod session;
pub mod workspace;

pub use error::CoreError;
pub use session::{AgentSession, AgentStatus};
pub use workspace::{SetupStatus, Workspace};

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn workspace_round_trips_json() {
        let ws = Workspace {
            id: Uuid::new_v4(),
            name: "feat/login".into(),
            repo_path: PathBuf::from("/tmp/repo"),
            worktree_path: PathBuf::from("/tmp/wt"),
            branch: "feat/login".into(),
            created_at: Utc::now(),
            setup_status: SetupStatus::Pending,
        };
        let j = serde_json::to_string(&ws).unwrap();
        let back: Workspace = serde_json::from_str(&j).unwrap();
        assert_eq!(back.id, ws.id);
        assert_eq!(back.branch, ws.branch);
    }

    #[test]
    fn setup_status_failed_carries_message() {
        let s = SetupStatus::Failed {
            stderr_tail: "boom\n".into(),
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("boom"));
    }

    #[test]
    fn agent_status_serializes_as_tagged() {
        let s = AgentStatus::NeedsInput;
        assert_eq!(serde_json::to_string(&s).unwrap(), "\"needs_input\"");
    }
}
```

Also add `serde_json` to dev-deps in `crates/core/Cargo.toml`:

```toml
[dev-dependencies]
serde_json.workspace = true
```

- [ ] **Step 2: Run tests, expect compile failures**

Run: `cargo test -p super-core`
Expected: errors about unresolved `error`, `session`, `workspace` modules.

- [ ] **Step 3: Implement `error.rs`**

Create `crates/core/src/error.rs`:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("invalid workspace name: {0}")]
    InvalidName(String),
}
```

- [ ] **Step 4: Implement `workspace.rs`**

Create `crates/core/src/workspace.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetupStatus {
    Pending,
    Running,
    Ok,
    Failed { stderr_tail: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub repo_path: PathBuf,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub created_at: DateTime<Utc>,
    pub setup_status: SetupStatus,
}
```

- [ ] **Step 5: Implement `session.rs`**

Create `crates/core/src/session.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    NeedsInput,
    Done,
    Crashed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub pty_pid: Option<u32>,
    pub status: AgentStatus,
    pub started_at: DateTime<Utc>,
    pub last_event_at: DateTime<Utc>,
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p super-core`
Expected: 3 tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/core
git commit -m "feat(core): domain types for workspaces and agent sessions"
```

---

## Task 3: SQLite Store — Migrations and Schema

**Files:**
- Modify: `crates/store/src/lib.rs`
- Create: `crates/store/src/migrations.rs`
- Create: `crates/store/migrations/0001_init.sql`

- [ ] **Step 1: Write the schema migration**

Create `crates/store/migrations/0001_init.sql`:

```sql
CREATE TABLE schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);

CREATE TABLE workspaces (
    id            TEXT PRIMARY KEY NOT NULL,
    name          TEXT NOT NULL,
    repo_path     TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    branch        TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    setup_status  TEXT NOT NULL  -- JSON-encoded SetupStatus
);

CREATE TABLE agent_sessions (
    id             TEXT PRIMARY KEY NOT NULL,
    workspace_id   TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    pty_pid        INTEGER,
    status         TEXT NOT NULL,
    started_at     TEXT NOT NULL,
    last_event_at  TEXT NOT NULL
);

CREATE INDEX idx_sessions_by_workspace ON agent_sessions(workspace_id);
```

- [ ] **Step 2: Write the failing migration test**

Replace `crates/store/src/lib.rs` with:

```rust
//! SQLite-backed persistence for super-linux.

pub mod migrations;

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
        assert_eq!(version, 1);
    }
}
```

- [ ] **Step 3: Run tests to confirm failure**

Run: `cargo test -p super-store`
Expected: compile error — `migrations::apply` undefined.

- [ ] **Step 4: Implement migrations runner**

Create `crates/store/src/migrations.rs`:

```rust
use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;

struct Migration {
    version: i64,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: include_str!("../migrations/0001_init.sql"),
}];

pub fn apply(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (\
            version INTEGER PRIMARY KEY,\
            applied_at TEXT NOT NULL\
        );",
    )?;

    let current: i64 = conn
        .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| r.get(0))
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
```

- [ ] **Step 5: Run tests, expect green**

Run: `cargo test -p super-store`
Expected: 2 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/store
git commit -m "feat(store): sqlite migrations and schema for workspaces and sessions"
```

---

## Task 4: Store DAO — Workspaces

**Files:**
- Modify: `crates/store/src/lib.rs`
- Create: `crates/store/src/workspaces.rs`

- [ ] **Step 1: Failing tests for workspace CRUD**

Add to `crates/store/src/lib.rs` after the `open_in_memory` function:

```rust
pub mod workspaces;
```

Create `crates/store/src/workspaces.rs` with tests at the bottom — but write the tests first by appending to the same file an empty `pub` block we'll fill, then writing tests inline. Use this content:

```rust
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use super_core::{SetupStatus, Workspace};
use uuid::Uuid;

pub fn insert(conn: &Connection, ws: &Workspace) -> Result<()> {
    let setup_json = serde_json::to_string(&ws.setup_status)?;
    conn.execute(
        "INSERT INTO workspaces (id, name, repo_path, worktree_path, branch, created_at, setup_status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            ws.id.to_string(),
            ws.name,
            ws.repo_path.to_string_lossy(),
            ws.worktree_path.to_string_lossy(),
            ws.branch,
            ws.created_at.to_rfc3339(),
            setup_json,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: Uuid) -> Result<Option<Workspace>> {
    conn.query_row(
        "SELECT id, name, repo_path, worktree_path, branch, created_at, setup_status \
         FROM workspaces WHERE id = ?1",
        params![id.to_string()],
        row_to_workspace,
    )
    .optional()
    .map_err(Into::into)
}

pub fn list(conn: &Connection) -> Result<Vec<Workspace>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, repo_path, worktree_path, branch, created_at, setup_status \
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

pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
    let n = conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id.to_string()])?;
    anyhow::ensure!(n == 1, "workspace {id} not found");
    Ok(())
}

fn row_to_workspace(row: &rusqlite::Row<'_>) -> rusqlite::Result<Workspace> {
    let id_s: String = row.get(0)?;
    let created_s: String = row.get(5)?;
    let setup_s: String = row.get(6)?;
    Ok(Workspace {
        id: Uuid::parse_str(&id_s).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?,
        name: row.get(1)?,
        repo_path: std::path::PathBuf::from(row.get::<_, String>(2)?),
        worktree_path: std::path::PathBuf::from(row.get::<_, String>(3)?),
        branch: row.get(4)?,
        created_at: DateTime::parse_from_rfc3339(&created_s)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e)))?
            .with_timezone(&Utc),
        setup_status: serde_json::from_str(&setup_s)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e)))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;
    use std::path::PathBuf;

    fn sample(name: &str) -> Workspace {
        Workspace {
            id: Uuid::new_v4(),
            name: name.into(),
            repo_path: PathBuf::from("/tmp/repo"),
            worktree_path: PathBuf::from(format!("/tmp/wt-{name}")),
            branch: name.into(),
            created_at: Utc::now(),
            setup_status: SetupStatus::Pending,
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
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p super-store`
Expected: previous 2 + 4 new = 6 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/store
git commit -m "feat(store): workspace CRUD DAO with tests"
```

---

## Task 5: Git Crate — Worktree Operations

**Files:**
- Modify: `crates/git/src/lib.rs`
- Create: `crates/git/src/worktree.rs`
- Create: `crates/git/src/test_repo.rs` (test helper)

- [ ] **Step 1: Failing tests**

Replace `crates/git/src/lib.rs` with:

```rust
//! libgit2-backed worktree and diff operations.

pub mod worktree;

#[cfg(test)]
pub(crate) mod test_repo;
```

Create `crates/git/src/test_repo.rs`:

```rust
//! Helpers to spin up a real on-disk git repo for tests.

use anyhow::Result;
use git2::{Repository, Signature};
use std::path::Path;
use tempfile::TempDir;

pub struct Fixture {
    pub dir: TempDir,
    pub repo: Repository,
}

impl Fixture {
    pub fn new() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let repo = Repository::init(dir.path())?;
        let sig = Signature::now("test", "test@example.com")?;

        let path = dir.path().join("README.md");
        std::fs::write(&path, "hello\n")?;
        let mut index = repo.index()?;
        index.add_path(Path::new("README.md"))?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;
        repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])?;
        index.write()?;
        // ensure the default branch ref is set up
        let head_name = repo.head()?.shorthand().unwrap_or("master").to_string();
        let head_commit = repo.head()?.peel_to_commit()?;
        repo.branch(&head_name, &head_commit, true)?;
        Ok(Fixture { dir, repo })
    }
}
```

Create `crates/git/src/worktree.rs` with tests at the bottom:

```rust
use anyhow::Result;
use git2::{Repository, WorktreeAddOptions};
use std::path::{Path, PathBuf};

/// Create a new worktree at `worktree_path` for `branch_name`, branching from HEAD of `repo_path`.
pub fn create(repo_path: &Path, worktree_path: &Path, branch_name: &str) -> Result<PathBuf> {
    let repo = Repository::open(repo_path)?;
    let head_commit = repo.head()?.peel_to_commit()?;
    let branch = repo.branch(branch_name, &head_commit, false)?;
    let reference = branch.into_reference();

    let mut opts = WorktreeAddOptions::new();
    opts.reference(Some(&reference));

    let worktree_name = worktree_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("worktree path has no file name"))?;

    let wt = repo.worktree(worktree_name, worktree_path, Some(&opts))?;
    Ok(wt.path().to_path_buf())
}

/// Remove a worktree by `worktree_path`, regardless of state. Caller is responsible for asking
/// the user about dirty-state.
pub fn delete(repo_path: &Path, worktree_path: &Path, force: bool) -> Result<()> {
    let repo = Repository::open(repo_path)?;
    let worktree_name = worktree_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("worktree path has no file name"))?;
    let wt = repo.find_worktree(worktree_name)?;

    let mut prune_opts = git2::WorktreePruneOptions::new();
    prune_opts.working_tree(true).valid(force).locked(force);
    wt.prune(Some(&mut prune_opts))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_repo::Fixture;

    #[test]
    fn create_makes_worktree_directory() {
        let fx = Fixture::new().unwrap();
        let wt = fx.dir.path().join("wt-feat");
        let result = create(fx.dir.path(), &wt, "feat/x").unwrap();
        assert!(result.exists());
        assert!(wt.join("README.md").exists());
    }

    #[test]
    fn delete_removes_worktree() {
        let fx = Fixture::new().unwrap();
        let wt = fx.dir.path().join("wt-feat");
        create(fx.dir.path(), &wt, "feat/x").unwrap();
        delete(fx.dir.path(), &wt, true).unwrap();
        assert!(!wt.exists());
    }

    #[test]
    fn create_with_existing_branch_name_errors() {
        let fx = Fixture::new().unwrap();
        let wt1 = fx.dir.path().join("wt1");
        create(fx.dir.path(), &wt1, "dupe").unwrap();
        let wt2 = fx.dir.path().join("wt2");
        let err = create(fx.dir.path(), &wt2, "dupe").unwrap_err();
        let s = format!("{err}");
        assert!(s.contains("dupe") || s.contains("reference"), "got: {s}");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p super-git`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/git
git commit -m "feat(git): worktree create and delete via libgit2"
```

---

## Task 6: Git Crate — Diff Against Base Branch

**Files:**
- Create: `crates/git/src/diff.rs`
- Modify: `crates/git/src/lib.rs`

- [ ] **Step 1: Failing tests**

Add `pub mod diff;` to `crates/git/src/lib.rs` (after the existing `pub mod worktree;`).

Create `crates/git/src/diff.rs`:

```rust
use anyhow::Result;
use git2::{DiffOptions, Repository};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct FileDiff {
    pub path: PathBuf,
    pub status: String,   // "added" | "modified" | "deleted" | "renamed"
    pub patch: String,    // unified diff text
}

/// Diff `worktree_path` (HEAD + uncommitted) against the tip of `base_branch` in `repo_path`.
pub fn diff_against_base(
    repo_path: &Path,
    worktree_path: &Path,
    base_branch: &str,
) -> Result<Vec<FileDiff>> {
    let repo = Repository::open(repo_path)?;
    let base_ref = repo.find_branch(base_branch, git2::BranchType::Local)?;
    let base_commit = base_ref.get().peel_to_commit()?;
    let base_tree = base_commit.tree()?;

    // Use the worktree as the comparison target.
    let wt_repo = Repository::open(worktree_path)?;
    let head_tree = wt_repo.head()?.peel_to_tree()?;

    let mut opts = DiffOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);

    let diff = wt_repo.diff_tree_to_workdir_with_index(Some(&head_tree), Some(&mut opts))?;
    let base_to_head = repo.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None)?;

    let mut out: Vec<FileDiff> = Vec::new();

    let mut collect = |d: git2::Diff| -> Result<()> {
        d.foreach(
            &mut |delta, _| {
                let path = delta
                    .new_file()
                    .path()
                    .or_else(|| delta.old_file().path())
                    .map(PathBuf::from)
                    .unwrap_or_default();
                out.push(FileDiff {
                    path,
                    status: status_to_str(delta.status()).into(),
                    patch: String::new(),
                });
                true
            },
            None,
            None,
            Some(&mut |_, _, line| {
                if let Some(last) = out.last_mut() {
                    let origin = line.origin();
                    if matches!(origin, '+' | '-' | ' ' | 'F' | 'H') {
                        if origin == '+' || origin == '-' || origin == ' ' {
                            last.patch.push(origin);
                        }
                        last.patch.push_str(&String::from_utf8_lossy(line.content()));
                    } else {
                        last.patch.push_str(&String::from_utf8_lossy(line.content()));
                    }
                }
                true
            }),
        )?;
        Ok(())
    };

    collect(base_to_head)?;
    collect(diff)?;
    Ok(out)
}

fn status_to_str(s: git2::Delta) -> &'static str {
    match s {
        git2::Delta::Added => "added",
        git2::Delta::Modified => "modified",
        git2::Delta::Deleted => "deleted",
        git2::Delta::Renamed => "renamed",
        git2::Delta::Copied => "copied",
        git2::Delta::Untracked => "added",
        _ => "modified",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_repo::Fixture;
    use crate::worktree;

    #[test]
    fn diff_reports_modified_file() {
        let fx = Fixture::new().unwrap();
        let base = fx.repo.head().unwrap().shorthand().unwrap().to_string();
        let wt = fx.dir.path().join("wt");
        worktree::create(fx.dir.path(), &wt, "feat/x").unwrap();

        // edit and commit inside the worktree
        std::fs::write(wt.join("README.md"), "hello\nworld\n").unwrap();
        let wt_repo = git2::Repository::open(&wt).unwrap();
        let sig = git2::Signature::now("t", "t@x").unwrap();
        let mut index = wt_repo.index().unwrap();
        index.add_path(std::path::Path::new("README.md")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = wt_repo.find_tree(tree_id).unwrap();
        let parent = wt_repo.head().unwrap().peel_to_commit().unwrap();
        wt_repo
            .commit(Some("HEAD"), &sig, &sig, "edit", &tree, &[&parent])
            .unwrap();

        let diffs = diff_against_base(fx.dir.path(), &wt, &base).unwrap();
        assert!(diffs.iter().any(|d| d.path == std::path::PathBuf::from("README.md")));
    }
}
```

Add `serde` to the `crates/git/Cargo.toml` dependencies:

```toml
serde.workspace = true
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p super-git`
Expected: 4 tests pass total (3 from Task 5 + 1 new).

- [ ] **Step 3: Commit**

```bash
git add crates/git
git commit -m "feat(git): diff worktree against base branch"
```

---

## Task 7: SolidJS + Vite Frontend Skeleton

**Files:**
- Create: `ui/package.json`
- Create: `ui/vite.config.ts`
- Create: `ui/tsconfig.json`
- Create: `ui/index.html`
- Create: `ui/src/main.tsx`
- Create: `ui/src/App.tsx`
- Create: `ui/src/index.css`

- [ ] **Step 1: package.json**

Create `ui/package.json`:

```json
{
  "name": "super-linux-ui",
  "private": true,
  "version": "0.0.0",
  "type": "module",
  "scripts": {
    "dev": "vite --port 1420 --strictPort",
    "build": "vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "solid-js": "1.9.3",
    "@tauri-apps/api": "2.1.1"
  },
  "devDependencies": {
    "vite": "5.4.10",
    "vite-plugin-solid": "2.10.2",
    "typescript": "5.6.3"
  }
}
```

- [ ] **Step 2: vite + tsconfig**

Create `ui/vite.config.ts`:

```ts
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: { target: "esnext", minify: false, sourcemap: true },
});
```

Create `ui/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "jsx": "preserve",
    "jsxImportSource": "solid-js",
    "strict": true,
    "skipLibCheck": true,
    "isolatedModules": true,
    "esModuleInterop": true,
    "allowSyntheticDefaultImports": true,
    "resolveJsonModule": true,
    "types": ["vite/client"]
  },
  "include": ["src/**/*.ts", "src/**/*.tsx"]
}
```

- [ ] **Step 3: HTML + entry**

Create `ui/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>super-linux</title>
    <link rel="stylesheet" href="/src/index.css" />
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

Create `ui/src/main.tsx`:

```tsx
import { render } from "solid-js/web";
import App from "./App";

const root = document.getElementById("root");
if (!root) throw new Error("missing root element");
render(() => <App />, root);
```

Create `ui/src/App.tsx`:

```tsx
import type { Component } from "solid-js";

const App: Component = () => {
  return (
    <main>
      <h1>super-linux</h1>
      <p>Foundation scaffold — no agents yet.</p>
    </main>
  );
};

export default App;
```

Create `ui/src/index.css`:

```css
:root {
  font-family: system-ui, sans-serif;
  color-scheme: dark;
  background: #1e1e1e;
  color: #ddd;
}
body { margin: 0; }
main { padding: 2rem; }
h1 { font-weight: 500; }
```

- [ ] **Step 4: Install deps and build**

Run: `cd ui && bun install && bun run build`
Expected: `ui/dist/index.html` exists, no TS errors.

- [ ] **Step 5: Commit**

```bash
git add ui/
git commit -m "feat(ui): solidjs+vite scaffold with empty app shell"
```

---

## Task 8: Tauri 2 App Scaffold

**Files:**
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/build.rs`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/icons/icon.png` (1x1 transparent placeholder)
- Create: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Tauri Cargo.toml**

Create `src-tauri/Cargo.toml`:

```toml
[package]
name = "super-linux"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "super_linux_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
super-core = { path = "../crates/core" }
super-store = { path = "../crates/store" }
super-git = { path = "../crates/git" }
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
```

- [ ] **Step 2: tauri-build runner**

Create `src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build();
}
```

- [ ] **Step 3: Tauri configuration**

Create `src-tauri/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "super-linux",
  "version": "0.1.0",
  "identifier": "sh.superlinux.app",
  "build": {
    "beforeDevCommand": "cd ../ui && bun run dev",
    "beforeBuildCommand": "cd ../ui && bun run build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../ui/dist"
  },
  "app": {
    "windows": [
      {
        "title": "super-linux",
        "width": 1200,
        "height": 800,
        "resizable": true,
        "fullscreen": false
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["deb", "appimage"],
    "icon": ["icons/icon.png"]
  }
}
```

- [ ] **Step 4: Tauri capabilities (Tauri 2 ACL)**

Create `src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "default window permissions",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

- [ ] **Step 5: Placeholder icon**

Run: `mkdir -p src-tauri/icons && printf '\x89PNG\r\n\x1a\n' > /dev/null` — actually create a real 1×1 PNG:

```bash
python3 - <<'PY'
import struct, zlib, pathlib
sig = b'\x89PNG\r\n\x1a\n'
def chunk(t, d):
    return struct.pack('>I',len(d)) + t + d + struct.pack('>I', zlib.crc32(t+d) & 0xffffffff)
ihdr = struct.pack('>IIBBBBB', 1, 1, 8, 6, 0, 0, 0)
raw = b'\x00\x00\x00\x00\x00'
idat = zlib.compress(raw)
pathlib.Path('src-tauri/icons').mkdir(parents=True, exist_ok=True)
with open('src-tauri/icons/icon.png','wb') as f:
    f.write(sig + chunk(b'IHDR',ihdr) + chunk(b'IDAT',idat) + chunk(b'IEND',b''))
print('wrote 1x1 png')
PY
```

Expected: prints `wrote 1x1 png`. File exists at `src-tauri/icons/icon.png`.

- [ ] **Step 6: Tauri lib + main**

Create `src-tauri/src/lib.rs`:

```rust
use tracing_subscriber::EnvFilter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    tauri::Builder::default()
        .setup(|_app| {
            tracing::info!("super-linux starting");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Create `src-tauri/src/main.rs`:

```rust
// Prevent the launcher console on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    super_linux_lib::run();
}
```

- [ ] **Step 7: Add tauri-cli as a workspace dev tool**

Run: `cargo install tauri-cli@^2 --locked` (one-time global install)

Then verify the workspace builds: `cargo build -p super-linux`
Expected: build succeeds. (Frontend not required for `cargo build`.)

- [ ] **Step 8: Smoke test the dev runner (manual, optional)**

Run: `cargo tauri dev --no-watch`
Expected: a window titled "super-linux" appears showing "Foundation scaffold — no agents yet." Close it with the window button.

If `cargo tauri` isn't found, ensure `~/.cargo/bin` is on PATH and re-run.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/
git commit -m "feat(app): tauri 2 scaffold with empty window"
```

---

## Task 9: CI Workflow

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: CI yaml**

Create `.github/workflows/ci.yml`:

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

jobs:
  rust:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - name: install system deps (tauri)
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libwebkit2gtk-4.1-dev \
            libssl-dev \
            libgtk-3-dev \
            libayatana-appindicator3-dev \
            librsvg2-dev \
            libsoup-3.0-dev \
            libjavascriptcoregtk-4.1-dev \
            patchelf
      - uses: dtolnay/rust-toolchain@1.83
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: fmt
        run: cargo fmt --all -- --check
      - name: clippy
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: test
        run: cargo test --workspace --all-targets

  ui:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - uses: oven-sh/setup-bun@v2
        with:
          bun-version: 1.1.x
      - name: install
        working-directory: ui
        run: bun install --frozen-lockfile
      - name: build
        working-directory: ui
        run: bun run build
```

- [ ] **Step 2: Verify locally before commit**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

Expected: all three commands succeed. If `clippy` fails, fix warnings and re-run.

- [ ] **Step 3: Commit**

```bash
git add .github/
git commit -m "ci: fmt/clippy/test on ubuntu-24.04 + frontend build"
```

---

## Task 10: README

**Files:**
- Create: `README.md`

- [ ] **Step 1: README content**

Create `README.md`:

```markdown
# super-linux

Linux-first, Rust-based reimplementation of [Superset](https://github.com/superset-sh/superset) — orchestrate parallel CLI coding agents (Claude Code first) across isolated git worktrees.

> Status: foundation scaffold. Worktree + diff + SQLite + Tauri shell are in place. No PTY, no agent integration yet.

## Architecture

See [docs/superpowers/specs/2026-05-24-superset-linux-rust-design.md](docs/superpowers/specs/2026-05-24-superset-linux-rust-design.md).

## Build

### Requirements

- Rust 1.83 (pinned via `rust-toolchain.toml`)
- Bun 1.1+
- Linux system deps for Tauri:
  ```
  sudo apt-get install libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev \
                       libayatana-appindicator3-dev librsvg2-dev \
                       libsoup-3.0-dev libjavascriptcoregtk-4.1-dev patchelf
  ```
- Tauri CLI: `cargo install tauri-cli@^2 --locked`

### Run

```
cd ui && bun install && cd ..
cargo tauri dev
```

### Test

```
cargo test --workspace
```

## License

Apache-2.0. See [LICENSE](LICENSE) (added in a later plan).
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: README with build + test instructions"
```

---

## Final Verification

- [ ] **Step 1: Full test pass**

Run: `cargo test --workspace`
Expected: all tests green across `super-core`, `super-store`, `super-git`.

- [ ] **Step 2: Clippy clean**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: zero warnings.

- [ ] **Step 3: UI builds**

Run: `cd ui && bun run build && cd ..`
Expected: `ui/dist/` populated.

- [ ] **Step 4: Tauri builds debug binary**

Run: `cargo build -p super-linux`
Expected: `target/debug/super-linux` exists.

- [ ] **Step 5: Manual smoke (optional)**

Run: `cargo tauri dev`
Expected: window opens, shows "super-linux / Foundation scaffold — no agents yet.", closes cleanly.

---

## Out of Scope (Tracked for Plan 2+)

- PTY supervision (`portable-pty`)
- Tauri commands for create/delete/list workspaces (no `ipc` crate yet)
- xterm.js terminal pane
- Claude Code hook integration
- Setup/teardown script runner
- Diff viewer UI
- Notifications
- Packaging (AppImage/.deb beyond the default Tauri config)
