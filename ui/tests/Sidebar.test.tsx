import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@solidjs/testing-library";
import Sidebar from "../src/Sidebar";
import type { WorkspaceDto } from "../src/lib/workspaces";

function makeWorkspace(overrides: Partial<WorkspaceDto> = {}): WorkspaceDto {
  return {
    id: "ws-1",
    name: "alpha",
    repo_path: "/tmp/alpha",
    worktree_path: "/tmp/alpha",
    setup_status: { kind: "ok" },
    project_id: null,
    session_id: null,
    agent_status: null,
    sort_order: 10,
    created_at: "2026-01-01T00:00:00Z",
    detected_worktree: null,
    detected_branch: null,
    dangerous_skip_permissions: false,
    ...overrides,
  } as WorkspaceDto;
}

/** Open the inline rename input for the first (only) workspace row by
 *  clicking the ⋯ menu then "Rename". Returns the live <input>. */
function openRename(): HTMLInputElement {
  fireEvent.click(screen.getByTitle("Workspace options"));
  fireEvent.click(screen.getByText("Rename"));
  return screen.getByDisplayValue("alpha") as HTMLInputElement;
}

describe("Sidebar", () => {
  it("shows the empty-state message when no workspaces are loaded", () => {
    render(() => (
      <Sidebar
        workspaces={[]}
        archived={[]}
        projects={[]}
        selectedId={null}
        onSelect={() => {}}
        onDelete={() => {}}
        onNew={() => {}}
        onReorder={() => {}}
        onAssignProject={() => {}}
        onRename={() => {}}
        onArchive={() => {}}
        onUnarchive={() => {}}
        onResetSession={() => {}}
      />
    ));
    expect(screen.getByText("No workspaces yet")).toBeInTheDocument();
  });

  it("commits a rename on Enter (BUG 2)", () => {
    const onRename = vi.fn();
    render(() => (
      <Sidebar
        workspaces={[makeWorkspace()]}
        projects={[]}
        selectedId={null}
        onSelect={() => {}}
        onDelete={() => {}}
        onNew={() => {}}
        onReorder={() => {}}
        onAssignProject={() => {}}
        onRename={onRename}
      />
    ));
    const input = openRename();
    fireEvent.input(input, { target: { value: "beta" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onRename).toHaveBeenCalledTimes(1);
    expect(onRename).toHaveBeenCalledWith("ws-1", "beta");
  });

  it("commits a rename on blur instead of discarding it (BUG 2)", () => {
    const onRename = vi.fn();
    render(() => (
      <Sidebar
        workspaces={[makeWorkspace()]}
        projects={[]}
        selectedId={null}
        onSelect={() => {}}
        onDelete={() => {}}
        onNew={() => {}}
        onReorder={() => {}}
        onAssignProject={() => {}}
        onRename={onRename}
      />
    ));
    const input = openRename();
    fireEvent.input(input, { target: { value: "gamma" } });
    fireEvent.blur(input);
    expect(onRename).toHaveBeenCalledTimes(1);
    expect(onRename).toHaveBeenCalledWith("ws-1", "gamma");
  });

  it("does not double-commit when Enter is followed by blur (BUG 2)", () => {
    const onRename = vi.fn();
    render(() => (
      <Sidebar
        workspaces={[makeWorkspace()]}
        projects={[]}
        selectedId={null}
        onSelect={() => {}}
        onDelete={() => {}}
        onNew={() => {}}
        onReorder={() => {}}
        onAssignProject={() => {}}
        onRename={onRename}
      />
    ));
    const input = openRename();
    fireEvent.input(input, { target: { value: "delta" } });
    // Enter handler calls blur(); jsdom fires blur synchronously, but we
    // also fire it explicitly to model the real DOM event order.
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.blur(input);
    expect(onRename).toHaveBeenCalledTimes(1);
  });

  it("cancels (no commit) on Escape", () => {
    const onRename = vi.fn();
    render(() => (
      <Sidebar
        workspaces={[makeWorkspace()]}
        projects={[]}
        selectedId={null}
        onSelect={() => {}}
        onDelete={() => {}}
        onNew={() => {}}
        onReorder={() => {}}
        onAssignProject={() => {}}
        onRename={onRename}
      />
    ));
    const input = openRename();
    fireEvent.input(input, { target: { value: "epsilon" } });
    fireEvent.keyDown(input, { key: "Escape" });
    fireEvent.blur(input);
    expect(onRename).not.toHaveBeenCalled();
  });

  it("does not commit an unchanged or empty name", () => {
    const onRename = vi.fn();
    render(() => (
      <Sidebar
        workspaces={[makeWorkspace()]}
        projects={[]}
        selectedId={null}
        onSelect={() => {}}
        onDelete={() => {}}
        onNew={() => {}}
        onReorder={() => {}}
        onAssignProject={() => {}}
        onRename={onRename}
      />
    ));
    const input = openRename();
    fireEvent.input(input, { target: { value: "   " } });
    fireEvent.blur(input);
    expect(onRename).not.toHaveBeenCalled();
  });
});
