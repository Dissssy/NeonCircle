mod raw_azuracast;
mod raw_consent_data;
mod raw_guild_config;
mod raw_voice_data;
pub async fn init() {
    raw_consent_data::init();
    raw_guild_config::init();
    raw_azuracast::init().await;
}
pub async fn save() {
    raw_consent_data::save();
    raw_guild_config::save();
    raw_azuracast::save().await;
}
pub mod consent_data {
    pub use super::raw_consent_data::{get_consent, set_consent};
}
pub mod voice_data {
    pub use super::raw_voice_data::{
        add_satellite, add_satellite_wait, channel_count_besides, initialize_planet, insert_guild,
        lazy_refresh_guild, mutual_channel, refresh_guild, update_voice, VoiceAction,
    };
}
pub mod guild_config {
    pub use super::raw_guild_config::GuildConfig;
}
pub mod azuracast {
    pub use super::raw_azuracast::resubscribe;
}
