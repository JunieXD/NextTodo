-- Repeating tasks (docs/design.md#重复): the three columns set together, only with a due date, and
-- never on a task done, as a repeating one is never done.
ALTER TABLE tasks ADD COLUMN repeat_every INTEGER CHECK (repeat_every IS NULL OR (repeat_every BETWEEN 1 AND 365 AND due_date IS NOT NULL AND done_at IS NULL));
ALTER TABLE tasks ADD COLUMN repeat_unit TEXT CHECK ((repeat_unit IS NULL) = (repeat_every IS NULL) AND (repeat_unit IS NULL OR repeat_unit IN ('day', 'week', 'month')));
ALTER TABLE tasks ADD COLUMN repeat_from TEXT CHECK ((repeat_from IS NULL) = (repeat_every IS NULL) AND (repeat_from IS NULL OR repeat_from IN ('due', 'done')));
