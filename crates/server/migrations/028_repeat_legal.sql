-- 每个法定工作日 (docs/design.md#重复): every week, Monday to Friday, by the days off and worked the State
-- Council sets around public holidays.
ALTER TABLE tasks ADD COLUMN repeat_legal INTEGER NOT NULL DEFAULT 0 CHECK (repeat_legal IN (0, 1) AND (repeat_legal = 0 OR (repeat_every IS 1 AND repeat_unit IS 'week' AND repeat_days IS 31)));
