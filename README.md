# Tessera

Linux/macOS-native orchestrator for parallel Claude Code agents, each in its
own workspace. Built in Rust + Tauri 2 + SolidJS. Inspired by
[Superset](https://github.com/superset-sh/superset) (macOS-only).

A *tessera* is a single tile of a mosaic — each agent is a piece, together
they form the picture.

> Status: alpha. Workspaces, hooks, status updates, dangerous mode,
> session resume (`--continue`), and per-workspace claude PTY are working.
> Diff viewer, desktop notifications, and packaging-by-CI are not yet.

## Quick start

```bash
git clone https://github.com/nvrxq/Tessera.git
cd Tessera
./scripts/build.sh
./target/release/tessera
```

The build script checks prerequisites, builds the UI, and produces a single
self-contained binary at `target/release/tessera` (~9MB).

### Prerequisites

- **Rust** 1.88+ (pinned via `rust-toolchain.toml`):
  `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Bun** 1.3+ (for the UI build):
  `curl -fsSL https://bun.sh/install | bash`
- **Claude Code** on `PATH` (the agent Tessera spawns):
  `npm install -g @anthropic-ai/claude-code`

#### Linux-only — system deps for webkit2gtk

```bash
sudo apt-get install libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev \
                     libayatana-appindicator3-dev librsvg2-dev \
                     libsoup-3.0-dev libjavascriptcoregtk-4.1-dev patchelf
```

macOS has no extra system deps — Tauri uses the system WebKit.

## How it works

- Each workspace is a folder you point Tessera at. Click → `claude` spawns in
  a PTY scoped to that folder. Subsequent launches add `--continue` so the
  conversation resumes from disk (Claude Code's built-in session store).
- Claude's `PostToolUse` / `Stop` / `Notification` hooks fire back to Tessera
  over a Unix-domain socket. Tessera updates the workspace status dot
  (Working / Needs input / Done) in the sidebar in real time.
- If Claude runs `git worktree add ...`, Tessera parses the command from the
  hook payload and displays the new worktree path next to the workspace.
- All state lives in SQLite at `~/.local/share/tessera/state.db` (Linux) /
  `~/Library/Application Support/tessera/state.db` (macOS).

## Architecture

Workspace crate layout:

| crate                  | role                                                  |
| ---------------------- | ----------------------------------------------------- |
| `tessera-core`         | domain types (`Workspace`, `AgentStatus`)             |
| `tessera-store`        | SQLite migrations + queries                           |
| `tessera-git`          | libgit2 worktree + diff (reserved for diff viewer)    |
| `tessera-pty`          | `portable-pty` supervisor + tokio broadcast events    |
| `tessera-hook`         | Unix-socket hook listener + `git worktree add` parser |
| `tessera-workspace`    | orchestration glue                                    |
| `src-tauri`            | Tauri 2 shell, commands, event pumps                  |

Frontend is SolidJS + Vite + xterm.js. Fonts (Geist, Geist Mono, Space Mono,
Instrument Serif) are bundled via `@fontsource/*` — no network on launch.

Full spec: [docs/superpowers/specs/2026-05-24-superset-linux-rust-design.md](docs/superpowers/specs/2026-05-24-superset-linux-rust-design.md).

## Tests

```bash
cargo test --workspace
```

## License

Apache-2.0. See [LICENSE](LICENSE) (added in a later plan).
