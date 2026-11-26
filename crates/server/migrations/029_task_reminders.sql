-- Reminders by the start and at times of their own (docs/design.md#提醒): JSON lists as the API has
-- them, none when empty; those by the start only with a start date.
ALTER TABLE tasks ADD COLUMN start_reminders TEXT CHECK (start_reminders IS NULL OR (start_date IS NOT NULL AND json_valid(start_reminders)));
ALTER TABLE tasks ADD COLUMN remind_at TEXT CHECK (remind_at IS NULL OR json_valid(remind_at));
