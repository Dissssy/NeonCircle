use super::OrAuto;

#[derive(Clone, PartialEq, Debug)]
pub struct SettingsData {
    pub something_playing: bool,
    pub log_empty: bool,

    // READ ONLY?
    volume: f64,
    radiovolume: f64,
    pub bitrate: OrAuto,

    // CLICKABLE
    pub autoplay: bool,
    pub looped: bool,
    pub repeat: bool,
    pub shuffle: bool,
    pub pause: bool,
    pub read_titles: bool,
}

impl SettingsData {
    pub fn volume(&mut self) -> f64 {
        self.something_playing = true;
        self.volume * 0.5
    }
    pub fn set_volume(&mut self, v: f64) {
        self.volume = v;
    }
    pub fn raw_volume(&self) -> f64 {
        self.volume
    }
    pub fn radiovolume(&mut self) -> f64 {
        self.something_playing = false;
        self.radiovolume * 0.5
    }
    pub fn set_radiovolume(&mut self, v: f64) {
        self.radiovolume = v;
    }
    pub fn raw_radiovolume(&self) -> f64 {
        self.radiovolume
    }
}

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            something_playing: false,
            volume: 1.0,
            radiovolume: 0.33,
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
