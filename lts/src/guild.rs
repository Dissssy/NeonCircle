// CREATE TABLE IF NOT EXISTS guilds (
//     -- Discord guild ID for querying
//     id BIGINT PRIMARY KEY,
//     -- Default song playback volume (between 0.0 and 1.0, enforced by an ON INSERT OR UPDATE trigger)
//     default_volume REAL NOT NULL DEFAULT 0.75,
//     -- Default radio playback volume (between 0.0 and 1.0, enforced by an ON INSERT OR UPDATE trigger)
//     radio_volume REAL NOT NULL DEFAULT 0.25,
//     -- Default "read titles" setting (whether or not the bot should read song titles)
//     read_titles BOOLEAN NOT NULL DEFAULT TRUE,
//     -- Custom Radio URL for the guild
//     radio_url TEXT,
//     -- Custom Radio Data URL for the guild
//     radio_data_url TEXT,
//     -- Empty channel timeout in milliseconds (between 0 and 600000, enforced by an ON INSERT OR UPDATE trigger), defaults to 30 seconds: 30000
//     empty_channel_timeout INTEGER NOT NULL DEFAULT 30000
// );

use std::sync::Arc;

use common::{
    anyhow::{anyhow, Result},
    serenity::all::GuildId,
    tokio::time::Duration,
};

pub struct Guild {
    pub id: GuildId,
    pub default_song_volume: f32,
    pub default_radio_volume: f32,
    pub read_titles: bool,
    pub radio_audio_url: Option<Arc<str>>,
    pub radio_data_url: Option<Arc<str>>,
    pub empty_channel_timeout: Duration,
}

impl Guild {
    pub async fn load_opt(id: GuildId) -> Result<Option<Self>> {
        let mut conn = crate::get_connection().await?;
        get::full(id, &mut conn).await
    }
    pub async fn load(id: GuildId) -> Result<Self> {
        let mut conn = crate::get_connection().await?;
        match get::full(id, &mut conn).await? {
            Some(guild) => Ok(guild),
            None => {
                set::default(id, &mut conn).await?;
                match get::full(id, &mut conn).await? {
                    Some(guild) => {
                        conn.commit().await?;
                        Ok(guild)
                    }
                    None => Err(anyhow!("Failed to write default guild")),
                }
            }
        }
    }
    pub async fn save(self) -> Result<()> {
        let mut conn = crate::get_connection().await?;
        set::full(self, &mut conn).await?;
        conn.commit().await?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct RawGuild {
    id: i64,
    default_volume: f32,
    radio_volume: f32,
    read_titles: bool,
    radio_url: Option<String>,
    radio_data_url: Option<String>,
    empty_channel_timeout: i32,
}

impl From<RawGuild> for Guild {
    fn from(raw: RawGuild) -> Self {
        Self {
            id: GuildId::new(raw.id as u64),
            default_song_volume: raw.default_volume,
            default_radio_volume: raw.radio_volume,
            read_titles: raw.read_titles,
            radio_audio_url: raw.radio_url.map(Into::into),
            radio_data_url: raw.radio_data_url.map(Into::into),
            empty_channel_timeout: Duration::from_millis(raw.empty_channel_timeout as u64),
        }
    }
}

mod get {
    use super::{Guild, GuildId, RawGuild, Result};

    pub async fn full(
        id: GuildId,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Option<Guild>> {
        let guild = sqlx::query_as!(
            RawGuild,
            "SELECT * FROM guilds WHERE id = $1",
            id.get() as i64
        )
        .fetch_optional(&mut **conn)
        .await?;
        Ok(guild.map(Into::into))
    }
}

mod set {
    use super::{Guild, GuildId, Result};

    pub async fn full(
        guild: Guild,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<()> {
        let Guild {
            id,
            default_song_volume,
            default_radio_volume,
            read_titles,
            radio_audio_url,
            radio_data_url,
            empty_channel_timeout,
        } = guild;
        sqlx::query!(
            "INSERT INTO guilds (id, default_volume, radio_volume, read_titles, radio_url, radio_data_url, empty_channel_timeout) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (id) DO UPDATE SET default_volume = $2, radio_volume = $3, read_titles = $4, radio_url = $5, radio_data_url = $6, empty_channel_timeout = $7",
            id.get() as i64,
            default_song_volume,
            default_radio_volume,
            read_titles,
            radio_audio_url.map(|s| s.to_string()),
            radio_data_url.map(|s| s.to_string()),
            empty_channel_timeout.as_millis() as i32
        )
        .execute(&mut **conn)
        .await?;
        Ok(())
    }

    pub async fn default(
        id: GuildId,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<()> {
        sqlx::query!(
            "INSERT INTO guilds (id) VALUES ($1) ON CONFLICT (id) DO NOTHING",
            id.get() as i64
        )
        .execute(&mut **conn)
        .await?;
        Ok(())
    }
}

pub async fn migrate_data_from_json(
    conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<()> {
    let guilds = common::global_data::extract::guild_config().await;

    for (id, config) in guilds {
        sqlx::query!(
            "INSERT INTO guilds (id, default_volume, radio_volume, read_titles, radio_url, radio_data_url, empty_channel_timeout) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (id) DO UPDATE SET default_volume = $2, radio_volume = $3, read_titles = $4, radio_url = $5, radio_data_url = $6, empty_channel_timeout = $7",
            id.get() as i64,
            config.get_default_song_volume(),
            config.get_default_radio_volume(),
            config.get_read_titles_by_default(),
            config.get_radio_audio_url().map(|s| s.to_string()),
            config.get_radio_data_url().map(|s| s.to_string()),
            config.get_empty_channel_timeout().as_millis() as i32
        )
        .execute(&mut **conn)
        .await?;
    }
    Ok(())
}
