-- The default reminders of the device that asked, for the tasks AI organizing makes
-- (docs/ai.md#提示词和结果): a JSON object as the API has it.
ALTER TABLE ai_jobs ADD COLUMN reminders TEXT CHECK (reminders IS NULL OR json_valid(reminders));
