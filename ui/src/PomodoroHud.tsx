import {
  createSignal,
  onCleanup,
  onMount,
  Show,
  type Component,
} from "solid-js";
import {
  appPomodoroGet,
  appPomodoroPause,
  appPomodoroRemainingSeconds,
  appPomodoroReset,
  appPomodoroResume,
  appPomodoroStart,
  formatMmSs,
  type AppPomodoroState,
} from "./lib/pomodoro";

// localStorage key for the popover's open/closed state. The HUD is global,
// so the key isn't workspace-scoped — flipping it open on one workspace
// keeps it open everywhere.
const POPOVER_OPEN_KEY = "tessera.pomodoroHudOpen";

const PomodoroHud: Component = () => {
  const [state, setState] = createSignal<AppPomodoroState | null>(null);
  // Bumped every second so the countdown re-renders without re-fetching
  // from the backend. The server-side `started_at` is the source of truth
  // — local ticks just sample the wall clock.
  const [now, setNow] = createSignal(Date.now());
  const [popoverOpen, setPopoverOpen] = createSignal(
    localStorage.getItem(POPOVER_OPEN_KEY) === "1",
  );

  let intervalId: number | null = null;

  const refresh = async () => {
    try {
      setState(await appPomodoroGet());
    } catch (err) {
      console.error("app_pomodoro_get failed", err);
    }
  };

  // setInterval pauses while the OS sleeps or the tab is hidden, but the
  // server-side `started_at` keeps moving. Re-fetch on visibility/focus so
  // the countdown snaps back to reality instead of resuming from its
  // frozen value.
  const onVis = () => {
    if (document.visibilityState === "visible") void refresh();
  };
  const onFocus = () => void refresh();

  // Outside-click closes the popover. Capture phase so clicks on other
  // popovers (e.g. workspace ⋯ menu) close ours first.
  const onDocClick = (e: MouseEvent) => {
    if (!popoverOpen()) return;
    const target = e.target as HTMLElement | null;
    if (!target?.closest(".pomodoro-hud")) {
      setPopoverOpen(false);
      localStorage.setItem(POPOVER_OPEN_KEY, "0");
    }
  };

  onMount(() => {
    void refresh();
    intervalId = window.setInterval(() => setNow(Date.now()), 1000);
    document.addEventListener("visibilitychange", onVis);
    window.addEventListener("focus", onFocus);
    document.addEventListener("click", onDocClick, true);
  });
  onCleanup(() => {
    if (intervalId != null) window.clearInterval(intervalId);
    document.removeEventListener("visibilitychange", onVis);
    window.removeEventListener("focus", onFocus);
    document.removeEventListener("click", onDocClick, true);
  });

  const remaining = () => {
    const s = state();
    if (!s) return 0;
    return appPomodoroRemainingSeconds(s, now());
  };

  const modeClass = () => {
    const s = state();
    if (!s) return "pomodoro-hud-dot pomodoro-hud-dot--idle";
    return `pomodoro-hud-dot pomodoro-hud-dot--${s.mode}`;
  };

  const isRunning = () => {
    const m = state()?.mode;
    return m === "work" || m === "break";
  };

  const togglePopover = (e: MouseEvent) => {
    e.stopPropagation();
    const next = !popoverOpen();
    setPopoverOpen(next);
    localStorage.setItem(POPOVER_OPEN_KEY, next ? "1" : "0");
  };

  // Play/pause acts like the panel's primary button: idle → start work,
  // paused → resume, running → pause.
  const onPlayPause = async (e: MouseEvent) => {
    e.stopPropagation();
    const s = state();
    if (!s) return;
    try {
      let next: AppPomodoroState;
      if (s.mode === "idle") {
        next = await appPomodoroStart("work", null);
      } else if (s.mode === "paused") {
        next = await appPomodoroResume();
      } else {
        next = await appPomodoroPause();
      }
      setState(next);
    } catch (err) {
      console.error("pomodoro action failed", err);
      void refresh();
    }
  };

  const onStartWork = async () => {
    try {
      setState(await appPomodoroStart("work", null));
    } catch (err) {
      console.error("app_pomodoro_start work failed", err);
    }
  };
  const onStartBreak = async () => {
    try {
      setState(await appPomodoroStart("break", null));
    } catch (err) {
      console.error("app_pomodoro_start break failed", err);
    }
  };
  const onReset = async () => {
    try {
      setState(await appPomodoroReset());
    } catch (err) {
      console.error("app_pomodoro_reset failed", err);
    }
  };

  const playPauseTitle = () => {
    const s = state();
    if (!s || s.mode === "idle") return "Start work";
    if (s.mode === "paused") return "Resume";
    return "Pause";
  };

  return (
    <div class="pomodoro-hud" role="group" aria-label="Pomodoro timer">
      <button
        type="button"
        class="pomodoro-hud-bar"
        classList={{ "pomodoro-hud-bar--running": isRunning() }}
        onClick={togglePopover}
        aria-expanded={popoverOpen() ? "true" : "false"}
        aria-haspopup="dialog"
        title="Pomodoro options"
      >
        <span class={modeClass()} aria-hidden="true" />
        <span class="pomodoro-hud-countdown">{formatMmSs(remaining())}</span>
        <span
          class="pomodoro-hud-playpause"
          role="button"
          tabIndex={0}
          aria-label={playPauseTitle()}
          title={playPauseTitle()}
          onClick={onPlayPause}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              void onPlayPause(e as unknown as MouseEvent);
            }
          }}
        >
          <Show when={isRunning()} fallback={<PlayIcon />}>
            <PauseIcon />
          </Show>
        </span>
      </button>
      <Show when={popoverOpen()}>
        <div
          class="pomodoro-hud-popover"
          role="dialog"
          aria-label="Pomodoro controls"
        >
          <div class="pomodoro-hud-popover-cycles">
            {state()?.cycles_completed ?? 0} cycle
            {(state()?.cycles_completed ?? 0) === 1 ? "" : "s"} today
          </div>
          <div class="pomodoro-hud-popover-actions">
            <button
              type="button"
              class="pomodoro-hud-action"
              onClick={onStartWork}
            >
              Work
            </button>
            <button
              type="button"
              class="pomodoro-hud-action"
              onClick={onStartBreak}
            >
              Break
            </button>
            <button
              type="button"
              class="pomodoro-hud-action"
              onClick={onReset}
            >
              Reset
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
};

const PlayIcon: Component = () => (
  <svg viewBox="0 0 12 12" width="10" height="10" aria-hidden="true">
    <path d="M3 2.5v7l6-3.5z" fill="currentColor" />
  </svg>
);

const PauseIcon: Component = () => (
  <svg viewBox="0 0 12 12" width="10" height="10" aria-hidden="true">
    <rect x="3" y="2.5" width="2.2" height="7" fill="currentColor" />
    <rect x="6.8" y="2.5" width="2.2" height="7" fill="currentColor" />
  </svg>
);

export default PomodoroHud;
