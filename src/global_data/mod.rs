mod consent;
mod voice_data;
pub use consent::{get_consent, set_consent};
pub use voice_data::{
    add_satellite, add_satellite_wait, initialize_planet, insert_guild, lazy_refresh_guild,
    mutual_channel, refresh_guild, update_voice, VoiceAction,
};
pub fn init() {
    consent::init();
}
pub fn save() {
    consent::save();
}
