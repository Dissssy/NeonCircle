use anyhow::Result;
use reqwest::IntoUrl;
use serde::{Deserialize, Serialize};
use serenity::futures::{channel::mpsc, SinkExt as _, StreamExt as _};
use std::{ops::Deref, sync::Arc};
use tokio::sync::broadcast;
#[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzuraCastData {
    pub station: Station,
    // pub listeners: Listeners2,
    // pub live: Live,
    #[serde(rename = "now_playing")]
    pub now_playing: NowPlaying,
    #[serde(rename = "playing_next")]
    pub playing_next: PlayingNext,
    // #[serde(rename = "song_history")]
    // pub song_history: Vec<SongHistory>,
    // #[serde(rename = "is_online")]
    // pub is_online: bool,
}
#[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Station {
    // pub id: i64,
    pub name: String,
    // pub shortcode: String,
    // pub description: String,
    // pub frontend: String,
    // pub backend: String,
    // #[serde(rename = "listen_url")]
    // pub listen_url: String,
    // pub url: String,
    // #[serde(rename = "public_player_url")]
    // pub public_player_url: String,
    // #[serde(rename = "playlist_pls_url")]
    // pub playlist_pls_url: String,
    // #[serde(rename = "playlist_m3u_url")]
    // pub playlist_m3u_url: String,
    // #[serde(rename = "is_public")]
    // pub is_public: bool,
    // pub mounts: Vec<Mount>,
    // #[serde(rename = "hls_enabled")]
    // pub hls_enabled: bool,
    // #[serde(rename = "hls_url")]
    // pub hls_url: String,
    // #[serde(rename = "hls_listeners")]
    // pub hls_listeners: i64,
}
// #[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct Mount {
//     pub id: i64,
//     pub name: String,
//     pub url: String,
//     pub bitrate: i64,
//     pub format: String,
//     pub listeners: Listeners,
//     pub path: String,
//     #[serde(rename = "is_default")]
//     pub is_default: bool,
// }
// #[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct Listeners {
//     pub total: i64,
//     pub unique: i64,
//     pub current: i64,
// }
// #[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct Listeners2 {
//     pub total: i64,
//     pub unique: i64,
//     pub current: i64,
// }
// #[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct Live {
//     #[serde(rename = "is_live")]
//     pub is_live: bool,
//     #[serde(rename = "streamer_name")]
//     pub streamer_name: String,
// }
#[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    // #[serde(rename = "sh_id")]
    // pub sh_id: i64,
    // #[serde(rename = "played_at")]
    // pub played_at: i64,
    // pub duration: i64,
    // pub playlist: String,
    // pub streamer: String,
    // #[serde(rename = "is_request")]
    // pub is_request: bool,
    pub song: Song,
    // pub elapsed: i64,
    // pub remaining: i64,
}
#[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    // pub id: String,
    // pub text: String,
    pub artist: String,
    pub title: String,
    pub album: String,
    // pub genre: String,
    // pub isrc: String,
    // pub lyrics: String,
    pub art: String,
}
#[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayingNext {
    // #[serde(rename = "cued_at")]
    // pub cued_at: i64,
    // #[serde(rename = "played_at")]
    // pub played_at: i64,
    // pub duration: i64,
    // pub playlist: String,
    // #[serde(rename = "is_request")]
    // pub is_request: bool,
    pub song: Song,
}
// #[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct SongHistory {
//     #[serde(rename = "sh_id")]
//     pub sh_id: i64,
//     #[serde(rename = "played_at")]
//     pub played_at: i64,
//     pub duration: i64,
//     pub playlist: String,
//     pub streamer: String,
//     #[serde(rename = "is_request")]
//     pub is_request: bool,
//     pub song: Song,
// }
pub struct AzuraCastThread {
    kill: mpsc::Sender<()>,
    recv: broadcast::Receiver<Arc<OriginalOrCustom>>,
    manual_current: mpsc::Sender<()>,
    handle: tokio::task::JoinHandle<()>,
}
impl AzuraCastThread {
    pub async fn new() -> Result<Self> {
        let (kill, mut rx) = mpsc::channel(1);
        let (send_data, recv) = broadcast::channel(1);
        let (manual_current, mut manual_current_recv) = mpsc::channel::<()>(1);
        let handle = {
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(1500));
                let mut last_data = None;
                let url = crate::config::get_config().api_url;
                loop {
                    tokio::select! {
                        _ = tick.tick() => {
                            let data = RadioData::get_original(&url).await;
                            match data {
                                Ok(d) => {
                                    let data = Arc::new(d);
                                    if last_data.as_ref() == Some(&data) {
                                        continue;
                                    }
                                    last_data = Some(Arc::clone(&data));
                                    if let Err(e) = send_data.send(data) {
                                        log::error!("Failed to send azuracast data: {:?}", e);
                                    }
                                }
                                Err(e) => {
                                    log::error!("Failed to get azuracast data: {:?}", e);
                                }
                            }
                        }
                        Some(()) = manual_current_recv.next() => {
                            // this is a signal to send the last good data to all listeners
                            if let Some(data) = last_data.as_ref().map(Arc::clone) {
                                if let Err(e) = send_data.send(data) {
                                    log::error!("Failed to send manual azuracast data: {:?}", e);
                                }
                            }
                        }
                        _ = rx.next() => {
                            break;
                        }
                    }
                }
            })
        };
        Ok(Self {
            kill,
            recv,
            handle,
            manual_current,
        })
    }
    pub async fn signal_resend(&mut self) {
        if let Err(e) = self.manual_current.send(()).await {
            log::error!("Failed to signal resend: {:?}", e);
        }
    }
    pub async fn resubscribe(&mut self) -> broadcast::Receiver<Arc<OriginalOrCustom>> {
        let recv = self.recv.resubscribe();
        self.signal_resend().await;
        recv
    }
    pub async fn kill(mut self) {
        if let Err(e) = self.kill.send(()).await {
            log::error!("Failed to kill azuracast thread: {:?}", e);
        }
        if let Err(e) = self.handle.await {
            log::error!("Failed to await azuracast thread: {:?}", e);
        }
    }
}
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RadioData {
    AzuraCast(AzuraCastData),
    IceCast(IceCastRoot),
}
#[derive(Debug, PartialEq)]
pub enum OriginalOrCustom {
    Original(RadioData),
    Custom(RadioData),
}
impl Deref for OriginalOrCustom {
    type Target = RadioData;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Original(data) => data,
            Self::Custom(data) => data,
        }
    }
}
impl OriginalOrCustom {
    pub fn is_original(&self) -> bool {
        matches!(self, Self::Original(_))
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum RadioDataKind {
    AzuraCast,
    IceCast,
}
impl RadioData {
    pub async fn get(url: impl IntoUrl) -> Result<OriginalOrCustom> {
        let data = crate::statics::WEB_CLIENT
            .get(url)
            .send()
            .await?
            .json::<Self>()
            .await?;
        Ok(OriginalOrCustom::Custom(data))
    }
    pub fn kind(&self) -> RadioDataKind {
        match self {
            Self::AzuraCast(_) => RadioDataKind::AzuraCast,
            Self::IceCast(_) => RadioDataKind::IceCast,
        }
    }
    async fn get_original(url: impl IntoUrl) -> Result<OriginalOrCustom> {
        let data = crate::WEB_CLIENT
            .get(url)
            .send()
            .await?
            .json::<Self>()
            .await?;
        Ok(OriginalOrCustom::Original(data))
    }
    pub fn station_name(&self, url: &str) -> &str {
        match self {
            Self::AzuraCast(data) => &data.station.name,
            Self::IceCast(data) => {
                let search_for = url.split('/').last().unwrap_or(url);
                data.icestats
                    .source
                    .iter()
                    .find(|s| s.listenurl.ends_with(search_for))
                    .map_or("Unknown station name", |s| &s.server_name)
            }
        }
    }
    pub fn now_playing_title(&self, url: &str) -> &str {
        match self {
            Self::AzuraCast(data) => &data.now_playing.song.title,
            Self::IceCast(data) => {
                let search_for = url.split('/').last().unwrap_or(url);
                data.icestats
                    .source
                    .iter()
                    .find(|s| s.listenurl.ends_with(search_for))
                    .and_then(|s| s.title.as_deref())
                    .unwrap_or("Unknown title")
            }
        }
    }
    pub fn now_playing_artist(&self, url: &str) -> Option<&str> {
        match self {
            Self::AzuraCast(data) => Some(&data.now_playing.song.artist),
            Self::IceCast(data) => {
                let search_for = url.split('/').last().unwrap_or(url);
                data.icestats
                    .source
                    .iter()
                    .find(|s| s.listenurl.ends_with(search_for))
                    .and_then(|s| s.artist.as_deref())
            }
        }
    }
    pub fn now_playing_album(&self, _url: &str) -> Option<&str> {
        match self {
            Self::AzuraCast(data) => Some(&data.now_playing.song.album),
            Self::IceCast(_data) => None,
        }
    }
    pub fn now_playing_art(&self, _url: &str) -> Option<&str> {
        match self {
            Self::AzuraCast(data) => Some(&data.now_playing.song.art),
            Self::IceCast(_data) => None,
        }
    }
    // icecast doesn't have a playing next dataset
    pub fn playing_next_title(&self, _url: &str) -> Option<&str> {
        match self {
            Self::AzuraCast(data) => Some(&data.playing_next.song.title),
            Self::IceCast(_data) => None,
        }
    }
    pub fn playing_next_artist(&self, _url: &str) -> Option<&str> {
        match self {
            Self::AzuraCast(data) => Some(&data.playing_next.song.artist),
            Self::IceCast(_data) => None,
        }
    }
    pub fn playing_next_album(&self, _url: &str) -> Option<&str> {
        match self {
            Self::AzuraCast(data) => Some(&data.playing_next.song.album),
            Self::IceCast(_data) => None,
        }
    }
}
#[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct IceCastRoot {
    pub icestats: IceCastStats,
}
#[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct IceCastStats {
    // pub admin: String,
    // pub host: String,
    // pub location: String,
    // pub server_id: String,
    // pub server_start: String,
    // pub server_start_iso8601: String,
    pub source: Vec<IceCastSource>,
}
#[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct IceCastSource {
    pub artist: Option<String>,
    // pub audio_bitrate: Option<i64>,
    // pub audio_channels: Option<i64>,
    // pub audio_info: Option<String>,
    // pub audio_samplerate: Option<i64>,
    // pub bitrate: Option<i64>,
    // pub channels: Option<i64>,
    // pub genre: Option<String>,
    // pub ice_bitrate: Option<i64>,
    // pub listener_peak: Option<i64>,
    // pub listeners: Option<i64>,
    pub listenurl: String,
    // pub quality: Option<String>,
    // pub samplerate: Option<i64>,
    // pub server_description: Option<String>,
    pub server_name: String,
    // pub server_type: Option<String>,
    // pub stream_start: String,
    // pub stream_start_iso8601: String,
    pub title: Option<String>,
    // pub subtype: Option<String>,
    // pub dummy: Option<serde_json::Value>,
}
