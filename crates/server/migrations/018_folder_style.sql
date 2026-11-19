-- A folder's colour and icon, from the presets (docs/ui.md#文件夹的颜色和图标); NULL is the default.
ALTER TABLE folders ADD COLUMN color TEXT;
ALTER TABLE folders ADD COLUMN icon TEXT;
