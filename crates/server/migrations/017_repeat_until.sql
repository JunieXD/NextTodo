-- The last day a repeat falls on, when it ends (docs/design.md#重复): only on a repeat, and not before
-- the due date.
ALTER TABLE tasks ADD COLUMN repeat_until TEXT CHECK (repeat_until IS NULL OR (repeat_every IS NOT NULL AND repeat_until >= due_date));
