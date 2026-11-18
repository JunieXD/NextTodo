-- Syncing devices in local mode through a cloud drive (docs/local.md#借网盘同步).

-- The round of organizing a job is in while it runs: the id of the change that started it. Results
-- of an earlier round, coming late, are told apart by it.
ALTER TABLE ai_jobs ADD COLUMN run TEXT;

-- This device's place in the drive's history: the generation it syncs with and the commits read.
CREATE TABLE sync_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    generation TEXT NOT NULL,
    applied INTEGER NOT NULL
) STRICT;

-- The rounds of organizing this device started, written down before the start is committed, so
-- that one cut short is failed after a restart even if the start never came back here.
CREATE TABLE sync_runs (
    run TEXT PRIMARY KEY,
    job_id TEXT NOT NULL
) STRICT;
