#![feature(once_cell_get_mut)]
#![allow(static_mut_refs)]
use common::anyhow::Result;
use common::tokio::sync::OnceCell;
use sqlx::{PgPool, Postgres, Transaction};

mod channel;
pub use channel::{get_receiver as get_tts_receiver, send_message as send_tts_message, Channel};
mod guild;
pub use guild::Guild;
mod user;
pub use user::User;
pub use user::VoicePreference;
mod reminder;
pub use reminder::Reminder;
// This crate is for LTS (Long Term Storage) of data for the Neon Circle Discord bot.
// Uses PostgreSQL as the database.
//
// user stores
//  the id for querying
//  a boolean to consent (or not) to process their microphone data.
//  a voice preference, which is either male or female and tells the bot which gender to use for TTS.
//  a timezone, which is a string that can be parsed by chrono to get the timezone.
//
// guild will store
//  the id for querying
//  the default song volume
//  the default radio volume
//  whether to read titles by default
//  the radio audio url
//  the radio data url
//  the empty channel timeout (a duration between 0 and 600 seconds)
//
// channel will be a map from a voice channel id to a text channel id, and usually be queried in reverse, getting a list of voice channels from a text channel id.

static POOL: OnceCell<PgPool> = OnceCell::const_new();

async fn get_connection() -> Result<Transaction<'static, Postgres>> {
    Ok(POOL
        .get_or_init(|| async {
            PgPool::connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
                .await
                .expect("Failed to connect to database")
        })
        .await
        .begin()
        .await?)
}

pub async fn init() {
    user::init().await;
}

pub async fn migrate_data_from_json() -> Result<()> {
    let mut conn = get_connection().await?;
    // user::migrate_data_from_json(&mut conn).await?;
    guild::migrate_data_from_json(&mut conn).await?;
    channel::migrate_data_from_json(&mut conn).await?;
    conn.commit().await?;
    Ok(())
}
