# Superset-for-Linux — Design Spec

**Date:** 2026-05-24
**Status:** Draft (awaiting user review)
**Goal:** Linux-first, Rust-based reimplementation of [Superset](https://github.com/superset-sh/superset) — a desktop tool that orchestrates parallel CLI coding agents across isolated git worktrees.

## 1. Motivation and Scope

The upstream Superset (`superset-sh/superset`) is an Electron app shipped only for macOS, with Linux explicitly marked as untested. The goal of this project is a Rust-native equivalent that:

- Runs natively on Linux as the first-class platform (other OSes are a free Tauri side effect, not a goal).
- Is suitable as a daily-driver tool for orchestrating multiple Claude Code sessions in parallel git worktrees.
- Is eventually open-sourced as a community-shippable artifact (binary distributable, no proprietary cloud dependencies).

### Non-goals for v1

- Multi-agent support beyond Claude Code (Codex/Cursor/Gemini/Copilot).
- Cloud sync (ElectricSQL, Neon).
- Authentication, billing, Stripe integration.
- Mobile companion app.
- Marketing site, docs site.
- Bundled AI SDK integrations (Anthropic, OpenAI) — agents bring their own auth.
- IDE handoff (Open in VSCode/nvim/Cursor button).

## 2. Form Factor and Stack

Form factor: **Tauri 2.x desktop app**. Rust backend + WebView frontend (SolidJS + xterm.js). Mirrors upstream's architecture (Electron main + React + xterm.js) but with the heavy lifting in Rust.

Rejected alternatives:
- **TUI (ratatui)**: terminal-in-terminal is painful (escape codes, mouse, image rendering), and the upstream's diff viewer and overview panels assume real pixel rendering.
- **Native Rust GUI (egui/iced)**: no production-grade embedded terminal widget; would require reinventing a VT100 emulator on top of GUI primitives.
- **Embed Zellij as PTY engine**: external binary dependency breaks the single-binary install promise needed for an OSS release.

### Stack

| Layer       | Choice                                       | Reason                                                      |
|-------------|----------------------------------------------|-------------------------------------------------------------|
| Runtime     | Rust 1.83+                                   | User-chosen.                                                |
| App shell   | Tauri 2.x                                    | Rust-native Electron equivalent, small binaries.            |
| Frontend    | SolidJS + Vite                               | Fast, signal-based, small bundle; React-like ergonomics.    |
| Terminals   | `@xterm/xterm` + `addon-fit`, `-search`, `-clipboard` | Battle-tested, used by upstream.                            |
| Diff view   | CodeMirror 6 + Solid bindings                | Syntax highlighting, line numbers, gutter for hunks.        |
| PTY         | `portable-pty` crate                         | Cross-platform PTY in pure Rust.                            |
| Git         | `git2` crate (libgit2)                       | Worktree, diff, status APIs without shelling out.           |
| Persistence | `rusqlite` + handwritten migrations          | Tiny schema, no need for an ORM.                            |
| Async       | `tokio`                                      | Standard Rust async runtime.                                |
| IPC         | Tauri commands + events                      | Built-in; no separate RPC layer needed.                     |

## 3. Architecture

### 3.1 Process model

**Monolith**: one Tauri binary. PTYs are direct child processes of Tauri main. Closing the GUI window kills running agents — this is an explicit v1 limitation, with a documented migration path to a separate `super-pty-daemon` process in v2 (out of scope for this spec).

```
┌────────────────────────────────────────────────┐
│                  Tauri main (Rust)             │
│ ┌──────────────────────────────────────────┐   │
│ │ git    pty    agent    store    ipc      │   │
│ └──────────────────────────────────────────┘   │
│                    ↕ Tauri IPC                 │
│ ┌──────────────────────────────────────────┐   │
│ │ WebView: SolidJS + xterm.js              │   │
│ │   workspace tree | terminal panes | diff │   │
│ └──────────────────────────────────────────┘   │
└────────────────────────────────────────────────┘
         ↕ Unix socket (~/.local/share/<app>/hooks.sock)
┌────────────────────────────────────────────────┐
│ Claude Code (running inside a worktree)        │
│   hooks → `super-hook <event>` subcommand →    │
│   writes JSON to socket                        │
└────────────────────────────────────────────────┘
```

### 3.2 Crate layout

```
crates/
  core/        domain types: Workspace, AgentSession, Worktree, Status
  git/         worktree mgmt + diff (libgit2)
  pty/         portable-pty supervisor: spawn, write, resize, kill
  agent/       Claude Code hooks: install templates, listen on socket, parse events
  store/       rusqlite migrations + DAOs
  ipc/         Tauri command handlers + event emitters
src-tauri/     Tauri main entrypoint: window, menu, notifications, glue
ui/            SolidJS frontend (Vite project)
```

Each crate has a single clear purpose and a minimal public API. UI talks only to `ipc::*`; `ipc` orchestrates the other crates. No cross-crate cycles.

### 3.3 Filesystem layout (runtime)

| Path                                            | Purpose                                              |
|-------------------------------------------------|------------------------------------------------------|
| `~/.local/share/<app>/state.db`                 | SQLite database (XDG_DATA_HOME).                     |
| `~/.local/share/<app>/hooks.sock`               | Unix socket for Claude Code hook events.             |
| `~/.local/share/<app>/worktrees/<id>/`          | Worktree directories (created per workspace).        |
| `~/.config/<app>/config.toml`                   | User-level settings (theme, default editor).         |
| `<repo>/.super/setup.sh` (in user's project)    | Per-repo setup script (optional).                    |
| `<worktree>/.claude/settings.local.json`        | Auto-generated hooks config in each worktree.        |

App name is a parking-lot item (see §9). `<app>` is a placeholder until then.

## 4. Domain Model

```rust
struct Workspace {
    id: Uuid,
    name: String,                  // user-facing label
    repo_path: PathBuf,            // absolute path to source repo
    worktree_path: PathBuf,        // absolute path to created worktree
    branch: String,                // branch checked out in the worktree
    created_at: DateTime<Utc>,
    setup_status: SetupStatus,     // Pending | Running | Ok | Failed { stderr_tail }
}

enum AgentStatus {
    Idle,           // PTY alive, no recent activity
    Working,        // tool use observed recently
    NeedsInput,     // Notification hook fired
    Done,           // Stop hook fired without follow-up activity
    Crashed,        // PTY exited non-zero
}

struct AgentSession {
    id: Uuid,
    workspace_id: Uuid,
    pty_pid: Option<u32>,
    status: AgentStatus,
    last_event_at: DateTime<Utc>,
    started_at: DateTime<Utc>,
}
```

SQLite schema mirrors these structs 1:1. Migrations are forward-only, embedded as `.sql` strings in `crates/store/migrations/`.

## 5. Data Flow

### 5.1 Creating a workspace

1. User invokes "New workspace" → frontend calls `ipc::create_workspace(repo_path, branch_name)`.
2. `git`: create worktree at `~/.local/share/<app>/worktrees/<uuid>/`, branched from current HEAD of `repo_path` as `branch_name`.
3. `store`: insert `Workspace` row with `setup_status = Pending`.
4. `agent::install_hooks(worktree_path)`: write `.claude/settings.local.json` referencing `super-hook` subcommand.
5. If `<repo>/.super/setup.sh` exists: `pty::run_setup(worktree)` → on exit `setup_status = Ok` or `Failed { stderr_tail }`.
6. `pty::spawn_agent(worktree)` runs `claude` in the worktree → `AgentSession` row inserted.
7. `ipc::emit("workspace_ready", workspace)` → UI opens a terminal pane.

If step 2 or 3 fails, no row is committed — full rollback. If step 4 or 5 fails, the workspace exists with a failed setup badge; user can retry.

### 5.2 Agent status updates

```
Claude Code triggers hook (PostToolUse | Stop | Notification)
  → ~/.claude/settings.local.json invokes: super-hook <event> $CLAUDE_SESSION_ID
  → super-hook reads stdin (Claude passes JSON event payload)
  → writes JSON message to ~/.local/share/<app>/hooks.sock
  → main process socket listener parses → store::update_session_status
  → tauri::emit("agent_status_changed", {session_id, status})
  → UI updates indicator
  → if status == NeedsInput → tauri notification
```

Status mapping:
- `PostToolUse` → `Working` (reset 30s timer to `Idle`).
- `Stop` → `Done`.
- `Notification` → `NeedsInput` + desktop notification.
- PTY exit code != 0 → `Crashed`.

### 5.3 Diff viewer

User selects a workspace → UI calls `ipc::diff(workspace_id)` → `git::diff(worktree_path, base_branch)` returns `Vec<FileDiff { path, hunks }>` → UI renders in CodeMirror with diff gutter and syntax based on file extension.

### 5.4 Terminal I/O

xterm.js in the WebView is paired with one PTY in `pty::Supervisor`. Keystrokes from xterm.js → Tauri command `pty_write(session_id, bytes)`. PTY output bytes → Tauri event `pty_data(session_id, bytes)` → xterm.js `write()`. Resize events follow the same pattern with `pty_resize(session_id, cols, rows)`.

## 6. Claude Code Hook Integration

This is the most app-specific piece. Claude Code reads `~/.claude/settings.json` or `<cwd>/.claude/settings.local.json` for hooks (per the Claude Code docs).

Per-worktree, we write `<worktree>/.claude/settings.local.json`:

```json
{
  "hooks": {
    "Stop":         [{"hooks": [{"type": "command", "command": "<app> hook stop"}]}],
    "Notification": [{"hooks": [{"type": "command", "command": "<app> hook notify"}]}],
    "PostToolUse":  [{"hooks": [{"type": "command", "command": "<app> hook activity"}]}]
  }
}
```

`<app> hook <event>` is a subcommand of our own binary: reads Claude's JSON event from stdin, augments it with the worktree path (taken from `$PWD` or `$CLAUDE_PROJECT_DIR`), and writes a length-prefixed JSON line to `~/.local/share/<app>/hooks.sock`.

The main process maintains a tokio task that accepts connections on the socket, parses lines into typed events, and dispatches them to `store` + `ipc::emit`. If the socket is missing (app not running), `<app> hook` exits 0 silently so it doesn't break Claude Code.

## 7. Error Handling

| Failure                          | Behavior                                                          |
|----------------------------------|-------------------------------------------------------------------|
| Worktree create fails            | Rollback: delete partial worktree, no DB row.                     |
| `setup.sh` exit != 0             | Workspace stays, `setup_status = Failed`, UI shows badge + retry. |
| PTY child exits non-zero         | `status = Crashed`. UI offers Restart.                            |
| SQLite locked                    | Retry with exponential backoff, max 3 attempts.                   |
| Hook socket missing on read      | Recreate on next event. Don't crash main.                         |
| `<app> hook` invoked, no socket  | Exit 0 silently — never break Claude Code.                        |
| `git worktree remove` blocked by dirty state | Confirmation dialog: "Force delete? Local changes will be lost."  |

Errors surfaced to UI are `Result<T, AppError>` returned from `ipc` commands. `AppError` is a tagged enum serialised to JSON; UI renders human-readable messages.

## 8. Testing

| Layer          | Approach                                                                 |
|----------------|--------------------------------------------------------------------------|
| `git` unit     | `tempfile`-backed temporary repos; assert worktree create/diff/status.   |
| `store` unit   | In-memory SQLite; migration + DAO round-trips.                           |
| `agent` unit   | JSON fixture files for hook events; parser tests.                        |
| `pty` unit     | Spawn `echo`/`cat`; assert input/output round-trip.                      |
| Integration    | `bash -c 'echo {...} \| <app> hook stop'`; verify state propagates to store. |
| E2E smoke      | Spawn workspace in a fake repo, run `bash -i` as fake agent, verify status lifecycle. |
| UI             | Manual smoke for v1. Playwright wiring deferred to v1.1.                 |

CI: GitHub Actions on Linux, runs `cargo test` and a headless Tauri build to catch breakage.

## 9. Open Questions and Parking Lot

1. **Project name** — `superset` is taken (and confusable with Apache Superset). Candidates to surface for user choice: Loom, Quay, Pier, Mantle, Conductor, Hive. Decided before first commit, not before spec approval.
2. **Worktree naming** — `<uuid>/` vs `<branch-slug>-<short-uuid>/`. Lean towards branch-slug for debuggability when poking inside `~/.local/share/<app>/worktrees/`.
3. **XDG paths** — confirm `~/.local/share/<app>/` (data) and `~/.config/<app>/` (config) per XDG Base Directory spec. macOS port (later) will use `~/Library/Application Support/<app>/`.
4. **Window model** — v1 is single window with internal tabs. Multi-window deferred to v2.
5. **Licence** — upstream is ELv2 (source-available, not OSI). For a community port aiming at PRs and packaging, Apache-2.0 or MIT recommended. Final pick before public push.
6. **Status decay** — `Working → Idle` after 30s of no events; tunable in settings.
7. **`<app>` binary singleton** — what if user launches a second instance? Lock file in `~/.local/share/<app>/.lock`, second instance focuses the running window via D-Bus (Linux) instead of starting fresh.

## 10. Milestones (informational, not part of approval)

This spec covers v1 only. Concrete tasks come in the implementation plan (next step after spec approval). Rough cut:

1. Crate skeleton + Tauri scaffold + CI.
2. `git` + `store` with tests.
3. `pty` + Tauri terminal panel + xterm.js wiring.
4. `agent` hook listener + Claude Code template installation.
5. Workspace lifecycle (create, list, delete) + setup.sh.
6. Status indicator + notifications.
7. Diff viewer.
8. Packaging: AppImage + .deb + `cargo install --git` path.

## 11. References

- Upstream Superset: https://github.com/superset-sh/superset
- Tauri 2.x: https://v2.tauri.app/
- xterm.js: https://xtermjs.org/
- portable-pty: https://docs.rs/portable-pty/
- Claude Code hooks: documented in Claude Code's official docs (read at implementation time, do not assume current shape).
