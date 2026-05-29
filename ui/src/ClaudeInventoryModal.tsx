import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  type Component,
} from "solid-js";
import {
  claudeInventory,
  type ClaudeInventory,
  type McpServer,
  type McpSource,
  type Skill,
  type SkillSource,
} from "./lib/claudeInventory";

export interface ClaudeInventoryModalProps {
  /** Currently-selected workspace id, or null for the "all workspaces" view. */
  workspaceId: string | null;
  /** Display name of the selected workspace (for the context label). */
  workspaceLabel: string | null;
  onClose: () => void;
}

type Tab = "skills" | "mcp";

/**
 * Full-screen modal showing what Claude Code will see when launched in the
 * current workspace: skills (global + plugin-shipped + project-local) and
 * configured MCP servers. The backend never returns MCP env values — see
 * `tessera_core::claude` for the security regression guard.
 *
 * Style mirrors SettingsModal (surface-elevated card, border-subtle
 * dividers, terracotta accent) but goes wider — inventories can be long.
 */
const ClaudeInventoryModal: Component<ClaudeInventoryModalProps> = (props) => {
  const [tab, setTab] = createSignal<Tab>("skills");
  // Re-run the query whenever the workspace context flips. createResource's
  // source signal handles the refetch automatically.
  // Wrap in an object so the source is always truthy — a raw null source
  // would cause SolidJS to skip the fetcher entirely, leaving globals-only
  // view permanently empty. The fetcher unwraps back to the real id (null =
  // "all workspaces / globals only"), which the backend handles correctly.
  const [inv, { refetch }] = createResource(
    () => ({ id: props.workspaceId }),
    ({ id }) => claudeInventory(id),
  );

  const skills = createMemo<Skill[]>(() => inv()?.skills ?? []);
  const servers = createMemo<McpServer[]>(() => inv()?.mcp_servers ?? []);

  // Escape closes — bind on the document so the modal works regardless of
  // where focus landed. Bubble phase + no stopPropagation: matches
  // SettingsModal and avoids swallowing Escape from sibling overlays or
  // terminal keybindings when this modal isn't the topmost concern.
  const onDocKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      props.onClose();
    }
  };
  onMount(() => document.addEventListener("keydown", onDocKey));
  onCleanup(() => document.removeEventListener("keydown", onDocKey));

  // Auto-flip from an empty Skills tab to MCP — but only once per mount,
  // so a fresh-machine user (zero skills) who clicks back to Skills
  // doesn't get bounced to MCP again on the next refresh.
  let autoSwitched = false;
  createEffect(() => {
    if (inv.loading) return;
    if (autoSwitched) return;
    if (tab() === "skills" && skills().length === 0 && servers().length > 0) {
      autoSwitched = true;
      setTab("mcp");
    }
  });

  return (
    <div
      class="inventory-overlay"
      role="dialog"
      aria-modal="true"
      aria-label="Claude inventory"
      onClick={props.onClose}
      tabIndex={-1}
      ref={(el) => queueMicrotask(() => el.focus())}
    >
      <div class="inventory-modal" onClick={(e) => e.stopPropagation()}>
        <header class="inventory-head">
          <div class="inventory-head-text">
            <h2 class="inventory-title">Claude inventory</h2>
            <span class="inventory-subtitle" title={props.workspaceLabel ?? ""}>
              {props.workspaceLabel
                ? `Workspace: ${props.workspaceLabel}`
                : "All workspaces (globals only)"}
            </span>
          </div>
          <div class="inventory-head-actions">
            <button
              type="button"
              class="inventory-refresh"
              title="Refresh"
              aria-label="Refresh inventory"
              onClick={() => refetch()}
              disabled={inv.loading}
            >
              <svg viewBox="0 0 24 24" width="14" height="14" fill="none" aria-hidden="true">
                <path
                  d="M20 12a8 8 0 1 1-2.34-5.66"
                  stroke="currentColor"
                  stroke-width="1.6"
                  stroke-linecap="round"
                />
                <path
                  d="M20 4v4h-4"
                  stroke="currentColor"
                  stroke-width="1.6"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                />
              </svg>
            </button>
            <button
              type="button"
              class="inventory-close"
              aria-label="Close"
              title="Close"
              onClick={props.onClose}
            >
              ×
            </button>
          </div>
        </header>

        <div class="inventory-body">
          <nav class="inventory-nav" aria-label="Inventory sections">
            <TabButton
              id="skills"
              label="Skills"
              count={skills().length}
              current={tab()}
              onSelect={setTab}
            />
            <TabButton
              id="mcp"
              label="MCP servers"
              count={servers().length}
              current={tab()}
              onSelect={setTab}
            />
          </nav>
          <div class="inventory-pane">
            <Show when={!inv.loading} fallback={<LoadingState />}>
              <Show
                when={!inv.error}
                fallback={
                  <ErrorState
                    message={String(inv.error)}
                    onRetry={() => refetch()}
                    busy={inv.loading}
                  />
                }
              >
                <Show when={tab() === "skills"}>
                  <Show when={skills().length > 0} fallback={<EmptySkills />}>
                    <ul class="inventory-list">
                      <For each={skills()}>{(s) => <SkillCard skill={s} />}</For>
                    </ul>
                  </Show>
                </Show>
                <Show when={tab() === "mcp"}>
                  <Show when={servers().length > 0} fallback={<EmptyMcp />}>
                    <ul class="inventory-list">
                      <For each={servers()}>{(m) => <McpCard server={m} />}</For>
                    </ul>
                  </Show>
                </Show>
              </Show>
            </Show>
          </div>
        </div>
      </div>
    </div>
  );
};

