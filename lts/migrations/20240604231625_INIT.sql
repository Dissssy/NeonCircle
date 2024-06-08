-- INITIAL MIGRATION, CREATION OF ALL TABLES
-- DB: POSTGRESQL

CREATE TABLE IF NOT EXISTS users (
    -- Discord user ID for querying
    id BIGINT PRIMARY KEY,
    -- Whether or not the user consents to their microphone data being processed
    mic_consent BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE IF NOT EXISTS guilds (
    -- Discord guild ID for querying
    id BIGINT PRIMARY KEY,
    -- Default song playback volume (between 0.0 and 1.0, enforced by an ON INSERT OR UPDATE trigger)
    default_volume REAL NOT NULL DEFAULT 0.75,
    -- Default radio playback volume (between 0.0 and 1.0, enforced by an ON INSERT OR UPDATE trigger)
    radio_volume REAL NOT NULL DEFAULT 0.25,
    -- Default "read titles" setting (whether or not the bot should read song titles)
    read_titles BOOLEAN NOT NULL DEFAULT TRUE,
    -- Custom Radio URL for the guild
    radio_url TEXT,
    -- Custom Radio Data URL for the guild
    radio_data_url TEXT,
    -- Empty channel timeout in milliseconds (between 0 and 600000, enforced by an ON INSERT OR UPDATE trigger), defaults to 30 seconds: 30000
    empty_channel_timeout INTEGER NOT NULL DEFAULT 30000
);

CREATE OR REPLACE FUNCTION validate_settings() RETURNS TRIGGER AS $$
BEGIN
    NEW.default_volume = GREATEST(LEAST(NEW.default_volume, 1.0), 0.0);
    NEW.radio_volume = GREATEST(LEAST(NEW.radio_volume, 1.0), 0.0);
    NEW.empty_channel_timeout = GREATEST(LEAST(NEW.empty_channel_timeout, 600000), 0);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER validate_settings_trigger
    BEFORE INSERT OR UPDATE ON guilds
    FOR EACH ROW
    EXECUTE FUNCTION validate_settings();

-- channel table will map from a voice channel discord id to a list of text channel discord ids, defaulting to a list containing the voice channel id
CREATE TABLE IF NOT EXISTS channels (
    -- The voice channel ID
    voice_id BIGINT PRIMARY KEY,
    -- The text channel IDs
    text_ids BIGINT[] NOT NULL DEFAULT '{}'
);
-- now we need to default the text_ids to an array containing the voice_id
CREATE OR REPLACE FUNCTION default_text_ids() RETURNS TRIGGER AS $$
BEGIN
    IF OLD IS NULL THEN
        -- append the voice_id to the text_ids array since we might be writing a value into it too
        NEW.text_ids = NEW.text_ids || ARRAY[NEW.voice_id];
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER default_text_ids_trigger
    BEFORE INSERT ON channels
    FOR EACH ROW
    EXECUTE FUNCTION default_text_ids();

