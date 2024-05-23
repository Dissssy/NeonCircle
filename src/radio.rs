use anyhow::Result;
use serde::{Deserialize, Serialize};
use serenity::futures::{
    channel::{mpsc, oneshot},
    SinkExt as _, StreamExt as _,
};
use std::sync::Arc;
use tokio::sync::broadcast;
#[derive(Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
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
impl Root {
    pub async fn get() -> Result<Self> {
        let url = crate::Config::get().api_url;
        let data = crate::WEB_CLIENT
            .get(&url)
            .send()
            .await?
            .json::<Root>()
            .await?;
        Ok(data)
    }
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
    recv: broadcast::Receiver<Arc<Root>>,
    manual_current: mpsc::Sender<oneshot::Sender<Arc<Root>>>,
    handle: tokio::task::JoinHandle<()>,
}
impl AzuraCastThread {
    pub async fn new() -> Result<Self> {
        let (kill, mut rx) = mpsc::channel(1);
        let (send_data, recv) = broadcast::channel(1);
        let (manual_current, mut manual_current_recv) =
            mpsc::channel::<oneshot::Sender<Arc<Root>>>(1);
        let handle = {
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(100));
                let mut last_data = None;
                loop {
                    tokio::select! {
                        _ = tick.tick() => {
                            let data = Root::get().await;
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
                        Some(sender) = manual_current_recv.next() => {
                            if let Some(data) = last_data.as_ref() {
                                if let Err(e) = sender.send(Arc::clone(data)) {
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
    pub async fn resubscribe(&mut self) -> Result<(broadcast::Receiver<Arc<Root>>, Arc<Root>)> {
        let (send, recv) = oneshot::channel();
        self.manual_current.send(send).await?;
        let data = recv.await?;
        Ok((self.recv.resubscribe(), data))
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
