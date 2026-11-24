-- A due date some days after a task's prerequisites are all done (docs/design.md#前置完成后的截止):
-- `due_after` as the API has it, a JSON object, and the day they were, from which its due date counts.
ALTER TABLE tasks ADD COLUMN due_after TEXT CHECK (due_after IS NULL OR json_valid(due_after));
ALTER TABLE tasks ADD COLUMN unlocked_on TEXT CHECK (unlocked_on IS NULL OR due_after IS NOT NULL);
