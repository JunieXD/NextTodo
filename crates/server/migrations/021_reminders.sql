-- Several reminders, also by the day (docs/design/dates.md#提醒): a JSON list as the API has it, instead of
-- one so many minutes before.
ALTER TABLE tasks ADD COLUMN reminders TEXT CHECK (reminders IS NULL OR json_valid(reminders));
UPDATE tasks SET reminders = json_array(json_object('before', remind_before)) WHERE remind_before IS NOT NULL;
ALTER TABLE tasks DROP COLUMN remind_before;
