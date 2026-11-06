-- Jobs that tidy a folder's open tasks: done, they stay, ready, with AI's suggestions as JSON until
-- the user takes them or drops the job. SQLite changes no CHECK in place: the table is made anew.
CREATE TABLE ai_jobs_new (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    folder_id TEXT REFERENCES folders (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('organize', 'tidy')),
    text TEXT NOT NULL,
    local_time TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('waiting', 'running', 'failed', 'ready')),
    error TEXT CHECK ((error IS NOT NULL) = (state = 'failed')),
    suggestions TEXT CHECK ((suggestions IS NOT NULL) = (state = 'ready')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
INSERT INTO ai_jobs_new (id, owner_id, folder_id, kind, text, local_time, state, error, created_at, updated_at)
    SELECT id, owner_id, folder_id, 'organize', text, local_time, state, error, created_at, updated_at FROM ai_jobs;
DROP TABLE ai_jobs;
ALTER TABLE ai_jobs_new RENAME TO ai_jobs;
CREATE INDEX ai_jobs_owner ON ai_jobs (owner_id, state);
CREATE INDEX ai_jobs_folder ON ai_jobs (folder_id);
