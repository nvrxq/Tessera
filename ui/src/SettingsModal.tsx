import {
  createEffect,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  type Component,
  type JSX,
} from "solid-js";
import {
  DEFAULT_ANSI_PALETTE,
  DEFAULT_CONFIG,
  saveSettings,
  setSettings,
  settings,
  settingsConfigPath,
  type CursorShape,
  type Density,
  type UserConfig,
} from "./lib/settings";
import SettingsPreview from "./SettingsPreview";

/** Debounce window (ms) for committing draft edits to the global
 *  `settings` signal. The main Terminal subscribes to that signal and a
 *  font change kicks off a backend resize round-trip; 60ms lets the
 *  user drag a slider smoothly while coalescing per-pixel updates into
 *  ~one IPC every refresh frame. The SettingsPreview reads the draft
 *  directly (no debounce) so the in-modal preview stays snappy. */
// 200 ms ≈ 5 commits/sec during a fast slider drag — still feels live in
// the preview, but doesn't flood the Rust side with one PTY-resize IPC
// per pixel. 60 ms (initial pick) was ~16 commits/sec which stuttered on
// slower hardware.
const DRAFT_COMMIT_DEBOUNCE_MS = 200;

export interface SettingsModalProps {
  onClose: () => void;
}

type SectionId = "appearance" | "terminal" | "cursor" | "behavior";

const FONT_OPTIONS = [
  '"Geist Mono", ui-monospace, Menlo, monospace',
  '"JetBrains Mono", ui-monospace, Menlo, monospace',
  '"Fira Code", ui-monospace, Menlo, monospace',
  "ui-monospace, Menlo, monospace",
  "Menlo, monospace",
];

const UI_FONT_OPTIONS = [
  "Geist, system-ui, -apple-system, sans-serif",
  "system-ui, -apple-system, sans-serif",
];

const HEX_RE = /^#[0-9A-Fa-f]{6}$/;
function isHex(s: string): boolean {
  return HEX_RE.test(s);
}

/**
 * Settings modal — left-nav + right-form layout (Linear-style). Reads the
 * live `settings` signal as the source of truth and mutates a local draft;
 * "Save" persists the draft via the Tauri command, which in turn fires
 * `settings_changed` so any other component (Terminal, App) reacts.
 *
 * Style follows DESIGN.md: surface-elevated background, border-subtle
 * dividers, terracotta accent on focus and the primary action.
 */
