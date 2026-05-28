import { describe, it, expect } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import Sidebar from "../src/Sidebar";

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
});
