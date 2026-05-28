import { describe, it, expect } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import Sidebar from "../src/Sidebar";

describe("Sidebar", () => {
  it("shows the empty-state message when no workspaces are loaded", () => {
    render(() => (
      <Sidebar
        workspaces={[]}
        projects={[]}
        selectedId={null}
        onSelect={() => {}}
        onDelete={() => {}}
        onNew={() => {}}
        onReorder={() => {}}
        onAssignProject={() => {}}
        onRename={() => {}}
      />
    ));
    expect(screen.getByText("No workspaces yet")).toBeInTheDocument();
  });
});
