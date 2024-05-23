use super::OrAuto;
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
}
impl SettingsData {
    pub fn song_volume(&self) -> f32 {
        // self.something_playing = true;
        self.song_volume * 0.5
    }
    pub fn display_song_volume(&self) -> f32 {
        self.song_volume
    }
    pub fn set_song_volume(&mut self, v: f32) {
        self.song_volume = v;
    }
    // pub fn raw_song_volume(&self) -> f32 {
    //     self.song_volume
    // }
    pub fn radio_volume(&self) -> f32 {
        // self.something_playing = false;
        self.radio_volume * 0.5
    }
    pub fn set_radio_volume(&mut self, v: f32) {
        self.radio_volume = v;
    }
    pub fn display_radio_volume(&self) -> f32 {
        self.radio_volume
    }
    // pub fn raw_radio_volume(&self) -> f32 {
    //     self.radio_volume
    // }
}
impl Default for SettingsData {
    fn default() -> Self {
        Self {
            // something_playing: false,
            song_volume: 1.0,
            radio_volume: 0.33,
            autoplay: false,
            looped: false,
            repeat: false,
            shuffle: false,
            pause: false,
            bitrate: OrAuto::Auto,
            log_empty: true,
            read_titles: cfg!(feature = "read-titles-by-default"),
        }
    }
}
