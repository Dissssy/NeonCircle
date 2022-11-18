#[cfg(feature = "tts")]
use crate::{commands::music::VideoType, video::Video};
use anyhow::Error;
use serde::{Deserialize, Serialize};
#[cfg(feature = "tts")]
use tokio::io::AsyncWriteExt;

#[allow(dead_code)]
pub async fn search(query: String) -> Vec<VideoInfo> {
    let url = format!("https://www.youtube.com/results?search_query={}", query);
    let client = reqwest::Client::new();
    let res = client.get(url.as_str()).header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/87.0.4280.88 Safari/537.36",
        ).send().await;
    let mut videos = Vec::new();
    if let Ok(res) = res {
        let text = res.text().await;

        if let Ok(text) = text {
            let split = text.split("{\"url\":\"/watch?v=");
            let mut h = Vec::new();
            for (i, s) in split.map(|s| s.to_owned()).enumerate() {
                if i > 1 {
                    break;
                }
                h.push(tokio::task::spawn(async move {
                    let split = s.split('\"').next();

                    if let Some(id) = split {
                        if !id
                            .chars()
                            .any(|c| c == '>' || c == ' ' || c == '/' || c == '\\')
                        {
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
                    } else {
                        None
                    }
                }));
            }
            for t in h {
                if let Ok(Some(vid)) = t.await {
                    videos.push(vid);
                }
            }
        }
    }

    videos
}

#[allow(dead_code)]
pub async fn get_video_info(url: String) -> Result<VideoInfo, Error> {
    let title = get_url_title(url.clone()).await;
    println!("title: {:?}", title);
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
    println!("url: {}", url);
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
        println!("spoofydata: {:?}", spoofydata);
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

#[cfg(feature = "tts")]
pub async fn get_tts(title: String, key: String) -> Result<VideoType, Error> {
    // return Err(anyhow::anyhow!("TTS is currently disabled"));
    // println!("key: {}", key);
    let body = serde_json::json!(
        {
            "input":{
                "text": format!("Now playing... {}", title)
            },
            "voice":{
                "languageCode":"en-us",
                "name":"en-US-Wavenet-C",
                "ssmlGender":"FEMALE"
            },
            "audioConfig":{
                "audioEncoding":"MP3"
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
    // println!("res: {:?}", res);
    // let res = res?;
    // let text = res.text().await?;
    // println!("{}", text);
    // let json: TTSResponse = serde_json::from_str(text.as_str())?;
    let json: TTSResponse = res.json().await?;

    let data = base64::decode(json.audio_content)?;

    let id = nanoid::nanoid!(10);
    let mut path = crate::Config::get().data_path;
    path.push("tmp");
    path.push(format!("GTTS{}_NA.mp3", id));
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
