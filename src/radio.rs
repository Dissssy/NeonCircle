use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::{sync::Mutex, time::Instant};

pub struct AzuraCast {
    data: Arc<Mutex<Root>>,
    last_update: Instant,
    url: String,
}

impl AzuraCast {
    pub async fn new(url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let data = reqwest::get(url).await?.json::<Root>().await?;
        Ok(Self {
            data: Arc::new(Mutex::new(data)),
            last_update: Instant::now(),
            url: url.to_string(),
        })
    }

    pub async fn slow_data(&mut self) -> Root {
        if self.last_update.elapsed().as_secs() > 5 {
            match self.data.lock().await.update(&self.url).await {
                Ok(_) => {}
                Err(e) => {
                    println!("Failed to update data: {}", e);
                }
            }
            self.last_update = Instant::now();
        }
        self.data.lock().await.clone()
    }

    #[allow(dead_code)]
    pub async fn fast_data(&mut self) -> Root {
        // dispatch a task to update the data
        let d = self.data.clone();
        let url = self.url.clone();
        if self.last_update.elapsed().as_secs() > 5 {
            tokio::spawn(async move {
                match d.lock().await.update(&url).await {
                    Ok(_) => {}
                    Err(e) => {
                        println!("Failed to update data: {}", e);
                    }
                }
            });
            self.last_update = Instant::now();
        }
        self.data.lock().await.clone()
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub station: Station,
    pub listeners: Listeners2,
    pub live: Live,
    #[serde(rename = "now_playing")]
    pub now_playing: NowPlaying,
    #[serde(rename = "playing_next")]
    pub playing_next: PlayingNext,
    #[serde(rename = "song_history")]
    pub song_history: Vec<SongHistory>,
    #[serde(rename = "is_online")]
    pub is_online: bool,
}

impl Root {
    pub async fn update(&mut self, url: &str) -> Result<(), Box<dyn std::error::Error>> {
        let data = reqwest::get(url).await?.json::<Root>().await?;
        *self = data;
        Ok(())
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Station {
    pub id: i64,
    pub name: String,
    pub shortcode: String,
    pub description: String,
    pub frontend: String,
    pub backend: String,
    #[serde(rename = "listen_url")]
    pub listen_url: String,
    pub url: String,
    #[serde(rename = "public_player_url")]
    pub public_player_url: String,
    #[serde(rename = "playlist_pls_url")]
    pub playlist_pls_url: String,
    #[serde(rename = "playlist_m3u_url")]
    pub playlist_m3u_url: String,
    #[serde(rename = "is_public")]
    pub is_public: bool,
    pub mounts: Vec<Mount>,
    #[serde(rename = "hls_enabled")]
    pub hls_enabled: bool,
    #[serde(rename = "hls_url")]
    pub hls_url: String,
    #[serde(rename = "hls_listeners")]
    pub hls_listeners: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mount {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub bitrate: i64,
    pub format: String,
    pub listeners: Listeners,
    pub path: String,
    #[serde(rename = "is_default")]
    pub is_default: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listeners {
    pub total: i64,
    pub unique: i64,
    pub current: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listeners2 {
    pub total: i64,
    pub unique: i64,
    pub current: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Live {
    #[serde(rename = "is_live")]
    pub is_live: bool,
    #[serde(rename = "streamer_name")]
    pub streamer_name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    #[serde(rename = "sh_id")]
    pub sh_id: i64,
    #[serde(rename = "played_at")]
    pub played_at: i64,
    pub duration: i64,
    pub playlist: String,
    pub streamer: String,
    #[serde(rename = "is_request")]
    pub is_request: bool,
    pub song: Song,
    pub elapsed: i64,
    pub remaining: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub text: String,
    pub artist: String,
    pub title: String,
    pub album: String,
    pub genre: String,
    pub isrc: String,
    pub lyrics: String,
    pub art: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayingNext {
    #[serde(rename = "cued_at")]
    pub cued_at: i64,
    #[serde(rename = "played_at")]
    pub played_at: i64,
    pub duration: i64,
    pub playlist: String,
    #[serde(rename = "is_request")]
    pub is_request: bool,
    pub song: Song,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongHistory {
    #[serde(rename = "sh_id")]
    pub sh_id: i64,
    #[serde(rename = "played_at")]
    pub played_at: i64,
    pub duration: i64,
    pub playlist: String,
    pub streamer: String,
    #[serde(rename = "is_request")]
    pub is_request: bool,
    pub song: Song,
}
