use serde::{Deserialize, Serialize};
use serenity::all::GuildId;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
lazy_static::lazy_static! {
    static ref RWLOCK: RwLock<HashMap<GuildId, InnerGuildConfig>> = {
        let file = match std::fs::File::open(crate::config::get_config().guild_config_path) {
            Ok(f) => f,
            Err(_) => {
                let f = match std::fs::File::create(crate::config::get_config().guild_config_path) {
                    Ok(f) => f,
                    Err(e) => panic!("Failed to create guild config file: {}", e),
                };
                if let Err(e) = serde_json::to_writer(f, &HashMap::<GuildId, InnerGuildConfig>::new()) {
                    panic!("Failed to write default guild config file: {}", e);
                }
                match std::fs::File::open(crate::config::get_config().guild_config_path) {
                    Ok(f) => f,
                    Err(e) => panic!("Failed to open guild config file: {}", e),
                }
            }
        };
        let res: HashMap<GuildId, RawInnerGuildConfig> = match serde_json::from_reader(file) {
            Ok(r) => r,
            Err(e) => panic!("Failed to read guild config file: {}", e),
        };
        RwLock::new(res.into_iter().map(|(k, v)| (k, v.with_defaults())).collect())
    };
}
pub async fn init() {
    let _ = RWLOCK.read().await;
}
pub async fn save() {
    let map = RWLOCK.read().await;
    let file = match std::fs::File::create(crate::config::get_config().guild_config_path) {
        Ok(f) => f,
        Err(e) => panic!("Failed to create guild config file: {}", e),
    };
    if let Err(e) = serde_json::to_writer(file, &*map) {
        panic!("Failed to write guild config file: {}", e);
    }
}
#[derive(Debug, Clone, Serialize)]
struct InnerGuildConfig {
    pub empty_channel_timeout: Duration,
    pub default_song_volume: f32,
    pub default_radio_volume: f32,
    pub read_titles_by_default: bool,
    pub radio_audio_url: Option<Arc<str>>,
    pub radio_data_url: Option<Arc<str>>,
}
impl Default for InnerGuildConfig {
    fn default() -> Self {
        Self {
            // Defaults to 30 seconds
            empty_channel_timeout: Duration::from_secs(30),
            default_song_volume: 1.0,
            default_radio_volume: 1.0 / 3.0,
            read_titles_by_default: true,
            radio_audio_url: None,
            radio_data_url: None,
        }
    }
}
#[derive(Debug, Clone, Deserialize)]
struct RawInnerGuildConfig {
    empty_channel_timeout: Option<Duration>,
    default_song_volume: Option<f32>,
    default_radio_volume: Option<f32>,
    read_titles_by_default: Option<bool>,
    radio_audio_url: Option<Arc<str>>,
    radio_data_url: Option<Arc<str>>,
}
impl RawInnerGuildConfig {
    fn with_defaults(self) -> InnerGuildConfig {
        InnerGuildConfig {
            empty_channel_timeout: self
                .empty_channel_timeout
                .unwrap_or(Duration::from_secs(30)),
            default_song_volume: self.default_song_volume.unwrap_or(1.0),
            default_radio_volume: self.default_radio_volume.unwrap_or(1.0 / 3.0),
            read_titles_by_default: self.read_titles_by_default.unwrap_or(true),
            radio_audio_url: self.radio_audio_url,
            radio_data_url: self.radio_data_url,
        }
    }
}
pub struct GuildConfig {
    guild: GuildId,
    inner: InnerGuildConfig,
}
impl GuildConfig {
    pub async fn get(guild: GuildId) -> Self {
        let mut inner = RWLOCK.write().await;
        let inner = inner.entry(guild).or_default().clone();
        Self { guild, inner }
    }
    pub async fn write(self) {
        {
            let mut map = RWLOCK.write().await;
            map.insert(self.guild, self.inner);
        }
        save().await;
    }
    // Time until the bot leaves the channel if it's empty
    pub fn get_empty_channel_timeout(&self) -> Duration {
        self.inner.empty_channel_timeout
    }
    pub fn set_empty_channel_timeout(mut self, timeout: Duration) -> Self {
        self.inner.empty_channel_timeout = timeout;
        self
    }
    // Default volume for songs when the bot joins
    pub fn get_default_song_volume(&self) -> f32 {
        self.inner.default_song_volume
    }
    pub fn set_default_song_volume(mut self, volume: f32) -> Self {
        self.inner.default_song_volume = volume;
        self
    }
    // Default volume for radio when the bot joins
    pub fn get_default_radio_volume(&self) -> f32 {
        self.inner.default_radio_volume
    }
    pub fn set_default_radio_volume(mut self, volume: f32) -> Self {
        self.inner.default_radio_volume = volume;
        self
    }
    // Whether the bot should read titles by default
    pub fn get_read_titles_by_default(&self) -> bool {
        self.inner.read_titles_by_default
    }
    pub fn set_read_titles_by_default(mut self, read: bool) -> Self {
        self.inner.read_titles_by_default = read;
        self
    }
    // The url for the audio stream
    pub fn get_radio_audio_url(&self) -> Option<Arc<str>> {
        self.inner.radio_audio_url.as_ref().map(Arc::clone)
    }
    pub fn set_radio_audio_url(mut self, url: Option<Arc<str>>) -> Self {
        self.inner.radio_audio_url = url;
        self
    }
    // The url for the data stream
    pub fn get_radio_data_url(&self) -> Option<Arc<str>> {
        self.inner.radio_data_url.as_ref().map(Arc::clone)
    }
    pub fn set_radio_data_url(mut self, url: Option<Arc<str>>) -> Self {
        self.inner.radio_data_url = url;
        self
    }
}

pub async fn extract_all() -> HashMap<GuildId, GuildConfig> {
    RWLOCK
        .read()
        .await
        .clone()
        .into_iter()
        .map(|(k, v)| (k, GuildConfig { guild: k, inner: v }))
        .collect()
}
