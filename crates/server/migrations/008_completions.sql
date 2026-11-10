-- How many times a repeating task has been done and the last day it was (docs/design.md#重复): set
-- together, from its first time on.
ALTER TABLE tasks ADD COLUMN completion_count INTEGER CHECK (completion_count IS NULL OR completion_count >= 1);
ALTER TABLE tasks ADD COLUMN last_completed TEXT CHECK ((last_completed IS NULL) = (completion_count IS NULL));
