use crate::Config;
use serde::{Deserialize, Serialize};
use serenity::all::GuildId;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;
lazy_static::lazy_static! {
    static ref RWLOCK: RwLock<HashMap<GuildId, InnerGuildConfig>> = {
        let file = match std::fs::File::open(Config::get().guild_config_path) {
            Ok(f) => f,
            Err(_) => {
                let f = match std::fs::File::create(Config::get().guild_config_path) {
                    Ok(f) => f,
                    Err(e) => panic!("Failed to create guild config file: {}", e),
                };
                if let Err(e) = serde_json::to_writer(f, &HashMap::<GuildId, InnerGuildConfig>::new()) {
                    panic!("Failed to write default guild config file: {}", e);
                }
                match std::fs::File::open(Config::get().guild_config_path) {
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
pub fn init() {
    if let Err(e) = RWLOCK.read() {
        panic!("Failed to read guild config file: {}", e);
    }
}
pub fn save() {
    let map = match RWLOCK.read() {
        Ok(r) => r,
        Err(e) => {
            log::error!("Failed to read guild config file: {}", e);
            e.into_inner()
        }
    };
    let file = match std::fs::File::create(Config::get().guild_config_path) {
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
}
impl Default for InnerGuildConfig {
    fn default() -> Self {
        Self {
            // Defaults to 30 seconds
            empty_channel_timeout: Duration::from_secs(30),
            default_song_volume: 1.0,
            default_radio_volume: 1.0 / 3.0,
        }
    }
}
#[derive(Debug, Clone, Deserialize)]
struct RawInnerGuildConfig {
    pub empty_channel_timeout: Option<Duration>,
    pub default_song_volume: Option<f32>,
    pub default_radio_volume: Option<f32>,
}
impl RawInnerGuildConfig {
    fn with_defaults(self) -> InnerGuildConfig {
        InnerGuildConfig {
            empty_channel_timeout: self
                .empty_channel_timeout
                .unwrap_or(Duration::from_secs(30)),
            default_song_volume: self.default_song_volume.unwrap_or(1.0),
            default_radio_volume: self.default_radio_volume.unwrap_or(1.0 / 3.0),
        }
    }
}
pub struct GuildConfig {
    guild: GuildId,
    inner: InnerGuildConfig,
}
impl GuildConfig {
    pub fn get(guild: GuildId) -> Self {
        let mut inner = match RWLOCK.write() {
            Ok(r) => r,
            Err(e) => {
                log::error!("Failed to read guild config file: {}", e);
                e.into_inner()
            }
        };
        let inner = inner.entry(guild).or_default().clone();
        Self { guild, inner }
    }
    pub fn write(self) {
        {
            let mut map = match RWLOCK.write() {
                Ok(r) => r,
                Err(e) => {
                    log::error!("Failed to write guild config file: {}", e);
                    e.into_inner()
                }
            };
            map.insert(self.guild, self.inner);
        }
        save();
    }
    pub fn get_empty_channel_timeout(&self) -> Duration {
        self.inner.empty_channel_timeout
    }
    pub fn set_empty_channel_timeout(mut self, timeout: Duration) -> Self {
        self.inner.empty_channel_timeout = timeout;
        self
    }
    pub fn get_default_song_volume(&self) -> f32 {
        self.inner.default_song_volume
    }
    pub fn set_default_song_volume(mut self, volume: f32) -> Self {
        self.inner.default_song_volume = volume;
        self
    }
    pub fn get_default_radio_volume(&self) -> f32 {
        self.inner.default_radio_volume
    }
    pub fn set_default_radio_volume(mut self, volume: f32) -> Self {
        self.inner.default_radio_volume = volume;
        self
    }
}
