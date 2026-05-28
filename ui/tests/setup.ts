import "@testing-library/jest-dom";
import { vi } from "vitest";

// Tauri IPC modules are loaded eagerly by `ui/src/lib/*.ts` at import
// time. Replace them with no-op stubs so component tests can mount
// without a backend round-trip. Individual tests can override these via
// `vi.mocked(invoke).mockResolvedValueOnce(...)` if they need a specific
// response.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: vi.fn().mockResolvedValue(false),
  message: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  readText: vi.fn().mockResolvedValue(""),
  writeText: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));
