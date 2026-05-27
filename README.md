# Tessera

Linux/macOS-native orchestrator for parallel Claude Code agents, each in its
own workspace. Built in Rust + Tauri 2 + SolidJS. Inspired by
[Superset](https://github.com/superset-sh/superset) (macOS-only).

A *tessera* is a single tile of a mosaic — each agent is a piece, together
they form the picture.

> Status: alpha. Workspaces, hooks, status updates, dangerous mode,
> session resume (`--continue`), and per-workspace claude PTY are working.
> Diff viewer, desktop notifications, and packaging-by-CI are not yet.

## Install

One-liner — installs the latest release for your OS+arch (macOS arm64/x64,
Linux x64):

```bash
curl -fsSL https://raw.githubusercontent.com/nvrxq/Tessera/main/install.sh | sh
```

- **macOS**: downloads the `.dmg`, copies `Tessera.app` to `/Applications`,
  and strips Gatekeeper quarantine (the binary isn't Apple-notarised; right-click
  → Open the first time if your security settings still block it).
- **Linux**: drops a single-file `.AppImage` into `~/.local/bin/tessera`.

Already installed? Re-running the script upgrades in place. Or just leave
the app open — the in-app **auto-updater** checks GitHub Releases on every
launch and prompts you to install + relaunch when a new signed version is
available. Update payloads are minisign-signed against the public key
embedded in the binary; an attacker on your network can't push a fake
update.

## Build from source

```bash
git clone https://github.com/nvrxq/Tessera.git
cd Tessera
./scripts/build.sh
./target/release/tessera
```

The build script checks prerequisites, builds the UI, and produces a single
self-contained binary at `target/release/tessera` (~12MB).

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

#### macOS

Tauri uses the system WebKit — no Homebrew/MacPorts deps required. You need:

- **macOS 11 Big Sur or newer**, on Apple Silicon (`aarch64`) or Intel (`x86_64`).
  Cargo auto-detects your host triple, so `./scripts/build.sh` produces a native
  binary for whichever Mac you build on.
- **Xcode Command Line Tools** — gives you the `clang` linker and macOS SDK
  headers that `rustc`/`cargo` need:
  ```bash
  xcode-select --install
  ```
  No full Xcode required.
- **`rustup` + Bun + Claude Code** — same install commands as above.

Build:

```bash
./scripts/build.sh
./target/release/tessera
```

First release build takes ~3–5 min on Apple Silicon (cold cache) and produces a
~11 MB binary at `target/release/tessera`. Subsequent incremental builds are a
few seconds.

If you'd rather develop with hot-reload UI, install the Tauri CLI once and use
its dev mode:

```bash
cargo install tauri-cli@^2 --locked
cargo tauri dev
```

##### Where Tessera stores data on macOS

Everything is under `~/Library/Application Support/tessera`:

| path                | purpose                                              |
| ------------------- | ---------------------------------------------------- |
| `state.db`          | SQLite — workspaces, projects, status                |
| `hooks.sock`        | Unix-domain socket for Claude Code hook callbacks    |
| `worktrees/`        | reserved for future Tessera-managed worktrees        |

Reset state by deleting `state.db` (Tessera will recreate it on next launch).

##### macOS troubleshooting

- **"`tessera` can't be opened because Apple cannot check it for malicious
  software"** — the binary you built locally isn't signed/notarized. Either
  right-click → Open the first time, or strip the quarantine bit:
  ```bash
  xattr -d com.apple.quarantine target/release/tessera
  ```
- **`claude` not found when spawning an agent** — Tessera launches `claude`
  from `PATH`. If you installed Claude Code via Homebrew Node, make sure your
  shell's `PATH` (in `~/.zshrc`) is exported in GUI sessions too, or symlink
  it into `/usr/local/bin`.
- **Permission prompts on first PTY spawn** — macOS may ask for access to the
  parent folder Tessera launches `claude` in (Documents/Desktop/etc.). Grant
  once; it's remembered per-app.

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

## Releasing (maintainer)

Releases are tag-driven. Pushing `v*` triggers `.github/workflows/release.yml`,
which builds `.dmg` (macOS arm64 + x64) and `.AppImage` (Linux x64) via
`tauri-action`, signs them with the minisign key, and publishes a GitHub
Release plus the `latest.json` manifest the in-app updater reads.

One-time setup before the first release:

1. Generate the minisign keypair locally:
   ```bash
   cargo tauri signer generate -w ~/.tauri/tessera.key
   ```
   The public key gets pasted into `src-tauri/tauri.conf.json` under
   `plugins.updater.pubkey`. Keep the private key secret.
2. Add two repo secrets at Settings → Secrets and variables → Actions:
   - `TAURI_SIGNING_PRIVATE_KEY` — contents of `~/.tauri/tessera.key`
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the password you set (empty
     string if you opted out)

Cutting a release:

```bash
# bump version in Cargo.toml workspace.package.version and src-tauri/tauri.conf.json
git commit -am "release: v0.1.0"
git tag v0.1.0
git push origin main v0.1.0
```

CI takes ~15 minutes. Watch progress under the repo's Actions tab.

## License

Apache-2.0. See [LICENSE](LICENSE) (added in a later plan).
