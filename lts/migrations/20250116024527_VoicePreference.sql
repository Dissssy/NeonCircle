-- Add migration script here
-- add column voice_preference to user table, nullable boolean, defaults null
ALTER TABLE users ADD COLUMN voice_preference BOOLEAN DEFAULT NULL;