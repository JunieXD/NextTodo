-- ECNU is no longer a preset: settings made with it go on as a custom provider, with the same
-- address and model. It needs no thinking parameter, and answers in JSON without JSON mode.
UPDATE ai_settings SET provider = 'custom' WHERE provider = 'ecnu';
