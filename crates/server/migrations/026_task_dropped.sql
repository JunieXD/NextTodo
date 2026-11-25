-- A task given up rather than done (docs/design.md#放弃): ended, `done_at` being when.
ALTER TABLE tasks ADD COLUMN dropped INTEGER NOT NULL DEFAULT 0 CHECK (dropped IN (0, 1) AND (dropped = 0 OR done_at IS NOT NULL));
