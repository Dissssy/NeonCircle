#![allow(dead_code)]
use crate::commands::music::{LazyLoadedVideo, MetaVideo};
use crate::video::RawVideo;
#[cfg(feature = "tts")]
use crate::{commands::music::VideoType, video::Video};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
#[cfg(feature = "tts")]
use tokio::io::AsyncWriteExt;
#[cfg(all(feature = "misandry", feature = "misogyny"))]
compile_error!("Cannot enable both misandrist and misogynist features");
lazy_static::lazy_static!(
    pub static ref VOICES: Vec<TTSVoice> = {

        let mut v = if cfg!(feature = "google-journey-tts") {
            vec![
                TTSVoice::new("en-US", "en-US-Journey-D", "MALE"),
                TTSVoice::new("en-US", "en-US-Journey-F", "FEMALE"),
            ]
        } else {
            vec![
                TTSVoice::new("en-AU", "en-AU-Neural2-A", "FEMALE"),
                TTSVoice::new("en-AU", "en-AU-Neural2-B", "MALE"),
                TTSVoice::new("en-AU", "en-AU-Neural2-C", "FEMALE"),
                TTSVoice::new("en-AU", "en-AU-Neural2-D", "MALE"),
                TTSVoice::new("en-IN", "en-IN-Neural2-A", "FEMALE"),
                TTSVoice::new("en-IN", "en-IN-Neural2-B", "MALE"),
                TTSVoice::new("en-IN", "en-IN-Neural2-C", "MALE"),
                TTSVoice::new("en-IN", "en-IN-Neural2-D", "FEMALE"),
                TTSVoice::new("en-GB", "en-GB-Neural2-A", "FEMALE"),
                TTSVoice::new("en-GB", "en-GB-Neural2-B", "MALE"),
                TTSVoice::new("en-GB", "en-GB-Neural2-C", "FEMALE"),
                TTSVoice::new("en-GB", "en-GB-Neural2-D", "MALE"),
                TTSVoice::new("en-GB", "en-GB-Neural2-F", "FEMALE"),
                TTSVoice::new("en-US", "en-US-Neural2-A", "MALE"),
                TTSVoice::new("en-US", "en-US-Neural2-C", "FEMALE"),
                TTSVoice::new("en-US", "en-US-Neural2-D", "MALE"),
                TTSVoice::new("en-US", "en-US-Neural2-E", "FEMALE"),
                TTSVoice::new("en-US", "en-US-Neural2-F", "FEMALE"),
                TTSVoice::new("en-US", "en-US-Neural2-G", "FEMALE"),
                TTSVoice::new("en-US", "en-US-Neural2-H", "FEMALE"),
                TTSVoice::new("en-US", "en-US-Neural2-I", "MALE"),
                TTSVoice::new("en-US", "en-US-Neural2-J", "MALE"),
            ]
        };

        use rand::seq::SliceRandom;
        v.shuffle(&mut rand::thread_rng());

        #[cfg(feature = "misandry")]
        {
            v.retain(|v| v.gender == "FEMALE");
        }
        #[cfg(feature = "misogyny")]
        {
            v.retain(|v| v.gender == "MALE");
        }
        v
    };
    static ref SILLYVOICES: Vec<TTSVoice> = {
        let mut v = if cfg!(feature = "google-journey-tts") {
            vec![
                TTSVoice::new("en-US", "en-US-Journey-D", "MALE"),
                TTSVoice::new("en-US", "en-US-Journey-F", "FEMALE"),
            ]
        } else {
            vec![
                TTSVoice::new("fil-PH", "fil-ph-Neural2-D", "MALE"),
                TTSVoice::new("de-DE", "de-DE-Neural2-D", "MALE"),
                TTSVoice::new("ja-JP", "ja-JP-Neural2-D", "MALE"),
                TTSVoice::new("es-ES", "es-ES-Neural2-F", "MALE"),
                TTSVoice::new("ko-KR", "ko-KR-Neural2-B", "FEMALE"),
                TTSVoice::new("th-TH", "th-TH-Neural2-C", "FEMALE"),
                TTSVoice::new("vi-VN", "vi-VN-Neural2-A", "FEMALE"),
            ]
        };

        use rand::seq::SliceRandom;
        v.shuffle(&mut rand::thread_rng());

        #[cfg(feature = "misandry")]
        {
            v.retain(|v| v.gender == "FEMALE");
        }
        #[cfg(feature = "misogyny")]
        {
            v.retain(|v| v.gender == "MALE");
        }

        v
    };
);
pub async fn search(query: String, lim: usize) -> Vec<VideoInfo> {
    let url = format!("https://www.youtube.com/results?search_query={}", query);
    videos_from_raw_youtube_url(url, lim).await
}
async fn videos_from_raw_youtube_url(url: String, lim: usize) -> Vec<VideoInfo> {
    let res = crate::WEB_CLIENT.get(url.as_str()).header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/87.0.4280.88 Safari/537.36").send().await;
    let mut videos = Vec::new();
    if let Ok(res) = res {
        let text = res.text().await;
        if let Ok(text) = text {
            let original_id = if url.contains("watch?v=") {
                url.split("watch?v=").nth(1).map(|s| s.to_string())
            } else {
                None
            };
            videos = get_ids_from_html(text, lim, original_id).await;
        }
    }
    videos
}
async fn get_ids_from_html(
    text: String,
    lim: usize,
    original_id: Option<String>,
) -> Vec<VideoInfo> {
    let mut ids = text
        .split("/watch?v=")
        .skip(1)
        .map(|s| {
            let mut id = String::new();
            for c in s.chars() {
                if c == '>' || c == ' ' || c == '/' || c == '\\' || c == '"' || c == '&' {
                    break;
                }
                id.push(c);
            }
            id
        })
        .collect::<Vec<_>>();
    ids.dedup();
    if let Some(original_id) = original_id {
        ids.retain(|s| s != &original_id);
    }
    let mut h = Vec::new();
    for (i, s) in ids.iter().cloned().enumerate() {
        if i >= lim {
            break;
        }
        h.push(tokio::task::spawn(async move {
            let split = s.split('\\').next();
            if let Some(id) = split {
                let url = format!("https://www.youtube.com/watch?v={}", id);
                let vid = get_video_info(url).await;
                if let Ok(vid) = vid {
                    Some(vid)
                } else {
                    None
                }
            } else {
                None
            }
        }));
    }
    let mut videos = Vec::new();
    for t in h {
        if let Ok(Some(vid)) = t.await {
            videos.push(vid);
        }
    }
    videos
}
pub async fn get_recommendations(url: String, lim: usize) -> Vec<VideoInfo> {
    if url
        .split("https://www.youtube.com/watch?v=")
        .nth(1)
        .is_none()
    {
        return Vec::new();
    }
    videos_from_raw_youtube_url(url, lim).await
}
#[allow(dead_code)]
pub async fn get_video_info(url: String) -> Result<VideoInfo> {
    let info = get_url_video_info(&url).await?;
    Ok(VideoInfo {
        title: info.title.into(),
        url: url.into(),
        duration: info.duration,
    })
}
pub async fn get_url_video_info(url: &str) -> Result<RawVidInfo> {
    let dl = ytd_rs::YoutubeDL::new(
        &std::path::PathBuf::from("/dev/null"),
        vec![ytd_rs::Arg::new_with_arg("-O", "%(.{title,duration})#j")],
        url,
    )?;
    let info = dl.download()?;
    let output = info.output();
    Ok(serde_json::from_str(output).map_err(|e| {
        log::error!("{}", output);
        e
    })?)
}
#[derive(Debug, Clone, Deserialize)]
pub struct RawVidInfo {
    pub title: String,
    #[serde(default)]
    pub duration: Option<f64>,
}
#[cfg(feature = "spotify")]
pub async fn get_spotify_song_title(id: String) -> Result<Vec<String>> {
    let token = crate::Config::get().spotify_api_key;
    let url = format!("https://api.spotify.com/v1/tracks/{}", id);
    let res = crate::WEB_CLIENT
        .get(url.as_str())
        .header("Authorization", format!("Bearer {}", token.clone()))
        .send()
        .await?;
    let spoofydata = res.json::<RawSpotifyTrack>().await;
    if let Ok(spoofy) = spoofydata {
        Ok(vec![format!(
            "{} - {}",
            spoofy.name,
            spoofy
                .artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )])
    } else {
        let url = format!("https://api.spotify.com/v1/albums/{}", id);
        let res = crate::WEB_CLIENT
            .get(url.as_str())
            .header("Authorization", format!("Bearer {}", token.clone()))
            .send()
            .await?;
        let spoofydata = res.json::<RawSpotifyAlbum>().await;
        if let Ok(spoofy) = spoofydata {
            Ok(spoofy
                .tracks
                .items
                .iter()
                .map(|t| {
                    format!(
                        "{} - {}",
                        t.name,
                        t.artists
                            .iter()
                            .map(|a| a.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
                .collect())
        } else {
            log::info!("spoofydata: {:?}", spoofydata);
            Err(anyhow::anyhow!("Could not get spotify song title"))
        }
    }
}
#[derive(Debug, Deserialize, Serialize)]
pub struct RawSpotifyAlbum {
    tracks: RawSpotifyTracks,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct RawSpotifyTracks {
    items: Vec<RawSpotifyTrack>,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct RawSpotifyTrack {
    name: String,
    artists: Vec<RawSpotifyArtist>,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct RawSpotifyArtist {
    name: String,
}
#[derive(Debug, Clone)]
pub struct VideoInfo {
    title: Arc<str>,
    url: Arc<str>,
    duration: Option<f64>,
}
impl VideoInfo {
    pub fn new(title: Arc<str>, url: Arc<str>, duration: Option<f64>) -> Self {
        Self {
            title,
            url,
            duration,
        }
    }
    pub fn title(&self) -> Arc<str> {
        Arc::clone(&self.title)
    }
    pub fn url(&self) -> Arc<str> {
        Arc::clone(&self.url)
    }
    pub fn duration(&self) -> Option<f64> {
        self.duration
    }
    pub fn to_songbird(&self) -> songbird::input::Input {
        songbird::input::YoutubeDl::new(crate::WEB_CLIENT.clone(), self.url().to_string()).into()
    }
    pub async fn to_metavideo(&self) -> anyhow::Result<MetaVideo> {
        let v = crate::video::Video::get_video(&self.url, true, false)
            .await?
            .first()
            .ok_or(anyhow::anyhow!("Could not get video"))?
            .clone();
        #[cfg(feature = "tts")]
        let key = crate::youtube::get_access_token().await;
        let title = match &v {
            VideoType::Disk(v) => v.title(),
            VideoType::Url(v) => v.title(),
        };
        #[cfg(feature = "tts")]
        if let Ok(key) = key.as_ref() {
            Ok(MetaVideo {
                video: v,
                ttsmsg: Some(LazyLoadedVideo::new(tokio::spawn(crate::youtube::get_tts(
                    Arc::clone(&title),
                    key.clone(),
                    None,
                )))),
                // title,
                author: None,
            })
        } else {
            Ok(MetaVideo {
                video: v,
                ttsmsg: None,
                // title,
                author: None,
            })
        }
        #[cfg(not(feature = "tts"))]
        return Ok(MetaVideo { video: v, title });
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, Copy)]
pub struct TTSVoice {
    pub language_code: &'static str,
    pub name: &'static str,
    pub gender: &'static str,
}
impl Default for TTSVoice {
    fn default() -> Self {
        Self::new("en-US", "en-US-Wavenet-C", "FEMALE")
    }
}
impl TTSVoice {
    pub fn new(language_code: &'static str, name: &'static str, gender: &'static str) -> Self {
        Self {
            language_code,
            name,
            gender,
        }
    }
}
#[cfg(feature = "tts")]
pub async fn get_tts<F, T>(title: F, key: T, specificvoice: Option<TTSVoice>) -> Result<Video>
where
    F: AsRef<str>,
    T: AsRef<str>,
{
    let mut title = title.as_ref().to_owned();
    use rand::seq::SliceRandom;
    let backup_voice = VOICES
        .choose(&mut rand::thread_rng())
        .copied()
        .ok_or(anyhow::anyhow!("Could not get voice"));
    let voice = match specificvoice {
        Some(v) => v,
        None => backup_voice?,
    };
    if specificvoice.is_none() {
        title = format!("Now playing... {}", title);
    }
    let body = serde_json::json!(
        {
            "input":{
                "text": title,
            },
            "voice":{
                "languageCode": voice.language_code,
                "name": voice.name,
                "ssmlGender": voice.gender
            },
            "audioConfig":{
                "audioEncoding":"OGG_OPUS"
            }
        }
    );
    let res = crate::WEB_CLIENT
        .post("https://texttospeech.googleapis.com/v1/text:synthesize")
        .header("Content-Type", "application/json; charset=utf-8")
        .header("X-Goog-User-Project", "97417849124")
        .header("Authorization", format!("Bearer {}", key.as_ref().trim()))
        .body(body.to_string())
        .send()
        .await?;
    let mut json: TTSResponse = res.json().await?;
    {
        let mut string = String::with_capacity(json.audio_content.len());
        json.audio_content
            .trim_end_matches('=')
            .clone_into(&mut string);
        std::mem::swap(&mut json.audio_content, &mut string);
    }
    let data = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        json.audio_content,
    )?;
    let id = nanoid::nanoid!(10);
    let mut path = crate::Config::get().data_path;
    path.push("tmp");
    path.push(format!("GTTS{}_NA.ogg", id));
    let mut file = tokio::fs::File::create(path.clone()).await?;
    file.write_all(data.as_ref()).await?;
    Video::from_path(
        path,
        "GTTS".to_owned(),
        crate::video::MediaType::Audio,
        "GTTS".to_owned(),
    )
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTSResponse {
    #[serde(rename = "audioContent")]
    audio_content: String,
}
#[cfg(feature = "tts")]
pub async fn get_access_token() -> Result<String> {
    #[cfg(target_family = "windows")]
    match powershell_script::PsScriptBuilder::new()
        .non_interactive(true)
        .hidden(true)
        .build()
        .run(crate::Config::get().gcloud_script.as_str())
    {
        Ok(token) => {
            let t = format!("{}", token).trim().to_string();
            if t.contains(' ') {
                Err(anyhow::anyhow!(t))
            } else {
                Ok(t)
            }
        }
        Err(e) => Err(anyhow::anyhow!(e)),
    }
    #[cfg(target_family = "unix")]
    {
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(crate::Config::get().gcloud_script.as_str())
            .output()
            .await?;
        let t = String::from_utf8(output.stdout)? + &String::from_utf8(output.stderr)?;
        if t.contains(' ') {
            log::error!("{}", t);
            Err(anyhow::anyhow!(t))
        } else {
            Ok(t)
        }
    }
}
#[cfg(feature = "youtube-search")]
pub async fn youtube_search(url: &str, lim: u64) -> Result<Vec<YoutubeMedia>> {
    let url = format!(
        "https://www.youtube.com/results?search_query={}",
        urlencoding::encode(url)
    );
    let lim = lim.to_string();
    let mut bot_path = crate::Config::get().data_path.clone();
    bot_path.push("cookies.txt");
    let output = if bot_path.exists() {
        tokio::process::Command::new("yt-dlp")
            .args([
                "--cookies",
                match bot_path.to_str() {
                    Some(s) => s,
                    None => return Err(anyhow::anyhow!("Could not get cookies path")),
                },
            ])
            .args(["-O", "%(.{webpage_url,title,duration,uploader})j"])
            .arg("--flat-playlist")
            .args(["--playlist-end", lim.as_str()])
            .arg("--force-ipv4")
            .arg(url)
            .output()
            .await?
    } else {
        tokio::process::Command::new("yt-dlp")
            .args(["-O", "%(.{webpage_url,title,duration,uploader})j"])
            .arg("--flat-playlist")
            .args(["--playlist-end", lim.as_str()])
            .arg("--force-ipv4")
            .arg(url)
            .output()
            .await?
    };
    let output = String::from_utf8(output.stdout)?;
    Ok(output
        .split('\n')
        .flat_map(|line| match serde_json::from_str::<YoutubeMedia>(line) {
            Ok(v) => Some(v),
            Err(e) => {
                if !line.trim().is_empty() {
                    log::error!("Error: {}", e);
                }
                None
            }
        })
        .collect::<Vec<YoutubeMedia>>())
}
#[derive(Deserialize, Debug)]
pub struct YoutubeMedia {
    #[serde(rename = "webpage_url")]
    pub url: String,
    pub title: String,
    pub duration: Option<f64>,
    pub uploader: Option<String>,
}
impl YoutubeMedia {
    pub fn to_raw(&self) -> RawVideo {
        RawVideo {
            url: self.url.clone(),
            title: self.title.clone(),
            duration: self.duration,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YTSearchResultMeta {
    pub items: Vec<YTSearchResult>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YTSearchResult {
    pub id: YTSearchID,
    pub snippet: YTSearchSnippet,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YTSearchID {
    #[serde(rename = "videoId")]
    pub video_id: Option<String>,
    #[serde(rename = "playlistId")]
    pub playlist_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YTSearchSnippet {
    pub title: String,
}
