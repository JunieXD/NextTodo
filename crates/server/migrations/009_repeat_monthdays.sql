-- The days of the month a monthly repeat falls on (docs/design.md#重复), as bits, the 1st the lowest;
-- only on a monthly repeat, which without them falls on the due date's day.
ALTER TABLE tasks ADD COLUMN repeat_monthdays INTEGER CHECK (repeat_monthdays IS NULL OR (repeat_unit IS 'month' AND repeat_monthdays BETWEEN 1 AND 2147483647));