const SettingsModal: Component<SettingsModalProps> = (props) => {
  const [section, setSection] = createSignal<SectionId>("appearance");
  // Local draft copy so cancel can be a no-op. Deep-clone the palette
  // array so swatch edits don't mutate the live signal in place.
  const [draft, setDraft] = createSignal<UserConfig>(cloneConfig(settings()));
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [configPath, setConfigPath] = createSignal<string>("");

  // Snapshot the persisted settings on open so a Cancel can restore
  // them — the debounced live-commit below mutates the global signal
  // while the user edits, which is fine for previewing in the main
  // Terminal but would leak unsaved drafts if the user backs out.
  const initialSnapshot = cloneConfig(settings());
  let committed = false;

  onMount(() => {
    void settingsConfigPath().then(setConfigPath).catch(() => {});
  });

  // Debounced mirror: draft → global `settings`. The main Terminal
  // listens to `settings()` and a font / size change triggers a
  // backend resize round-trip, so we coalesce slider drags into ~one
  // commit per 60 ms instead of one per pixel. SettingsPreview reads
  // `draft` directly (no debounce) so the in-modal preview stays
  // pixel-perfect during drag.
  let commitTimer: number | null = null;
  let firstRun = true;
  createEffect(() => {
    const d = draft();
    if (firstRun) {
      // Skip the initial run — `draft` starts equal to the live
      // settings, so committing on first effect would be a no-op
      // that still cleared the timer prematurely.
      firstRun = false;
      return;
    }
    if (commitTimer != null) window.clearTimeout(commitTimer);
    commitTimer = window.setTimeout(() => {
      commitTimer = null;
      setSettings(d);
    }, DRAFT_COMMIT_DEBOUNCE_MS);
  });
  const flushPendingCommit = () => {
    if (commitTimer != null) {
      window.clearTimeout(commitTimer);
      commitTimer = null;
    }
  };
  onCleanup(() => {
    flushPendingCommit();
    // If the modal is torn down without Save being clicked, revert any
    // debounced live-preview edits so the user's "cancel" intent is
    // honoured and the persisted config remains the source of truth.
    if (!committed) setSettings(initialSnapshot);
  });

  function cloneConfig(c: UserConfig): UserConfig {
    return {
      ...c,
      appearance: { ...c.appearance },
      terminal: { ...c.terminal, palette: c.terminal.palette.slice() },
      behavior: { ...c.behavior },
    };
  }

  /** Tiny helper that produces an updater closure for a `(group, field)`
   *  pair — keeps the JSX from being a wall of nested-spread expressions. */
  function patch<K extends keyof UserConfig>(
    group: K,
    update: (g: UserConfig[K]) => UserConfig[K],
  ) {
    setDraft((d) => ({ ...d, [group]: update(d[group]) }));
  }

  const onSave = async () => {
    setSaving(true);
    setError(null);
    try {
      const cfg = draft();
      // Client-side validation mirrors the Rust HexColor check — surface
      // errors here so the user gets immediate feedback instead of a
      // generic "save failed" toast from a serde rejection.
      if (!isHex(cfg.terminal.background)) {
        throw new Error("Terminal background must be a #RRGGBB hex colour.");
      }
      if (!isHex(cfg.terminal.foreground)) {
        throw new Error("Terminal foreground must be a #RRGGBB hex colour.");
      }
      if (!isHex(cfg.terminal.cursor_color)) {
        throw new Error("Cursor colour must be a #RRGGBB hex colour.");
      }
      for (let i = 0; i < cfg.terminal.palette.length; i++) {
        if (!isHex(cfg.terminal.palette[i])) {
          throw new Error(`ANSI slot ${i} is not a valid #RRGGBB hex colour.`);
        }
      }
      // Pre-empt the in-flight debounced commit so it can't fire
      // *after* save with a stale draft snapshot.
      flushPendingCommit();
      await saveSettings(cfg);
      // Mirror the saved state into the live signal immediately — the
      // settings_changed event will also fire and is a safety net, but
      // updating synchronously here avoids a one-frame flash where the
      // modal closes before the event round-trips.
      setSettings(cfg);
      // Mark as committed so onCleanup doesn't roll back to the
      // pre-modal snapshot now that the draft is persisted.
      committed = true;
      props.onClose();
    } catch (e) {
      // Save bombed mid-flight (file write, validation race, etc.).
      // Roll the live `settings` signal back to the pre-modal snapshot —
      // otherwise the user is left with a partially-committed draft (the
      // last debounced commit before the save attempt) reflected in the
      // main Terminal, even though the modal is still open showing the
      // error and any prior Save effort wasn't persisted on disk.
      setSettings(initialSnapshot);
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const onReset = () => {
    setDraft(cloneConfig(DEFAULT_CONFIG));
  };

  // Escape should always close the modal regardless of focus location.
  // Bind at document level for the modal's lifetime; ProjectsSettings
  // uses the same pattern.
  const onDocKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      props.onClose();
    }
  };
  onMount(() => document.addEventListener("keydown", onDocKey, true));
  onCleanup(() => document.removeEventListener("keydown", onDocKey, true));

  return (
    <div
      class="settings-overlay"
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
      onClick={props.onClose}
      tabIndex={-1}
      ref={(el) => queueMicrotask(() => el.focus())}
    >
      <div class="settings-modal" onClick={(e) => e.stopPropagation()}>
        <header class="settings-head">
          <h2 class="settings-title">Settings</h2>
          <button
            type="button"
            class="settings-close"
            aria-label="Close"
            title="Close"
            onClick={props.onClose}
          >
            ×
          </button>
        </header>

        <div class="settings-body">
          <nav class="settings-nav" aria-label="Settings sections">
            <SectionLink
              id="appearance"
              label="Appearance"
              current={section()}
              onSelect={setSection}
            />
            <SectionLink
              id="terminal"
              label="Terminal"
              current={section()}
              onSelect={setSection}
            />
            <SectionLink
              id="cursor"
              label="Cursor"
              current={section()}
              onSelect={setSection}
            />
            <SectionLink
              id="behavior"
              label="Behavior"
              current={section()}
              onSelect={setSection}
            />
          </nav>

          <div class="settings-pane">
            <Show when={section() === "appearance"}>
              <AppearanceSection draft={draft()} patch={patch} />
            </Show>
            <Show when={section() === "terminal"}>
              <TerminalSection draft={draft()} patch={patch} />
            </Show>
            <Show when={section() === "cursor"}>
              <CursorSection draft={draft()} patch={patch} />
            </Show>
            <Show when={section() === "behavior"}>
              <BehaviorSection draft={draft()} patch={patch} />
            </Show>
          </div>

          {/* Live preview column — sticky to the right so every section
              (Appearance / Terminal / Cursor / Behavior) sees its edits
              reflected on the synthetic mini-terminal without scrolling.
              The pane reads the *draft* signal, not the persisted one, so
              changes apply per-keystroke before Save. */}
          <aside class="settings-preview-pane" aria-label="Live preview">
            <span class="settings-preview-label">Preview</span>
            <SettingsPreview cfg={draft} />
          </aside>
        </div>

        <footer class="settings-foot">
          <Show when={configPath()}>
            <span class="settings-path" title={configPath()}>
              {configPath()}
            </span>
          </Show>
          <div class="settings-foot-actions">
            <Show when={error()}>
              <span class="settings-error">{error()}</span>
            </Show>
            <button type="button" class="settings-secondary" onClick={onReset}>
              Reset
            </button>
            <button
              type="button"
              class="settings-primary"
              disabled={saving()}
              onClick={onSave}
            >
              {saving() ? "Saving…" : "Save"}
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
};

interface SectionLinkProps {
  id: SectionId;
  label: string;
  current: SectionId;
  onSelect: (s: SectionId) => void;
}
const SectionLink: Component<SectionLinkProps> = (p) => (
  <button
    type="button"
    class="settings-nav-item"
    classList={{ "settings-nav-item--current": p.current === p.id }}
    onClick={() => p.onSelect(p.id)}
  >
    {p.label}
  </button>
);

// ── Section bodies ─────────────────────────────────────────────────────────
// Each section reads `draft` (a snapshot of the current local edit) and
// calls `patch` to push partial updates. We avoid Solid stores here so
// the modal stays close to the JSON shape it actually saves.

type Patch = <K extends keyof UserConfig>(
  group: K,
  update: (g: UserConfig[K]) => UserConfig[K],
) => void;

const AppearanceSection: Component<{ draft: UserConfig; patch: Patch }> = (p) => (
  <div class="settings-section">
    <h3 class="settings-section-title">Appearance</h3>
    <Field label="UI font family">
      <select
        class="settings-select"
        value={p.draft.appearance.ui_font_family}
        onChange={(e) =>
          p.patch("appearance", (a) => ({
            ...a,
            ui_font_family: e.currentTarget.value,
          }))
        }
      >
        <For each={UI_FONT_OPTIONS}>
          {(f) => <option value={f}>{f.split(",")[0].replace(/"/g, "")}</option>}
        </For>
      </select>
    </Field>
    <Field label="Density">
      <div class="settings-radio-row">
        <For each={["compact", "comfortable"] as Density[]}>
          {(d) => (
            <label class="settings-radio">
              <input
                type="radio"
                name="density"
                checked={p.draft.appearance.density === d}
                onChange={() =>
                  p.patch("appearance", (a) => ({ ...a, density: d }))
                }
              />
              <span>{d}</span>
            </label>
          )}
        </For>
      </div>
    </Field>
  </div>
);

const TerminalSection: Component<{ draft: UserConfig; patch: Patch }> = (p) => (
  <div class="settings-section">
    <h3 class="settings-section-title">Terminal</h3>
    <Field label="Font family">
      <select
        class="settings-select"
        value={p.draft.terminal.font_family}
        onChange={(e) =>
          p.patch("terminal", (t) => ({
            ...t,
            font_family: e.currentTarget.value,
          }))
        }
      >
        <For each={FONT_OPTIONS}>
          {(f) => <option value={f}>{f.split(",")[0].replace(/"/g, "")}</option>}
        </For>
      </select>
    </Field>
    <Field label={`Font size — ${p.draft.terminal.font_size_px}px`}>
      <input
        type="range"
        min={8}
        max={32}
        step={1}
        value={p.draft.terminal.font_size_px}
        onInput={(e) =>
          p.patch("terminal", (t) => ({
            ...t,
            font_size_px: Number(e.currentTarget.value),
          }))
        }
      />
    </Field>
    <Field label="Background">
      <ColorField
        value={p.draft.terminal.background}
        onChange={(v) =>
          p.patch("terminal", (t) => ({ ...t, background: v }))
        }
      />
    </Field>
    <Field label="Foreground">
      <ColorField
        value={p.draft.terminal.foreground}
        onChange={(v) =>
          p.patch("terminal", (t) => ({ ...t, foreground: v }))
        }
      />
    </Field>
    <Field label="ANSI palette (0–15)">
      <div class="settings-palette">
        <For each={p.draft.terminal.palette}>
          {(c, i) => (
            <ColorSwatch
              index={i()}
              value={c}
              onChange={(v) =>
                p.patch("terminal", (t) => {
                  const next = t.palette.slice();
                  next[i()] = v;
                  return { ...t, palette: next };
                })
              }
            />
          )}
        </For>
      </div>
      <button
        type="button"
        class="settings-inline-action"
        onClick={() =>
          p.patch("terminal", (t) => ({
            ...t,
            palette: DEFAULT_ANSI_PALETTE.slice(),
          }))
        }
      >
        Reset palette
      </button>
    </Field>
  </div>
);

const CursorSection: Component<{ draft: UserConfig; patch: Patch }> = (p) => (
  <div class="settings-section">
    <h3 class="settings-section-title">Cursor</h3>
    <Field label="Shape">
      <div class="settings-radio-row">
        <For each={["block", "bar", "underline"] as CursorShape[]}>
          {(s) => (
            <label class="settings-radio">
              <input
                type="radio"
                name="cursor-shape"
                checked={p.draft.terminal.cursor_shape === s}
                onChange={() =>
                  p.patch("terminal", (t) => ({ ...t, cursor_shape: s }))
                }
              />
              <span>{s}</span>
            </label>
          )}
        </For>
      </div>
    </Field>
    <Field label="Color">
      <ColorField
        value={p.draft.terminal.cursor_color}
        onChange={(v) =>
          p.patch("terminal", (t) => ({ ...t, cursor_color: v }))
        }
      />
    </Field>
    <Field label="">
      <label class="settings-checkbox">
        <input
          type="checkbox"
          checked={p.draft.terminal.cursor_blink}
          onChange={(e) =>
            p.patch("terminal", (t) => ({
              ...t,
              cursor_blink: e.currentTarget.checked,
            }))
          }
        />
        <span>Blink cursor</span>
      </label>
    </Field>
  </div>
);

const BehaviorSection: Component<{ draft: UserConfig; patch: Patch }> = (p) => (
  <div class="settings-section">
    <h3 class="settings-section-title">Behavior</h3>
    <Field label="">
      <label class="settings-checkbox">
        <input
          type="checkbox"
          checked={p.draft.behavior.auto_spawn_on_workspace_open}
          onChange={(e) =>
            p.patch("behavior", (b) => ({
              ...b,
              auto_spawn_on_workspace_open: e.currentTarget.checked,
            }))
          }
        />
        <span>Auto-spawn agent when a workspace is opened</span>
      </label>
    </Field>
    <Field label={`Scrollback (${p.draft.behavior.save_scrollback_lines} lines)`}>
      <input
        type="range"
        min={500}
        max={20000}
        step={100}
        value={p.draft.behavior.save_scrollback_lines}
        onInput={(e) =>
          p.patch("behavior", (b) => ({
            ...b,
            save_scrollback_lines: Number(e.currentTarget.value),
          }))
        }
      />
    </Field>
  </div>
);

// ── Atoms ──────────────────────────────────────────────────────────────────

const Field: Component<{ label: string; children?: JSX.Element }> = (p) => (
  <label class="settings-field">
    <Show when={p.label}>
      <span class="settings-field-label">{p.label}</span>
    </Show>
    <div class="settings-field-input">{p.children}</div>
  </label>
);

const ColorField: Component<{
  value: string;
  onChange: (v: string) => void;
}> = (p) => (
  <div class="settings-color-row">
    <input
      type="color"
      class="settings-color"
      value={p.value}
      onInput={(e) => p.onChange(e.currentTarget.value.toUpperCase())}
    />
    <input
      type="text"
      class="settings-input settings-input--hex"
      value={p.value}
      maxLength={7}
      spellcheck={false}
      onInput={(e) => p.onChange(e.currentTarget.value)}
    />
  </div>
);

const ColorSwatch: Component<{
  index: number;
  value: string;
  onChange: (v: string) => void;
}> = (p) => (
  <label class="settings-swatch" title={`ANSI ${p.index}`}>
    <input
      type="color"
      value={p.value}
      onInput={(e) => p.onChange(e.currentTarget.value.toUpperCase())}
    />
    <span
      class="settings-swatch-chip"
      style={`background-color: ${p.value}`}
      aria-label={`ANSI ${p.index}: ${p.value}`}
    />
    <span class="settings-swatch-label">{p.index}</span>
  </label>
);

export default SettingsModal;
