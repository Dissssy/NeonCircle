#![allow(dead_code)]
use crate::commands::music::MetaVideo;
#[cfg(feature = "tts")]
use crate::{commands::music::VideoType, video::Video};
use anyhow::Error;
use serde::{Deserialize, Serialize};
#[cfg(feature = "tts")]
use tokio::io::AsyncWriteExt;

lazy_static::lazy_static!(
    pub static ref VOICES: Vec<TTSVoice> = {
        let v = vec![
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
        ];
        // shuffle
        let mut v = v;
        use rand::seq::SliceRandom;
        v.shuffle(&mut rand::thread_rng());
        v
    };
    static ref SILLYVOICES: Vec<TTSVoice> = {
        let v = vec![
            TTSVoice::new("fil-PH", "fil-ph-Neural2-D", "MALE"),
            TTSVoice::new("de-DE", "de-DE-Neural2-D", "MALE"),
            TTSVoice::new("ja-JP", "ja-JP-Neural2-D", "MALE"),
            TTSVoice::new("es-ES", "es-ES-Neural2-F", "MALE"),
            TTSVoice::new("ko-KR", "ko-KR-Neural2-B", "FEMALE"),
            TTSVoice::new("th-TH", "th-TH-Neural2-C", "FEMALE"),
            TTSVoice::new("vi-VN", "vi-VN-Neural2-A", "FEMALE"),
        ];
        v
    };
);

pub async fn search(query: String, lim: usize) -> Vec<VideoInfo> {
    let url = format!("https://www.youtube.com/results?search_query={}", query);
    videos_from_raw_youtube_url(url, lim).await
}

async fn videos_from_raw_youtube_url(url: String, lim: usize) -> Vec<VideoInfo> {
    let client = reqwest::Client::new();
    let res = client
        .get(url.as_str())
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/87.0.4280.88 Safari/537.36",
        )
        .send()
        .await;
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
            // we want to get the video id, so split on the next invalid character
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
pub async fn get_video_info(url: String) -> Result<VideoInfo, Error> {
    let title = get_url_title(url.clone()).await;
    if let Some(title) = title {
        Ok(VideoInfo { title, url })
    } else {
        Err(anyhow::anyhow!("Could not get video info"))
    }
}

