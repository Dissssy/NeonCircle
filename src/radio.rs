use std::{sync::Arc, time::Duration};

use anyhow::Error;
use serde::{Deserialize, Serialize};
use tokio::{sync::Mutex, time::Instant};

use crate::commands::music::mainloop::Log;

pub struct AzuraCast {
    data: Arc<Mutex<Root>>,
    log: Log,
    last_update: Instant,
    url: String,
    timeout: Duration,
}

#[allow(dead_code)]
impl AzuraCast {
    pub async fn new(
        url: &str,
        log: Log,
        timeout: Duration,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let data = reqwest::get(url).await?.json::<Root>().await?;
        Ok(Self {
            data: Arc::new(Mutex::new(data)),
            last_update: Instant::now(),
            url: url.to_string(),
            log,
            timeout,
        })
    }

    pub async fn slow_data(&mut self) -> Result<Root, Error> {
        let r = tokio::time::timeout(self.timeout, self.data.lock()).await;

        match r {
            Ok(mut i) => {
                let r = tokio::time::timeout(self.timeout, i.update(&self.url)).await;
                match r {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        self.log
                            .log(&format!("Error updating azuracast data: {}", e))
                            .await;
                    }
                    Err(e) => {
                        self.log
                            .log(&format!("Timeout updating azuracast data: {}", e))
                            .await;
                    }
                }
            }
            Err(e) => {
                self.log
                    .log(&format!("Timeout getting azuracast data: {}", e))
                    .await;
            }
        }

        let r = tokio::time::timeout(self.timeout, self.data.lock()).await;

        match r {
            Ok(i) => Ok(i.clone()),
            Err(e) => {
                self.log
                    .log(&format!("Timeout getting azuracast data: {}", e))
                    .await;
                Err(e.into())
            }
        }
    }

    pub async fn fast_data(&mut self) -> Result<Root, Error> {
        if self.last_update.elapsed().as_secs() > 5 {
            let d = self.data.clone();
            let url = self.url.clone();
            let log = self.log.clone();
            let timeout = self.timeout;
            tokio::spawn(async move {
                let r = tokio::time::timeout(timeout, d.lock()).await;

                match r {
                    Ok(mut i) => {
                        let r = tokio::time::timeout(timeout, i.update(&url)).await;
                        match r {
                            Ok(Ok(())) => {}
                            Ok(Err(e)) => {
                                log.log(&format!("Error updating azuracast data: {}", e))
                                    .await;
                            }
                            Err(e) => {
                                log.log(&format!("Timeout updating azuracast data: {}", e))
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        log.log(&format!("Timeout getting azuracast data: {}", e))
                            .await;
                    }
                }
            });
            self.last_update = Instant::now();
        }

        let r = tokio::time::timeout(self.timeout, self.data.lock()).await;

        match r {
            Ok(i) => Ok(i.clone()),
            Err(e) => {
                self.log
                    .log(&format!("Timeout getting azuracast data: {}", e))
                    .await;
                Err(e.into())
            }
        }
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
    pub async fn update(&mut self, url: &str) -> Result<(), Error> {
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
