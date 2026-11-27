-- A monthly repeat by the lunar calendar (docs/design/repeat.md#重复): always on some days of the month, the
-- 1st to the 30th.
ALTER TABLE tasks ADD COLUMN repeat_lunar INTEGER NOT NULL DEFAULT 0 CHECK (repeat_lunar IN (0, 1) AND (repeat_lunar = 0 OR (repeat_unit IS 'month' AND repeat_monthdays IS NOT NULL AND repeat_monthdays BETWEEN 1 AND 1073741823)));
