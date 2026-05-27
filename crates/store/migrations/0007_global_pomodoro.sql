-- Global (app-wide) pomodoro timer.
--
-- One single row keyed on a constant id so the timer is shared across every
-- workspace. The per-workspace `workspace_pomodoro` table is left in place
-- to keep the migration additive — it just becomes unused once the
-- frontend stops calling the workspace_pomodoro_* commands.
CREATE TABLE app_pomodoro (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    mode TEXT NOT NULL DEFAULT 'idle',                  -- 'idle' | 'work' | 'break' | 'paused'
    started_at TEXT,
    paused_at TEXT,
    target_seconds INTEGER NOT NULL DEFAULT 1500,       -- 25 min
    elapsed_seconds_before_pause INTEGER NOT NULL DEFAULT 0,
    cycles_completed INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);
-- Seed `updated_at` as RFC3339 (with the trailing 'Z') so it round-trips
-- through `chrono::DateTime::parse_from_rfc3339` — sqlite's bare
-- `datetime('now')` is `YYYY-MM-DD HH:MM:SS`, which is NOT RFC3339 and
-- would blow up the first `get_app_pomodoro` read on a fresh DB.
INSERT OR IGNORE INTO app_pomodoro
    (id, mode, target_seconds, elapsed_seconds_before_pause, cycles_completed, updated_at)
VALUES
    (1, 'idle', 1500, 0, 0, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
