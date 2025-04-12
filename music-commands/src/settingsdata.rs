use common::{anyhow::Result, audio::OrAuto, serenity::all::GuildId};
#[derive(Clone, PartialEq, Debug)]
pub struct SettingsData {
    // pub something_playing: bool,
    pub log_empty: bool,

    song_volume: f32,
    radio_volume: f32,
    pub bitrate: OrAuto,

    pub autoplay: bool,
    pub looped: bool,
    pub repeat: bool,
    pub shuffle: bool,
    pub pause: bool,
    pub read_titles: bool,
    pub talk_over_eachother: bool,
}
impl SettingsData {
    pub async fn new(guild: GuildId) -> Result<Self> {
        // let cfg = common::global_data::guild_config::GuildConfig::get(guild).await;
        let cfg = long_term_storage::Guild::load(guild).await?;
        Ok(Self {
            // something_playing: false,
            song_volume: cfg.default_song_volume,
            radio_volume: cfg.default_radio_volume,
            autoplay: false,
            looped: false,
            repeat: false,
            shuffle: false,
            pause: false,
            // bitrate: OrAuto::Auto,
            bitrate: OrAuto::Specific(48000),
            log_empty: true,
            read_titles: cfg.read_titles,
            talk_over_eachother: cfg.talk_over_eachother,
        })
    }
    pub fn song_volume(&self) -> f32 {
        // self.something_playing = true;
        self.song_volume * 0.5
    }
    pub fn display_song_volume(&self) -> f32 {
        self.song_volume
    }
    pub fn set_song_volume(&mut self, v: f32, place: &str) {
        log::trace!("SONG VOL SET AT: {place}");
        self.song_volume = v;
    }
    // pub fn raw_song_volume(&self) -> f32 {
    //     self.song_volume
    // }
    pub fn radio_volume(&self) -> f32 {
        // self.something_playing = false;
        self.radio_volume * 0.5
    }
    pub fn set_radio_volume(&mut self, v: f32, place: &str) {
        log::trace!("RADIO VOL SET AT: {place}");
        self.radio_volume = v;
    }
    pub fn display_radio_volume(&self) -> f32 {
        self.radio_volume
    }
    // pub fn raw_radio_volume(&self) -> f32 {
    //     self.radio_volume
    // }
}
// impl Default for SettingsData {
//     fn default() -> Self {
//         Self {
//             // something_playing: false,
//             song_volume: 1.0,
//             radio_volume: 0.33,
//             autoplay: false,
//             looped: false,
//             repeat: false,
//             shuffle: false,
//             pause: false,
//             bitrate: OrAuto::Auto,
//             log_empty: true,
//             read_titles: cfg!(feature = "read-titles-by-default"),
//         }
//     }
// }
