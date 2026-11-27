-- Attachments (docs/design/attachments.md). A file is uploaded first, kept under the data directory's
-- `files/` by its id, and then attached to a task, or to an AI job until the job is organized. A file
-- no attachment uses, not attached yet or its attachment deleted, goes 90 days after. What a file is,
-- its type and size, is here alone: an attachment is only where it is and what it is called.
CREATE TABLE files (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    type TEXT NOT NULL,
    size INTEGER NOT NULL CHECK (size BETWEEN 1 AND 20971520),
    created_at INTEGER NOT NULL,
    unused_at INTEGER,
    -- Syncing through a drive (docs/local.md#附件): cleared, kept as a record so that its id is never
    -- used again; and uploaded on this device, not attached in a commit yet, this device's alone.
    purged INTEGER NOT NULL DEFAULT 0 CHECK (purged IN (0, 1)),
    staged INTEGER NOT NULL DEFAULT 0 CHECK (staged IN (0, 1)),
    -- Only a file no one uses is cleared, and then it is everyone's.
    CHECK (NOT purged OR (unused_at IS NOT NULL AND NOT staged))
) STRICT;
CREATE INDEX files_unused ON files (unused_at) WHERE unused_at IS NOT NULL;

CREATE TABLE attachments (
    id TEXT PRIMARY KEY REFERENCES files (id),
    task_id TEXT REFERENCES tasks (id) ON DELETE CASCADE,
    job_id TEXT REFERENCES ai_jobs (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
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
