ALTER TABLE ai_jobs ADD COLUMN day TEXT CHECK (day IS NULL OR kind = 'organize');
ALTER TABLE ai_jobs ADD COLUMN time TEXT CHECK (time IS NULL OR day IS NOT NULL);
