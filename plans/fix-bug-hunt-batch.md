# Plan: fix/bug-hunt-batch

Fix all 27 user-facing bugs found by the 2026-05-29 multi-agent audit (see `BUGS.md` Open).
Goal: every bug fixed, `cargo test` + `ui` tests green, Opus review, draft PR (authored nvrxq).

## Strategy
Bugs cluster into 6 disjoint file-lanes → parallel implementers, no merge conflicts.
Lane R1 (rendering/wire) is the most coupled → done by orchestrator directly.

## Lanes (disjoint file sets)

- **R1 — rendering/input/wire** (orchestrator): `crates/term/src/{grid.rs,cell.rs}`,
  `src-tauri/src/terminal.rs`, `ui/src/Terminal.tsx`
  - wide-char column alignment (S1) + width flag on WireCell, frontend 2-col draw
  - grapheme cluster as string (S1) [SmolStr]
  - Shift+Tab → CSI Z; F1–F12 escapes; Alt→ESC Meta prefix
  - keydown target guard (don't steal input from form fields)
  - spawn-error phase + retry (stuck "starting claude")
  - palette/bg stale-RGB repaint (S1); delta-on-stale-grid forceFull (S1)
  - cursor-over-scrollback `scrolled` flag (S1); stale-grid clear on switch (S2)
  - font re-measure on `document.fonts.ready` (S2); defensive font_size clamp
  - onSpawned re-check after await (S3 transient)

- **R2 — backend core**: `src-tauri/src/lib.rs`, `crates/workspace/src/service.rs`
  - Lagged pump: `match recv` continue on Lagged, break only on Closed
  - S3 folder-identity: pass `TESSERA_WORKSPACE_ID` env in spawn; hook command uses
    `${TESSERA_WORKSPACE_ID:-<id>}`; `run_hook` prefers env over argv

- **R3 — config + settings-ui**: `crates/core/src/config.rs`, `ui/src/lib/settings.ts`,
  `ui/src/SettingsModal.tsx`
  - tolerant per-field deserialize (one bad value must not wipe all)
  - font_size_px clamp 8..=32 at model boundary
  - Settings cancel must not revert externally-persisted zoom (reload on cleanup)

- **R4 — app shell ui**: `ui/src/App.tsx`, `ui/src/Sidebar.tsx`, `ui/src/index.css`
  - rename input survives status churn (mutate ref guard + controlled draft)
  - rename commit-on-blur
  - updater progress not clobbered by version-pill/poll; disable pill while downloading
  - quick-switch via `e.code` (AZERTY); reorder global unique sort_order
  - Appearance: apply `--font-ui` + `data-density`

- **R5 — pomodoro**: `src-tauri/src/commands.rs`, `crates/core/src/extras.rs`,
  `crates/store/src/extras.rs`, new migration
  - persist pre-pause mode (`paused_from`); resume restores it; cycle-credit only for Work

- **R6 — isolated modals**: `ui/src/ClaudeInventoryModal.tsx`, `ui/src/WorkspaceExtrasPanel.tsx`
  - inventory globals-only view (non-null source); extras-panel tab resync on workspace switch

## Verification
- `cargo test --workspace`
- `cd ui && npm run build` (tsc) + `npx vitest run`
- Opus code-review over `git diff main...HEAD`
- Manual S2 check needs a dev run (separate instance — never touch /home/save/tessera)

## ⚠️ Rebase residual — main moved to v0.1.10 mid-session (2026-05-29)

Remote `main` advanced from `6f4c2e2` (branch base) to `9d1f1f1`
("release: v0.1.10 — pin Claude session per workspace + archive") AFTER this
branch was cut. Draft PR #14 is open but conflicted. To make it mergeable:

1. **Migration collision:** v0.1.10 added `0009_claude_session_and_archive.sql`.
   Rename ours `0009_pomodoro_paused_from.sql` → `0010_…`, bump its entry in
   `migrations.rs` to version 10, and set the `migrations_are_idempotent`
   assertion in `store/src/lib.rs` to 10.
2. **S3 overlap (design):** v0.1.10 fixes the `--continue` *conversation* mixing
   via per-workspace `claude_session_id` + `--resume <id>` (the follow-up I'd
   deferred). Our S3 fix (`TESSERA_WORKSPACE_ID` env → hook/status/activity
   attribution) is ORTHOGONAL and still wanted. Reconcile in `service.rs`:
   keep the env injection in `spawn_session_inner`; their `spawn_agent` rewrite
   stays; merge `install_hooks` command change on top.
3. **Conflicting files to resolve on rebase onto `9d1f1f1`:** Cargo.lock,
   crates/store/src/{extras,lib,migrations}.rs, crates/workspace/src/service.rs,
   src-tauri/src/{commands,lib}.rs, ui/src/{App,Sidebar}.tsx, ui/src/index.css,
   ui/tests/Sidebar.test.tsx. (Their changes also touched workspace.rs struct
   with `claude_session_id`/`archived_at` — we don't touch the struct, so that
   side is clean.)
4. Re-run `cargo test --workspace` + `tsc` + `vite build` after resolving;
   force-push branch; PR #14 updates automatically.

## Constraints
- Follow DESIGN.md for any visual change. Surgical edits, match existing style.
- Commits authored `nvrxq <nvrxq@users.noreply.github.com>`. Never `gh`. Ask before push.
