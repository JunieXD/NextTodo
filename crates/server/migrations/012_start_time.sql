-- The time on its start day a task starts (docs/design.md#开始时间): only with a start date, and on
-- its due day not after the due time.
ALTER TABLE tasks ADD COLUMN start_time TEXT CHECK (
    start_time IS NULL
    OR (start_date IS NOT NULL AND (due_date IS NULL OR due_time IS NULL OR start_date < due_date OR start_time <= due_time))
);
