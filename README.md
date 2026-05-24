# Tessera

Linux-first, Rust-based orchestrator for parallel CLI coding agents (Claude Code first) running in isolated git worktrees. Inspired by [Superset](https://github.com/superset-sh/superset) (macOS-only).

A *tessera* is a single tile of a mosaic — each agent is a piece, together they form the picture.

> Status: foundation scaffold. Worktree + diff + SQLite + Tauri shell are in place. No PTY, no agent integration yet.

## Architecture

See [docs/superpowers/specs/2026-05-24-superset-linux-rust-design.md](docs/superpowers/specs/2026-05-24-superset-linux-rust-design.md).

## Build

### Requirements

- Rust 1.88 (pinned via `rust-toolchain.toml`)
- Bun 1.3+
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
