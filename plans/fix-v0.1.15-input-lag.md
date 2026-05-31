# v0.1.15 — input lag + session resume

## Problem (reported by user, 2026-05-29, running auto-updated v0.1.14)
1. **Slow input** into Claude Code. Confirmed laggy in BOTH the Claude pane and a
   companion `+ shell` pane → not Claude-specific, it's the shared WebView
   render pipeline.
2. **No session resume** — every workspace opens "from scratch" (`никакой сессии нет`).

## Evidence gathered (static + live, no GUI)
- Live DB (`~/.local/share/tessera/state.db`, read-only): 7 workspaces, 5 pinned
  `claude_session_id`, 2 NULL. **None of the 5 pinned uuids has a matching
  `<uuid>.jsonl` anywhere** under `~/.claude/projects/`.
- `ps`: the running Tessera (not the off-limits `/home/save/tessera`; the
  auto-updated 0.1.14) has 4 live claude children, **all `--session-id <uuid>`,
  none `--resume`**, alive ~50 min, **zero session files written** (checked via
  `find` + `/proc/<pid>/fd` — no jsonl open). Manual claude sessions in the same
  folders persist fine.
- Claude 2.1.156 contract verified by controlled probe:
  `claude --session-id X` (X absent) → creates `X.jsonl`;
  `--session-id X` (X present) → `Error: already in use`, exits;
  `--resume X` (present) → resumes. So Tessera's resume logic is correct in
  isolation — it just never has a file to resume.
- Env of a live claude child (`/proc`): `TERM=xterm-256color`,
  `COLORTERM=truecolor` (inherited), correct cwd. So TERM is NOT the cause here.
- GPU: **NVIDIA (proprietary) + AMD iGPU hybrid**; **X11 + i3** (no compositor);
  **WebKitGTK 2.52.3**.
- `src-tauri/src/main.rs` (in v0.1.14 / HEAD) force-sets
  `WEBKIT_DISABLE_DMABUF_RENDERER=1`. This was added in the Canvas2D era
  (commit 5a8f7ed) for the 2.42-era DMABUF bug.

## Root-cause chain
Forcing `WEBKIT_DISABLE_DMABUF_RENDERER=1` on **WebKitGTK 2.52** pushes the
webview onto a CPU-side compositing fallback. On this NVIDIA/i3/X11 box that
recomposites the whole webview per frame → laggy input in every pane → Claude is
too painful to drive → sessions stay empty → never persist → never resumable →
"from scratch". The two bugs are one chain; fixing the compositing lag unwinds it.

## Plan
1. **`main.rs` — tunable WebKit GPU mode** (this branch). Default to *not*
   force-disabling DMABUF (trust 2.52's GPU renderer). Keep an escape hatch env
   `TESSERA_GPU` for diverse hardware:
   - unset / `auto` (new default): set nothing → WebKitGTK uses its DMABUF/GPU path.
   - `no-dmabuf`: `WEBKIT_DISABLE_DMABUF_RENDERER=1` (old behaviour).
   - `software` / `off`: `WEBKIT_DISABLE_COMPOSITING_MODE=1` (+ dmabuf off) — full CPU.
   Always respect a pre-existing user-exported var (don't override).
2. **User A/B test** (one `npm run tauri dev` session): compare default vs
   `TESSERA_GPU=no-dmabuf` vs `TESSERA_GPU=software` — pick the snappiest that
   also renders correctly (NVIDIA dmabuf can black-screen → that's why we test
   before releasing). Bake the winner as the default.
3. **Robustness (cheap):** set `TERM=xterm-256color` + `COLORTERM=truecolor`
   explicitly on the Claude spawn (`spawn_session_inner`), matching
   `pty_spawn_shell` — so desktop-launched instances (no inherited TERM) aren't
   degraded.
4. **Verify resume end-to-end:** with input usable, have a short conversation in
   a workspace, restart Tessera, confirm it resumes (jsonl now exists → `--resume`).
5. cargo test/fmt/clippy + Docker bun sandbox (frontend unchanged, but CI parity)
   → bump 0.1.15 → commit (nvrxq) → tag → push → release.yml.

## Paste freeze (3rd symptom, reported mid-flow)
"Ctrl+V freezes everything, nothing pastes." Causes found (all pre-existing,
surfaced now):
- `navigator.clipboard.readText()` (rendererPool) — unreliable / can block the
  webview on WebKitGTK, esp. with non-text/image clipboard content.
- O(n²) base64 build (`bin += String.fromCharCode(b)`) in `rendererPool.writeToPty`
  AND `termSession.writeLeaf` — a large paste/screenshot froze the main thread.
- Paste bound to Ctrl+Shift+V only on Linux; plain Ctrl+V fell to the flaky
  native path.
- `save_paste_image` backend command orphaned (no caller) → image paste dead.

Fix (frontend + capability):
- `ipc.ts`: shared `encodeBytesToB64` (32 KB chunked, linear). Used by both write paths.
- `rendererPool.ts`: copy/paste via Tauri clipboard plugin (`readText`/`writeText`/
  `readImage`), not `navigator.clipboard`. New `pasteFromClipboard` (text-first,
  image-fallback) + `saveClipboardImage` (RGBA→PNG via canvas → `save_paste_image`
  → bracketed-paste the path = Claude attachment). Bind plain **Ctrl+V** (and
  Ctrl+Shift+V), preventDefault to kill the native double/freeze path.
- `capabilities/default.json`: + `clipboard-manager:allow-read-image`.
Verified: tsc clean (only pre-existing Sidebar.test debt), `vite build` ok,
`cargo check` ok (capability id valid).

## Status
- [x] branch + plan
- [x] main.rs tunable WebKit GPU change (default = GPU DMABUF + TESSERA_GPU knob)
- [x] TERM/COLORTERM on claude spawn
- [x] paste fix (clipboard plugin + chunked b64 + Ctrl+V + image paste)
- [x] build verify (cargo check/fmt/clippy, tsc, vite build)
- [x] bump 0.1.15, commit (nvrxq, 0c1e0e4), ff main, tag, push, release run #26 SUCCESS (published, latest.json v0.1.15)
- [ ] user confirms GPU default is snappy (+ paste/resume) on the real app; else flip TESSERA_GPU default → 0.1.16
User chose "ship now, tune live via env" (default = DMABUF/GPU on).
