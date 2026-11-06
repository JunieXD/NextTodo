-- A random name for this database's history. A restored backup gets a new one: sequence numbers
-- that devices kept from before mean nothing in it, even where they happen to match.
CREATE TABLE epoch (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    value TEXT NOT NULL
) STRICT;
INSERT INTO epoch (id, value) VALUES (1, lower(hex(randomblob(16))));
