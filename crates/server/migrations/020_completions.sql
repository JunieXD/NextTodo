-- A record of each time a repeating task was done (docs/design.md#重复): the time due on which day, done
-- on which. Done twice, the time due on a day keeps the later.
CREATE TABLE completions (
    task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    due_date TEXT NOT NULL,
    done_on TEXT NOT NULL,
    PRIMARY KEY (task_id, due_date)
) STRICT;
