CREATE TABLE ai_jobs (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    folder_id TEXT REFERENCES folders (id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    local_time TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('waiting', 'running', 'failed')),
    error TEXT CHECK ((error IS NOT NULL) = (state = 'failed')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
CREATE INDEX ai_jobs_owner ON ai_jobs (owner_id, state);
CREATE INDEX ai_jobs_folder ON ai_jobs (folder_id);

CREATE TABLE ai_settings (
    user_id TEXT PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    base_url TEXT NOT NULL,
    model TEXT NOT NULL,
    thinking TEXT NOT NULL CHECK (thinking IN ('off', 'low', 'high')),
    api_key BLOB NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
