# Plan: fix/session-identity-render

User report (v0.1.11 did NOT fix the originals): workspaces in the **same folder
still get each other's sessions**, visual **artifacts persist**, and the
**terminal (or half of it) can vanish**. Task: find ALL bugs, fix, report.

## Evidence-based root causes (verified by reading + empirical claude tests)

### RC1 — Same-folder session mixing (S3)  [CRITICAL]
`spawn_agent` pins a workspace to a claude session by **mtime detection**
(`schedule_session_id_detection` → `newest_session_id(cwd)`). With two
workspaces in one folder, the 1.5 s detection grabs whichever jsonl was
*touched last* — i.e. a **sibling's** session. Plus `--continue` resumes the
folder's newest conversation regardless of workspace.
Also: `encode_cwd` only maps `/`→`-`, but claude 2.x also maps `_` and `.`→`-`
(verified: `/tmp/tessera_sidtest` → `~/.claude/projects/-tmp-tessera-sidtest`),
so `session_file_exists`/detection silently look in the wrong dir for many paths.

**Fix:** own the identity. Generate a uuid per workspace, pass it to claude via
`claude --session-id <uuid>` (verified works, creates `<uuid>.jsonl`), and
`claude --resume <uuid>` once that jsonl exists (verified `--resume` hard-errors
on a missing id → must existence-check first, encoding-robust via uuid glob).
Delete the mtime detection thread and the `--continue` path entirely.

### RC2 — Artifacts / half-terminal on switch (S1)  [CRITICAL]
`applySnapshot` rebuilds the grid whenever `snap.full || sizeChanged`.
`setActiveSession` resets `gridCols=0`; a **delta** snapshot for the new
session that lands before the resize-triggered full has `sizeChanged===true`,
so its **partial** `cells` are treated as the whole grid → mostly-blank /
garbled frame ("half the terminal").
**Fix:** only re-baseline from `snap.full`. Drop a delta that arrives while the
mirror is reset (sizeChanged && !full) — the resize forces a full shortly.

### RC3 — Terminal vanishes when claude exits  [HIGH]
Backend emits `pty_event{exit}` + removes the session, but **no frontend
listener** clears the workspace's `session_id`. The dead id lingers; viewing /
reselecting that workspace binds to a dead session → no snapshots → blank
canvas, phase already "ready" so no overlay. Looks like the terminal vanished.
**Fix:** listen for `pty_event` exit in App; clear that workspace's session_id;
Terminal shows the spawn/error overlay (re-spawn affordance) instead of a void.

### RC4 — Backend double-spawn leak / mixing aggravator  [MED]
`spawn_agent` always spawns a NEW pty and overwrites `sessions[ws]`, orphaning
any prior claude for that workspace (more jsonls in the folder → worse mixing).
**Fix:** make backend spawn idempotent — reuse a live session for the workspace.

## Method
- Comprehensive multi-agent sweep (7 subsystems) with adversarial verification
  to avoid shipping non-fixes again. Merge with the RCs above.
- Implement, `cargo test --workspace` + ui tsc, then report to user.

## Constraints
- Never touch /home/save/tessera (running binary). Commits authored nvrxq.
  Never gh. DESIGN.md for any visual change.
