-- Add migration script here

-- Reminders table is a table where reminders are stored for a user
CREATE TABLE IF NOT EXISTS reminders (
    -- uuid for the reminder
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- discord user id
    user_id BIGINT NOT NULL,
    -- discord channel id (optional, as a user can create a reminder in the bot's DMs)
    channel_id BIGINT,
    -- discord guild id (optional, as a user can create a reminder in the bot's DMs)
    guild_id BIGINT,
    -- reminder message
    message TEXT NOT NULL,
    -- reminder time
    remind_at TIMESTAMP NOT NULL,
    -- created at
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- updated at
    updated_at TIMESTAMP,
    -- send attempt count
    send_attempt_count INT NOT NULL DEFAULT 0
);

-- Enforce that the created_at timestamp is only ever set when the row is created, and the updated_at timestamp is always updated when the row is updated
CREATE OR REPLACE FUNCTION update_timestamp()
RETURNS TRIGGER AS $$
BEGIN
    IF OLD IS NULL THEN
        NEW.created_at = CURRENT_TIMESTAMP;
    ELSE
        NEW.created_at = OLD.created_at;
        NEW.updated_at = CURRENT_TIMESTAMP;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER update_timestamp
    BEFORE INSERT OR UPDATE ON reminders
    FOR EACH ROW
    EXECUTE FUNCTION update_timestamp();


-- Add a Timezone column to the users table
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS timezone TEXT NOT NULL DEFAULT 'EST';