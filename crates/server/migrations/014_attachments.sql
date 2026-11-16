-- Attachments (docs/design.md#附件). A file is uploaded first, kept under the data directory's
-- `files/` by its id, and then attached to a task, or to an AI job until the job is organized. A file
-- no attachment uses, not attached yet or its attachment deleted, goes 90 days after.
CREATE TABLE files (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    type TEXT NOT NULL,
    size INTEGER NOT NULL CHECK (size BETWEEN 1 AND 20971520),
    created_at INTEGER NOT NULL,
    unused_at INTEGER
) STRICT;
CREATE INDEX files_unused ON files (unused_at) WHERE unused_at IS NOT NULL;

CREATE TABLE attachments (
    id TEXT PRIMARY KEY REFERENCES files (id),
    task_id TEXT REFERENCES tasks (id) ON DELETE CASCADE,
    job_id TEXT REFERENCES ai_jobs (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    type TEXT NOT NULL,
    size INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    CHECK ((task_id IS NULL) <> (job_id IS NULL))
) STRICT;
CREATE INDEX attachments_task ON attachments (task_id);
CREATE INDEX attachments_job ON attachments (job_id);

-- The server's settings, one row.
CREATE TABLE server_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    compress_images INTEGER NOT NULL CHECK (compress_images IN (0, 1))
) STRICT;
INSERT INTO server_settings (id, compress_images) VALUES (1, 1);
