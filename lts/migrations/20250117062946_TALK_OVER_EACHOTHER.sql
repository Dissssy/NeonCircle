-- Add migration script here
-- add a talk_over_eachother column to the guilds table, defaults to false
ALTER TABLE guilds
    ADD COLUMN IF NOT EXISTS talk_over_eachother BOOLEAN NOT NULL DEFAULT FALSE;