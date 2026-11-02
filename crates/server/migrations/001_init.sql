CREATE TABLE users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    is_admin INTEGER NOT NULL DEFAULT 0 CHECK (is_admin IN (0, 1)),
    next_task_number INTEGER NOT NULL DEFAULT 1,
    pruned_seq INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE invites (
    code TEXT PRIMARY KEY,
    created_by TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    used_by TEXT REFERENCES users (id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    used_at INTEGER
) STRICT;

CREATE TABLE tokens (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('session', 'api')),
    name TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('read', 'write')),
    secret_hash BLOB NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER
) STRICT;
CREATE INDEX tokens_user ON tokens (user_id);

CREATE TABLE folders (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES folders (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_key TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
CREATE INDEX folders_parent ON folders (owner_id, parent_id, sort_key);
CREATE UNIQUE INDEX folders_name ON folders (owner_id, coalesce(parent_id, ''), name);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    owner_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    number INTEGER NOT NULL,
    folder_id TEXT REFERENCES folders (id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    done_at INTEGER,
    due_date TEXT,
    due_time TEXT CHECK (due_time IS NULL OR due_date IS NOT NULL),
    remind_before INTEGER CHECK (remind_before IS NULL OR due_time IS NOT NULL),
    sort_key TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    created_via TEXT,
    updated_via TEXT,
    UNIQUE (owner_id, number)
) STRICT;
CREATE INDEX tasks_folder ON tasks (owner_id, folder_id, sort_key);

CREATE TABLE dependencies (
    before_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    after_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    owner_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (before_id, after_id),
    CHECK (before_id <> after_id)
) STRICT;
CREATE INDEX dependencies_after ON dependencies (after_id);
CREATE INDEX dependencies_owner ON dependencies (owner_id);

CREATE TABLE mutations (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    owner_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_id TEXT NOT NULL,
    actor TEXT,
    -- both cleared once past the retention period; the row stays so the id is still recognised
    ops TEXT,
    effects TEXT,
    created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX mutations_owner ON mutations (owner_id, seq);
