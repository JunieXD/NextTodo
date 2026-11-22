-- Each time a repeating task is done is kept, the same time done again too (docs/design.md#重复):
-- `again` tells them apart. A time had one record so far, the first.
CREATE TABLE completions_again (
    task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    due_date TEXT NOT NULL,
    again INTEGER NOT NULL CHECK (again >= 0),
    done_on TEXT NOT NULL,
    PRIMARY KEY (task_id, due_date, again)
) STRICT;
INSERT INTO completions_again (task_id, due_date, again, done_on) SELECT task_id, due_date, 0, done_on FROM completions;
DROP TABLE completions;
ALTER TABLE completions_again RENAME TO completions;
