-- The settings that go with the account (docs/ui/settings.md#跟着账号的设置), as JSON.
ALTER TABLE users ADD COLUMN prefs TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(prefs));
