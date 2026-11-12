-- The day a task can start, waiting until then (docs/design.md#开始日期): not after its due date.
ALTER TABLE tasks ADD COLUMN start_date TEXT CHECK (start_date IS NULL OR due_date IS NULL OR start_date <= due_date);
