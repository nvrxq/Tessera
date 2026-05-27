import { invoke } from "@tauri-apps/api/core";
import type { PomodoroMode } from "./extras";

// Re-export shared helpers so PomodoroHud doesn't have to import from two
// places. The countdown formatter and the running-time math are identical
// for the global timer and the per-workspace one — no point duplicating.
export { formatMmSs } from "./extras";
export type { PomodoroMode } from "./extras";

/// Global, app-wide pomodoro state. Same fields as the per-workspace
/// `PomodoroState` minus `workspace_id` — the timer isn't tied to any one
/// workspace. Backed by the single-row `app_pomodoro` table.
export interface AppPomodoroState {
  mode: PomodoroMode;
  started_at: string | null;
  paused_at: string | null;
  target_seconds: number;
  elapsed_seconds_before_pause: number;
  cycles_completed: number;
  updated_at: string;
}

export function appPomodoroGet(): Promise<AppPomodoroState> {
  return invoke<AppPomodoroState>("app_pomodoro_get");
}

export function appPomodoroStart(
  mode: "work" | "break",
  targetSeconds?: number | null,
): Promise<AppPomodoroState> {
  return invoke<AppPomodoroState>("app_pomodoro_start", {
    mode,
    targetSeconds: targetSeconds ?? null,
  });
}

export function appPomodoroPause(): Promise<AppPomodoroState> {
  return invoke<AppPomodoroState>("app_pomodoro_pause");
}

export function appPomodoroResume(): Promise<AppPomodoroState> {
  return invoke<AppPomodoroState>("app_pomodoro_resume");
}

export function appPomodoroReset(): Promise<AppPomodoroState> {
  return invoke<AppPomodoroState>("app_pomodoro_reset");
}

/// Same math as `pomodoroRemainingSeconds` in `lib/extras.ts`, but typed
/// against the workspace-less `AppPomodoroState`.
export function appPomodoroRemainingSeconds(
  state: AppPomodoroState,
  nowMs: number,
): number {
  if (state.mode === "idle") return state.target_seconds;
  if (state.mode === "paused") {
    return Math.max(
      0,
      state.target_seconds - state.elapsed_seconds_before_pause,
    );
  }
  // work / break
  if (!state.started_at) return state.target_seconds;
  const startedMs = new Date(state.started_at).getTime();
  const elapsed = Math.max(0, Math.floor((nowMs - startedMs) / 1000));
  return Math.max(0, state.target_seconds - elapsed);
}
