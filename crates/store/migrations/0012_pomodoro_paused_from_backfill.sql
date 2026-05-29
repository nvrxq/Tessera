-- Backfill paused_from for timers that were already paused before migration
-- 0010 added the column (those rows got NULL). With paused_from NULL, resume
-- defaults to Work and a paused Break could be mis-credited as a work cycle.
--
-- 0010 already shipped (v0.1.11), so editing it would not re-run for users who
-- applied it — hence this separate backfill. We conservatively assume a
-- pre-existing paused timer was a Work session (matching the old resume
-- behaviour), which only ever under-credits, never inflates, the cycle count.
UPDATE app_pomodoro SET paused_from = 'work'
  WHERE mode = 'paused' AND paused_from IS NULL;
UPDATE workspace_pomodoro SET paused_from = 'work'
  WHERE mode = 'paused' AND paused_from IS NULL;
