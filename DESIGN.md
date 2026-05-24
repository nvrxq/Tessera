# Design System — Tessera

## Product Context

- **What this is:** Linux-first desktop orchestrator for parallel CLI coding agents (Claude Code), running each in its own folder/worktree.
- **Who it's for:** Developers who run multiple AI coding sessions in parallel — power users of Claude Code, Codex, etc.
- **Space/industry:** Developer tools / AI tooling.
- **Project type:** Desktop app (Tauri 2 + SolidJS + xterm.js).
- **Reference products:** Linear (typography & density), Warp (terminal-as-product polish), Raycast (compact info-density).

## Aesthetic Direction

- **Direction:** Industrial / Editorial Hybrid.
- **Decoration level:** Minimal-Intentional. No decorative blobs, no gradients-for-the-sake-of-it. One signature motif: a subtle mosaic-tile pattern in empty states that plays on the product name (*tessera* = a single mosaic tile).
- **Mood:** Calm, considered, dense. The tool should feel like a workshop, not a marketing page. Warm dark theme to differentiate from the standard cool blue-gray Linear/Warp palette.

## Typography

All UI text is **Geist** (`@vercel/geist-sans` / Google Fonts `Geist`). Monospace contexts use **Geist Mono**.

- **Display / Hero / Headings:** Geist, weight 500–600.
- **Body / UI:** Geist, weight 400.
- **Labels / Small UI:** Geist, weight 500, slightly tighter letter-spacing.
- **Monospace (paths, branch names, commit hashes, status labels):** Geist Mono, weight 400.
- **Terminal content:** xterm.js default (already `ui-monospace, Menlo, monospace`). Left alone — terminal output formatting is the agent's job, not ours.
- **Loading strategy:** Google Fonts via `<link>` in `index.html` with `display=swap`. Subset to latin only.

### Scale (px)

| Token | Size | Line height | Use |
|-------|------|-------------|-----|
| 2xs   | 11   | 14          | Status badges, key combos |
| xs    | 12   | 16          | Sidebar branch, muted metadata |
| sm    | 13   | 18          | Body text, buttons |
| md    | 14   | 20          | Workspace names |
| lg    | 16   | 22          | Section headers |
| xl    | 20   | 26          | Page titles |
| 2xl   | 28   | 32          | Hero (rare, marketing only) |

## Color

Warm dark theme. Restrained palette — one accent, neutrals, semantic colors only when they mean something.

```
--bg                    #0F0F10   /* app background */
--surface               #141415   /* main pane, primary surface */
--surface-elevated      #1A1A1C   /* sidebar, modals, popovers */
--surface-hover         #1F1F22   /* hover state on list items */
--surface-selected      #232328   /* selected workspace */

--border-subtle         #26262A   /* dividers, card borders */
--border-strong         #34343A   /* focus rings, input borders */

--text-primary          #E8E8E6   /* main text */
--text-secondary        #B8B8B4   /* secondary labels */
--text-muted            #8A8A86   /* metadata, branch names */
--text-disabled         #5A5A57

--accent                #C8825B   /* terracotta — primary accent */
--accent-hover          #D89568
--accent-quiet          #6E4831   /* subdued backgrounds (e.g., selected tab bar) */

--success               #7FBD7F   /* agent: done */
--working               #5B8DEF   /* agent: working */
--needs-input           #E6C84C   /* agent: needs input */
--crashed               #D96666   /* agent: crashed */
--idle                  #6B6B6B   /* agent: idle */
```

Light mode is out of scope for v1 — this is a tool you live in at night.

## Spacing

Base unit: **4px**. Density: compact (Linear-style).

| Token | Px  | Use |
|-------|-----|-----|
| 2xs   | 2   | Tight inline (badge padding) |
| xs    | 4   | Icon-to-text gaps |
| sm    | 8   | Form field gaps, list item padding |
| md    | 12  | Section padding, card body |
| lg    | 16  | Major section gaps |
| xl    | 24  | Page padding |
| 2xl   | 32  | Hero margins |

## Layout

- **Approach:** Grid-disciplined with a strong sidebar/main split.
- **App grid:** `grid-template-columns: 240px 1fr` (sidebar was 220px; bumped 20px for breathing room).
- **Max content width:** none in app surfaces — they fill the window. Marketing site (future) would cap at 1200px.
- **Border radius:** hierarchical, lower than Tailwind defaults to keep the industrial feel.

| Token | Px  | Use |
|-------|-----|-----|
| sm    | 4   | Inputs, status dots |
| md    | 6   | Buttons, cards, workspace items |
| lg    | 10  | Modals, large surfaces |
| full  | 9999 | Avatars (none yet) |

## Motion

Minimal-functional everywhere — except one signature exception.

- **Easing:** `cubic-bezier(0.16, 1, 0.3, 1)` for enter, `ease-out` for exit, `ease-in-out` for layout shifts.
- **Duration:** 120ms (micro state — hover, focus ring), 180ms (selection, panel switch), 240ms (workspace mount).

**Signature exception — breathing status dot:**
The `working` agent status dot pulses at 1.6s `ease-in-out` (`opacity: 1 → 0.55 → 1`, `box-shadow: 0 → 8px → 0`). All other statuses are static. The breathing pulse signals "your agent is alive and thinking" without needing a spinner.

## Components

### Status dots
- Size: 8px (slightly larger than current 8px — actually keep at 8).
- Inset ring: 1px of `--surface-elevated` for definition against any background.
- Per-status: solid color, no gradient. `working` gets the breathing animation.

### Workspace items (sidebar list)
- Padding: `8px 12px`.
- Left border: 2px, transparent by default; `--accent` when selected.
- Hover: `--surface-hover` with 120ms transition.
- Selected: `--surface-selected` + accent left-border.
- Layout: `[status dot] [name + subline (truncate)] [× delete on hover]`.

### Empty state (Sidebar with no workspaces, main pane with nothing selected)
Subtle mosaic motif:
- Background: 4–6 small terracotta-tinted tiles at low opacity (0.04–0.10), rotated at small angles, scattered across a small area.
- Centered tagline in `--text-muted`: "Select a workspace or create a new one."
- Implemented as inline SVG so it scales with the surface.

### New workspace form
- Sits in the main pane, max-width 480px, left-aligned.
- Inputs: `--surface-elevated` background, `--border-subtle` border, focus ring `--accent` 1px.
- Buttons: primary uses `--accent` background with `--bg` text; secondary uses `--surface-elevated` with `--border-strong`.

### Header
- Height: 36px.
- Background: `--surface-elevated`.
- Padding: `0 16px`.
- Logo / wordmark on the left: "Tessera" in Geist 500, 14px, with the `t` painted in `--accent` for personality.

### Terminal pane
- Padding: 8px around the xterm container.
- Background: `--surface`. xterm's own background (`#1e1e1e`) is overridden to `--surface` to match.

## Decisions Log

| Date       | Decision | Rationale |
|------------|----------|-----------|
| 2026-05-24 | Initial design system created | Codified by `/design-consultation` based on Tessera's product context (developer tool, Linear/Warp references, mosaic-tile name metaphor). |
| 2026-05-24 | Warm dark + terracotta accent | Differentiates from cool-blue Linear/Warp; plays on `tessera` = mosaic tile. Risk worth taking — keeps the tool memorable. |
| 2026-05-24 | Breathing status dot for `working` | Signals agent activity without a spinner; cheap to implement; only motion exception in the system. |
| 2026-05-24 | Subtle mosaic pattern in empty states | Reinforces the product name without becoming a logo. Quiet, only visible when the surface is otherwise empty. |
