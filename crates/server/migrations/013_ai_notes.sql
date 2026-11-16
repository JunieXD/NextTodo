-- The user's notes for AI organizing, by folder, the root's NULL (docs/ai.md#说明). No foreign key to
-- the folder: one deleted keeps its note, which comes back with it when that is undone.
CREATE TABLE ai_notes (
    user_id TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    folder_id TEXT,
    text TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 2000),
    updated_at INTEGER NOT NULL
) STRICT;
CREATE UNIQUE INDEX ai_notes_folder ON ai_notes (user_id, ifnull(folder_id, ''));
