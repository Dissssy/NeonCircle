#![allow(static_mut_refs)]
mod raw_azuracast;
mod raw_consent_data;
mod raw_guild_config;
mod raw_transcribe;
mod raw_voice_data;
pub async fn init() {
    raw_consent_data::init();
    raw_guild_config::init().await;
    raw_azuracast::init().await;
}
pub async fn save() {
    raw_consent_data::save();
    raw_guild_config::save().await;
    raw_azuracast::save().await;
    raw_transcribe::save().await;
}

pub mod voice_data {
    pub use super::raw_voice_data::{
        add_satellite, add_satellite_wait, bot_connected, channel_count_besides, initialize_planet,
        insert_guild, lazy_refresh_guild, mutual_channel, refresh_guild, update_voice, VoiceAction,
    };
}

pub mod azuracast {
    pub use super::raw_azuracast::resubscribe;
}

pub mod extract {
    pub use super::raw_consent_data::extract_all as consent_data;
    pub use super::raw_guild_config::extract_all as guild_config;
    pub use super::raw_transcribe::extract_all as transcribe;
}
