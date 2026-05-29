-- Record the Work/Break mode that was active before a pause.
--
-- Without this, pause collapses Work and Break into a single 'paused' state
-- with no memory of the prior mode: resume always came back as Work, and a
-- paused Break ≥ half its target was mis-credited as a completed work cycle.
-- A nullable text column ('work' | 'break') holds the pre-pause mode; it is
-- NULL whenever the timer is not paused.
ALTER TABLE app_pomodoro ADD COLUMN paused_from TEXT;
ALTER TABLE workspace_pomodoro ADD COLUMN paused_from TEXT;