interface TabButtonProps {
  id: Tab;
  label: string;
  count: number;
  current: Tab;
  onSelect: (t: Tab) => void;
}
const TabButton: Component<TabButtonProps> = (p) => (
  <button
    type="button"
    class="inventory-nav-item"
    classList={{ "inventory-nav-item--current": p.current === p.id }}
    onClick={() => p.onSelect(p.id)}
  >
    <span>{p.label}</span>
    <span class="inventory-nav-count">{p.count}</span>
  </button>
);

const SkillCard: Component<{ skill: Skill }> = (props) => {
  const [showPath, setShowPath] = createSignal(false);
  return (
    <li class="inventory-card">
      <header class="inventory-card-head">
        <span class="inventory-card-name">{props.skill.name}</span>
        <SourcePill source={props.skill.source} />
      </header>
      <Show when={props.skill.description}>
        <p class="inventory-card-desc">{props.skill.description}</p>
      </Show>
      <Show when={props.skill.triggers.length > 0}>
        <ul class="inventory-chips">
          <For each={props.skill.triggers}>
            {(t) => <li class="inventory-chip">{t}</li>}
          </For>
        </ul>
      </Show>
      <button
        type="button"
        class="inventory-disclosure"
        onClick={() => setShowPath((v) => !v)}
        aria-expanded={showPath()}
      >
        {showPath() ? "Hide path" : "Show path"}
      </button>
      <Show when={showPath()}>
        <code class="inventory-card-path" title={props.skill.path}>
          {props.skill.path}
        </code>
      </Show>
    </li>
  );
};

const McpCard: Component<{ server: McpServer }> = (props) => {
  const [showPath, setShowPath] = createSignal(false);
  return (
    <li class="inventory-card">
      <header class="inventory-card-head">
        <span class="inventory-card-name">{props.server.name}</span>
        <McpSourcePill source={props.server.source} />
        <Show when={props.server.kind}>
          <span class="inventory-kind">{props.server.kind}</span>
        </Show>
      </header>
      <Show when={props.server.command}>
        <code class="inventory-card-cmd">
          {props.server.command}
          {props.server.args.length > 0 ? " " : ""}
          {props.server.args.join(" ")}
        </code>
      </Show>
      <Show when={props.server.env_keys.length > 0}>
        <div class="inventory-env-row">
          <span class="inventory-env-label">env</span>
          <ul class="inventory-chips">
            <For each={props.server.env_keys}>
              {(k) => <li class="inventory-chip inventory-chip--env">{k}</li>}
            </For>
          </ul>
        </div>
      </Show>
      <button
        type="button"
        class="inventory-disclosure"
        onClick={() => setShowPath((v) => !v)}
        aria-expanded={showPath()}
      >
        {showPath() ? "Hide source" : "Show source"}
      </button>
      <Show when={showPath()}>
        <code class="inventory-card-path" title={props.server.config_path}>
          {props.server.config_path}
        </code>
      </Show>
    </li>
  );
};

const SourcePill: Component<{ source: SkillSource }> = (props) => {
  const kind = () => props.source.kind;
  const label = () => {
    const s = props.source;
    if (s.kind === "plugin") return `Plugin: ${s.plugin}`;
    if (s.kind === "global") return "Global";
    return "Project";
  };
  const title = () => {
    const s = props.source;
    if (s.kind === "plugin") {
      return `Marketplace ${s.marketplace} · plugin ${s.plugin} @ ${s.version}`;
    }
    if (s.kind === "global") return "User-global skill (~/.claude/skills)";
    return "Project-local skill (<workspace>/.claude/skills)";
  };
  return (
    <span
      class="inventory-source-pill"
      classList={{
        "inventory-source-pill--global": kind() === "global",
        "inventory-source-pill--plugin": kind() === "plugin",
        "inventory-source-pill--project": kind() === "project",
      }}
      title={title()}
    >
      {label()}
    </span>
  );
};

const McpSourcePill: Component<{ source: McpSource }> = (props) => (
  <span
    class="inventory-source-pill"
    classList={{
      "inventory-source-pill--global": props.source.kind === "global",
      "inventory-source-pill--project": props.source.kind === "project",
    }}
    title={
      props.source.kind === "global"
        ? "Configured in ~/.claude.json"
        : "Configured in <workspace>/.mcp.json"
    }
  >
    {props.source.kind === "global" ? "Global" : "Project"}
  </span>
);

const LoadingState: Component = () => (
  <div class="inventory-loading">Loading…</div>
);

const EmptyState: Component<{ message: string }> = (props) => (
  <section class="inventory-empty">
    <div class="inventory-empty-mosaic" aria-hidden="true">
      <span /><span /><span /><span />
      <span /><span /><span /><span />
      <span /><span /><span /><span />
    </div>
    <p class="inventory-empty-message">{props.message}</p>
  </section>
);

const EmptySkills: Component = () => (
  <EmptyState message="No skills found in ~/.claude/skills, plugin cache, or this workspace." />
);
const EmptyMcp: Component = () => (
  <EmptyState message="No MCP servers configured in ~/.claude.json or this workspace's .mcp.json." />
);

interface ErrorStateProps {
  message: string;
  onRetry: () => void;
  busy: boolean;
}
const ErrorState: Component<ErrorStateProps> = (props) => (
  <section class="inventory-error">
    <p class="inventory-error-title">Couldn't load inventory.</p>
    <p class="inventory-error-message">{props.message}</p>
    <button
      type="button"
      class="inventory-error-retry"
      onClick={props.onRetry}
      disabled={props.busy}
    >
      Retry
    </button>
  </section>
);

export default ClaudeInventoryModal;
