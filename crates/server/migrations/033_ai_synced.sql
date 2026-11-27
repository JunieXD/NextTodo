-- AI settings synced through a drive keep their key as given: the app has no key to seal it with
-- (docs/ai.md#保存位置和安全).
ALTER TABLE ai_settings ADD COLUMN sealed INTEGER NOT NULL DEFAULT 1 CHECK (sealed IN (0, 1));
