-- The days a weekly repeat falls on (docs/design.md#重复), as bits, Monday the lowest; only on a weekly
-- repeat, which without them falls on the due date's day.
ALTER TABLE tasks ADD COLUMN repeat_days INTEGER CHECK (repeat_days IS NULL OR (repeat_unit IS 'week' AND repeat_days BETWEEN 1 AND 127));
