-- Whether the model sees pictures: a job's are sent with it then (docs/ai.md#附件).
ALTER TABLE ai_settings ADD COLUMN vision INTEGER NOT NULL DEFAULT 0 CHECK (vision IN (0, 1));
