use std::path::PathBuf;
pub fn get_config() -> Config {
    Config::get()
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    pub bots_config_path: PathBuf,
    pub guild_id: String,
    pub app_name: String,
    pub looptime: u64,
    pub data_path: PathBuf,
    pub shitgpt_path: PathBuf,
    pub whitelist_path: PathBuf,
    pub string_api_token: String,
    pub idle_url: String,
    pub api_url: String,
    #[cfg(feature = "tts")]
    pub gcloud_script: String,
    #[cfg(feature = "youtube-search")]
    pub youtube_api_key: String,
    #[cfg(feature = "youtube-search")]
    pub autocomplete_limit: u64,
    #[cfg(feature = "spotify")]
    pub spotify_api_key: String,
    pub bumper_url: String,
    #[cfg(feature = "transcribe")]
    pub transcribe_url: String,
    #[cfg(feature = "transcribe")]
    pub transcribe_token: String,
    #[cfg(feature = "transcribe")]
    pub alert_phrases_path: PathBuf,
    #[cfg(feature = "transcribe")]
    pub sam_path: PathBuf,
    #[cfg(feature = "transcribe")]
    pub consent_path: PathBuf,
    #[cfg(feature = "transcribe")]
    pub transcription_map_path: PathBuf,
    pub guild_config_path: PathBuf,
}
impl Config {
    pub fn get() -> Self {
        let path = dirs::data_dir();
        let mut path = if let Some(path) = path {
            path
        } else {
            PathBuf::from(".")
        };
        path.push("RmbConfig.json");
        Self::get_from_path(path)
    }
    fn onboarding(config_path: &PathBuf, recovered_config: Option<RecoverConfig>) {
        let config = if let Some(rec) = recovered_config {
            log::error!("Welcome back to my shitty Rust Music Bot!");
            log::error!(
                "It appears that you have run the bot before, but the config got biffed up."
            );
            log::error!("I will take you through a short onboarding process to get you back up and running.");
            let app_name = if let Some(app_name) = rec.app_name {
                app_name
            } else {
                Self::safe_read("\nPlease enter your application name:")
            };
            let mut data_path = match config_path.parent() {
                Some(p) => p.to_path_buf(),
                None => {
                    log::error!("Failed to get parent, this should never happen.");
                    return;
                }
            };
            data_path.push(app_name.clone());
            Config {
                bots_config_path: if let Some(bots_config_path) = rec.bots_config_path {
                    bots_config_path
                } else {
                    Self::safe_read("\nPlease enter your bots config path:")
                },
                guild_id: if let Some(guild_id) = rec.guild_id {
                    guild_id
                } else {
                    Self::safe_read("\nPlease enter your guild id:")
                },
                app_name,
                looptime: if let Some(looptime) = rec.looptime {
                    looptime
                } else {
                    Self::safe_read("\nPlease enter your loop time in ms\nlower time means faster response but potentially higher cpu utilization (50 is a good compromise):")
                },
                #[cfg(feature = "tts")]
                gcloud_script: if let Some(gcloud_script) = rec.gcloud_script {
                    gcloud_script
                } else {
                    Self::safe_read("\nPlease enter your gcloud script location:")
                },
                #[cfg(feature = "youtube-search")]
                youtube_api_key: if let Some(youtube_api_key) = rec.youtube_api_key {
                    youtube_api_key
                } else {
                    Self::safe_read("\nPlease enter your youtube api key:")
                },
                #[cfg(feature = "youtube-search")]
                autocomplete_limit: if let Some(autocomplete_limit) = rec.autocomplete_limit {
                    autocomplete_limit
                } else {
                    Self::safe_read("\nPlease enter your youtube autocomplete limit:")
                },
                #[cfg(feature = "spotify")]
                spotify_api_key: if let Some(spotify_api_key) = rec.spotify_api_key {
                    spotify_api_key
                } else {
                    Self::safe_read("\nPlease enter your spotify api key:")
                },
                idle_url: if let Some(idle_audio) = rec.idle_url {
                    idle_audio
                } else {
                    Self::safe_read("\nPlease enter your idle audio URL (NOT A FILE PATH)\nif you wish to use a file on disk, set this to something as a fallback, and name the file override.mp3 inside the bot directory)\n(appdata/local/ for windows users and ~/.local/share/ for linux users):")
                },
                api_url: if let Some(api_url) = rec.api_url {
                    api_url
                } else {
                    Self::safe_read("\nPlease enter your api url:")
                },
                bumper_url: if let Some(bumper_url) = rec.bumper_url {
                    bumper_url
                } else {
                    Self::safe_read("\nPlease enter your bumper audio URL (NOT A FILE PATH) (for silence put \"https://www.youtube.com/watch?v=Vbks4abvLEw\"):")
                },
                data_path: if let Some(data_path) = rec.data_path {
                    data_path
                } else {
                    data_path
                },
                shitgpt_path: if let Some(shitgpt_path) = rec.shitgpt_path {
                    shitgpt_path
                } else {
                    Self::safe_read("\nPlease enter your shitgpt path:")
                },
                whitelist_path: if let Some(whitelist_path) = rec.whitelist_path {
                    whitelist_path
                } else {
                    Self::safe_read("\nPlease enter your whitelist path:")
                },
                string_api_token: if let Some(string_api_token) = rec.string_api_token {
                    string_api_token
                } else {
                    Self::safe_read("\nPlease enter your string api token:")
                },
                #[cfg(feature = "transcribe")]
                transcribe_url: if let Some(transcribe_url) = rec.transcribe_url {
                    transcribe_url
                } else {
                    Self::safe_read("\nPlease enter your transcribe url:")
                },
                #[cfg(feature = "transcribe")]
                transcribe_token: if let Some(transcribe_token) = rec.transcribe_token {
                    transcribe_token
                } else {
                    Self::safe_read("\nPlease enter your transcribe token:")
                },
                #[cfg(feature = "transcribe")]
                alert_phrases_path: if let Some(alert_phrase_path) = rec.alert_phrase_path {
                    alert_phrase_path
                } else {
                    Self::safe_read("\nPlease enter your alert phrase path:")
                },
                #[cfg(feature = "transcribe")]
                sam_path: if let Some(sam_path) = rec.sam_path {
                    sam_path
                } else {
                    Self::safe_read("\nPlease enter your sam path:")
                },
                #[cfg(feature = "transcribe")]
                consent_path: if let Some(consent_path) = rec.consent_path {
                    consent_path
                } else {
                    Self::safe_read("\nPlease enter your consent path:")
                },
                guild_config_path: if let Some(guild_config_path) = rec.guild_config_path {
                    guild_config_path
                } else {
                    Self::safe_read("\nPlease enter your guild config path:")
                },
                transcription_map_path: if let Some(transcription_map_path) =
                    rec.transcription_map_path
                {
                    transcription_map_path
                } else {
                    Self::safe_read("\nPlease enter your transcription map path:")
                },
            }
        } else {
            log::error!("Welcome to my shitty Rust Music Bot!");
            log::error!("It appears that this may be the first time you are running the bot.");
            log::error!("I will take you through a short onboarding process to get you started.");
            let app_name: String = Self::safe_read("\nPlease enter your application name:");
            let mut data_path = match config_path.parent() {
                Some(p) => p.to_path_buf(),
                None => {
                    log::error!("Failed to get parent, this should never happen.");
                    return;
                }
            };
            data_path.push(app_name.clone());
            Config {
                bots_config_path: Self::safe_read("\nPlease enter your bots config path:"),
                guild_id: Self::safe_read("\nPlease enter your guild id:"),
                app_name,
                looptime: Self::safe_read("\nPlease enter your loop time in ms\nlower time means faster response but higher utilization:"),
                #[cfg(feature = "tts")]
                gcloud_script: Self::safe_read("\nPlease enter your gcloud script location:"),
                data_path,
                #[cfg(feature = "youtube-search")]
                youtube_api_key: Self::safe_read("\nPlease enter your youtube api key:"),
                #[cfg(feature = "youtube-search")]
                autocomplete_limit: Self::safe_read("\nPlease enter your youtube autocomplete limit:"),
                #[cfg(feature = "spotify")]
                spotify_api_key: Self::safe_read("\nPlease enter your spotify api key:"),
                idle_url: Self::safe_read("\nPlease enter your idle audio URL (NOT A FILE PATH):"),
                api_url: Self::safe_read("\nPlease enter your api url:"),
                bumper_url: Self::safe_read("\nPlease enter your bumper audio URL (NOT A FILE PATH) (for silence put \"https://www.youtube.com/watch?v=Vbks4abvLEw\"):"),
                shitgpt_path: Self::safe_read("\nPlease enter your shitgpt path:"),
                whitelist_path: Self::safe_read("\nPlease enter your whitelist path:"),
                string_api_token: Self::safe_read("\nPlease enter your string api token:"),
                #[cfg(feature = "transcribe")]
                transcribe_token: Self::safe_read("\nPlease enter your transcribe token:"),
                #[cfg(feature = "transcribe")]
                transcribe_url: Self::safe_read("\nPlease enter your transcribe url:"),
                #[cfg(feature = "transcribe")]
                alert_phrases_path: Self::safe_read("\nPlease enter your alert phrase path:"),
                #[cfg(feature = "transcribe")]
                sam_path: Self::safe_read("\nPlease enter your sam path:"),
                #[cfg(feature = "transcribe")]
                consent_path: Self::safe_read("\nPlease enter your consent path:"),
                guild_config_path: Self::safe_read("\nPlease enter your guild config path:"),
                transcription_map_path: Self::safe_read("\nPlease enter your transcription map path:"),
            }
        };
        match std::fs::write(
            config_path.clone(),
            match serde_json::to_string_pretty(&config) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("Failed to serialize config: {}", e);
                    return;
                }
            },
        ) {
            Ok(_) => {
                log::info!("Config written to {:?}", config_path);
            }
            Err(e) => {
                log::error!("Failed to write config to {:?}: {}", config_path, e);
            }
        }
    }
    fn safe_read<T: std::str::FromStr>(prompt: &str) -> T {
        loop {
            log::error!("{}", prompt);
            let mut input = String::new();
            if let Err(e) = std::io::stdin().read_line(&mut input) {
                log::error!("Failed to read input: {}", e);
                continue;
            }
            let input = input.trim();
            match input.parse::<T>() {
                Ok(input) => return input,
                Err(_) => log::error!("Invalid input"),
            }
        }
    }
    fn get_from_path(path: std::path::PathBuf) -> Self {
        if !path.exists() {
            Self::onboarding(&path, None);
        }
        let config = std::fs::read_to_string(&path);
        if let Ok(config) = config {
            let x: Result<Config, serde_json::Error> = serde_json::from_str(&config);
            if let Ok(x) = x {
                x
            } else {
                log::error!("Failed to parse config.json, Attempting recovery");
                let recovered = serde_json::from_str(&config);
                if let Ok(recovered) = recovered {
                    Self::onboarding(&path, Some(recovered));
                } else {
                    Self::onboarding(&path, None);
                }
                Self::get()
            }
        } else {
            log::error!("Failed to read config.json");
            Self::onboarding(&path, None);
            Self::get_from_path(path)
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RecoverConfig {
    bots_config_path: Option<PathBuf>,
    guild_id: Option<String>,
    app_name: Option<String>,
    looptime: Option<u64>,
    data_path: Option<PathBuf>,
    #[cfg(feature = "tts")]
    gcloud_script: Option<String>,
    #[cfg(feature = "youtube-search")]
    youtube_api_key: Option<String>,
    #[cfg(feature = "youtube-search")]
    autocomplete_limit: Option<u64>,
    #[cfg(feature = "spotify")]
    spotify_api_key: Option<String>,
    idle_url: Option<String>,
    api_url: Option<String>,
    shitgpt_path: Option<PathBuf>,
    whitelist_path: Option<PathBuf>,
    string_api_token: Option<String>,
    bumper_url: Option<String>,
    #[cfg(feature = "transcribe")]
    transcribe_url: Option<String>,
    #[cfg(feature = "transcribe")]
    transcribe_token: Option<String>,
    #[cfg(feature = "transcribe")]
    alert_phrase_path: Option<PathBuf>,
    #[cfg(feature = "transcribe")]
    sam_path: Option<PathBuf>,
    #[cfg(feature = "transcribe")]
    consent_path: Option<PathBuf>,
    #[cfg(feature = "transcribe")]
    transcription_map_path: Option<PathBuf>,
    guild_config_path: Option<PathBuf>,
}
