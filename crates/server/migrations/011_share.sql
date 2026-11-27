-- Sharing folders (docs/design/sharing.md): who a folder is shared with, which makes it and everything
-- in it theirs to see and change too; who is to do a task in one.
CREATE TABLE members (
    folder_id TEXT NOT NULL REFERENCES folders (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (folder_id, user_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX members_user ON members (user_id);

CREATE TABLE assignees (
    task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, user_id)
) STRICT, WITHOUT ROWID;

-- Who added a task and who changed it last: until now always its owner.
ALTER TABLE tasks ADD COLUMN created_by TEXT NOT NULL DEFAULT '';
ALTER TABLE tasks ADD COLUMN updated_by TEXT NOT NULL DEFAULT '';
UPDATE tasks SET created_by = owner_id, updated_by = owner_id;

-- Who each change goes to, and what they see of it when that is not all of it: a task moved out of a
-- shared folder is deleted for those left in it (docs/design.md#连接).
CREATE TABLE deliveries (
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    seq INTEGER NOT NULL REFERENCES mutations (seq) ON DELETE CASCADE,
    effects TEXT,
    PRIMARY KEY (user_id, seq)
) STRICT, WITHOUT ROWID;
INSERT INTO deliveries (user_id, seq) SELECT owner_id, seq FROM mutations WHERE effects IS NOT NULL;
DROP INDEX mutations_owner;

-- The tasks in the changes kept, as tasks are now, so that undoing them brings back whole tasks.
UPDATE mutations SET effects = (
    SELECT json_group_array(CASE WHEN e.value ->> 'kind' = 'task' THEN json_object(
        'kind', 'task',
        'before', CASE WHEN e.value ->> 'before' IS NULL THEN json('null') ELSE json_set(e.value -> 'before',
            '$.assignees', json('[]'), '$.created_by', mutations.owner_id, '$.updated_by', mutations.owner_id) END,
        'after', CASE WHEN e.value ->> 'after' IS NULL THEN json('null') ELSE json_set(e.value -> 'after',
            '$.assignees', json('[]'), '$.created_by', mutations.owner_id, '$.updated_by', mutations.owner_id) END)
    ELSE json(e.value) END)
    FROM json_each(mutations.effects) e
) WHERE effects IS NOT NULL;