pub async fn get_url_title(url: String) -> Option<String> {
    let client = reqwest::Client::new();
    let res = client.get(url.as_str()).header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/87.0.4280.88 Safari/537.36",
        ).send().await;
    if let Ok(res) = res {
        let text = res.text().await;
        if let Ok(text) = text {
            let title = text.split("<title>").nth(1);
            if let Some(title) = title {
                let title = title.split("</title>").next();
                title.map(|title| title.to_owned())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(feature = "spotify")]
pub async fn get_spotify_song_title(id: String) -> Result<Vec<String>, Error> {
    // get the song title from spotify api
    let token = crate::Config::get().spotify_api_key;
    let url = format!("https://api.spotify.com/v1/tracks/{}", id);
    let client = reqwest::Client::new();
    let res = client
        .get(url.as_str())
        .header("Authorization", format!("Bearer {}", token.clone()))
        .send()
        .await?;
    let spoofydata = res.json::<RawSpotifyTrack>().await;
    if let Ok(spoofy) = spoofydata {
        Ok(vec![format!(
            "{} - {}",
            spoofy.name, spoofy.artists[0].name
        )])
    } else {
        // attempt to get the album
        let url = format!("https://api.spotify.com/v1/albums/{}", id);
        let client = reqwest::Client::new();
        let res = client
            .get(url.as_str())
            .header("Authorization", format!("Bearer {}", token.clone()))
            .send()
            .await?;
        // println!("res: {:?}", res.text().await);
        // return Ok(Vec::new());
        let spoofydata = res.json::<RawSpotifyAlbum>().await;
        if let Ok(spoofy) = spoofydata {
            Ok(spoofy
                .tracks
                .items
                .iter()
                .map(|t| format!("{} - {}", t.name, t.artists[0].name))
                .collect())
        } else {
            println!("spoofydata: {:?}", spoofydata);
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
    pub title: String,
    pub url: String,
}

impl VideoInfo {
    pub async fn to_metavideo(&self) -> anyhow::Result<MetaVideo> {
        let v = crate::video::Video::get_video(self.url.clone(), true)
            .await?
            .get(0)
            .ok_or(anyhow::anyhow!("Could not get video"))?
            .clone();

        #[cfg(feature = "tts")]
        let key = crate::youtube::get_access_token().await;

        let title = match v.clone() {
            VideoType::Disk(v) => v.title,
            VideoType::Url(v) => v.title,
        };
        #[cfg(feature = "tts")]
        if let Ok(key) = key.as_ref() {
            let t = tokio::task::spawn(crate::youtube::get_tts(title.clone(), key.clone(), None))
                .await
                .unwrap();
            if let Ok(tts) = t {
                match tts {
                    VideoType::Disk(tts) => Ok(MetaVideo {
                        video: v,
                        ttsmsg: Some(tts),
                        title,
                    }),
                    VideoType::Url(_) => {
                        unreachable!("TTS should always be a disk file");
                    }
                }
            } else {
                println!("Error {:?}", t);
                Ok(MetaVideo {
                    video: v,
                    ttsmsg: None,
                    title,
                })
            }
        } else {
            Ok(MetaVideo {
                video: v,
                ttsmsg: None,
                title,
            })
        }
        #[cfg(not(feature = "tts"))]
        return Ok(MetaVideo { video: v, title });
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTSVoice {
    pub language_code: String,
    pub name: String,
    pub gender: String,
}

impl TTSVoice {
    pub fn new(language_code: impl ToString, name: impl ToString, gender: impl ToString) -> Self {
        Self {
            language_code: language_code.to_string(),
            name: name.to_string(),
            gender: gender.to_string(),
        }
    }
}

#[cfg(feature = "tts")]
pub async fn get_tts(
    title: String,
    key: String,
    specificvoice: Option<TTSVoice>,
) -> Result<VideoType, Error> {
    let mut title = title;
    // return Err(anyhow::anyhow!("TTS is currently disabled"));
    // println!("key: {}", key);

    use rand::seq::SliceRandom;

    // let voice = {
    //     let fallback = TTSVoice::new("en-US", "en-US-Wavenet-C", "FEMALE");

    //     if let Some(i) = specificvoice {
    //         let mut i = i;
    //         if i >= VOICES.len() {
    //             i %= VOICES.len();
    //         }
    //         VOICES.get(i).unwrap_or(&fallback).clone()
    //     } else {
    //         VOICES
    //             .choose(&mut rand::thread_rng())
    //             .unwrap_or(&fallback)
    //             .clone()
    //     }
    // };
    let voice = specificvoice
        .clone()
        .unwrap_or_else(|| SILLYVOICES.choose(&mut rand::thread_rng()).unwrap().clone());

    // body["voice"] = serde_json::json!(
    //     {
    //         "languageCode":"en-us",
    //         "name":"en-US-Wavenet-C",
    //         "ssmlGender":"FEMALE"
    //     }
    // );

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

    let client = reqwest::Client::new();
    let res = client
        .post("https://texttospeech.googleapis.com/v1/text:synthesize")
        .header("Content-Type", "application/json; charset=utf-8")
        .header("X-Goog-User-Project", "97417849124")
        .header("Authorization", format!("Bearer {}", key.trim()))
        .body(body.to_string())
        .send()
        .await?;

    // let res = res?;
    // let text = res.text().await?;
    // println!("{}", text);
    // let mut json: TTSResponse = serde_json::from_str(text.as_str())?;
    let mut json: TTSResponse = res.json().await?;

    // we're using the no_pad decoder so we need to remove the padding google adds
    json.audio_content = json.audio_content.trim_end_matches('=').to_owned();

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

    Ok(VideoType::Disk(Video::from_path(
        path,
        "GTTS".to_owned(),
        true,
        "GTTS".to_owned(),
    )?))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTSResponse {
    #[serde(rename = "audioContent")]
    audio_content: String,
}

#[cfg(feature = "tts")]
pub async fn get_access_token() -> Result<String, Error> {
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
            println!("{}", t);
            Err(anyhow::anyhow!(t))
        } else {
            Ok(t)
        }
    }
}

#[cfg(feature = "youtube-search")]
pub async fn youtube_search(query: String) -> Result<Vec<VideoInfo>, Error> {
    let client = reqwest::Client::new();
    let res = client
        .get("https://www.googleapis.com/youtube/v3/search")
        .header("Content-Type", "application/json; charset=utf-8")
        .query(&[
            ("key", crate::Config::get().youtube_api_key.as_str()),
            ("part", "snippet"),
            // ("type", "video"),
            ("q", query.as_str()),
        ])
        .send()
        .await?;
    // println!("res: {:?}", res.json().await?);
    // write res.text().await? to youtube.json
    // tokio::fs::write("youtube.json", res.text().await?).await?;
    // Ok(vec![])
    let r: YTSearchResultMeta = res.json().await?;
    let mut videos = Vec::new();
    for item in r.items {
        let video = if let Some(id) = item.id.video_id {
            VideoInfo {
                title: item.snippet.title,
                url: format!("https://www.youtube.com/watch?v={}", id),
            }
        } else if let Some(id) = item.id.playlist_id {
            VideoInfo {
                title: format!("{} (playlist)", item.snippet.title),
                url: format!("https://www.youtube.com/playlist?list={}", id),
            }
        } else {
            continue;
        };
        videos.push(video);
    }
    Ok(videos)
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
