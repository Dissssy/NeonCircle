use anyhow::Error;
use serde::{Deserialize, Serialize};
#[cfg(not(feature = "download"))]
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::{commands::music::VideoType, video::Video};
#[allow(dead_code)]
pub async fn search(query: String) -> Vec<VideoInfo> {
    let url = format!("https://www.youtube.com/results?search_query={}", query);
    let client = reqwest::Client::new();
    let res = client.get(url.as_str()).send().await;
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
    if let Some(title) = title {
        Ok(VideoInfo { title, url })
    } else {
        Err(anyhow::anyhow!("Could not get video info"))
    }
}

pub async fn get_url_title(url: String) -> Option<String> {
    let client = reqwest::Client::new();
    let res = client.get(url.as_str()).send().await;
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

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub title: String,

    pub url: String,
}

pub async fn get_tts(title: String, key: String) -> Result<VideoType, Error> {
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

pub async fn youtube_search(query: String) -> Result<Vec<VideoInfo>, Error> {
    let client = reqwest::Client::new();
    let res = client
        .get("https://www.googleapis.com/youtube/v3/search")
        .header("Content-Type", "application/json; charset=utf-8")
        .query(&[
            ("key", crate::Config::get().youtube_api_key.as_str()),
            ("part", "snippet"),
            ("type", "video"),
            ("q", query.as_str()),
        ])
        .send()
        .await?;
    // println!("res: {:?}", res.json().await?);
    // write res.text().await? to youtube.json

    let r: YTSearchResultMeta = res.json().await?;
    let mut videos = Vec::new();
    for item in r.items {
        let video = VideoInfo {
            title: item.snippet.title,
            url: format!("https://www.youtube.com/watch?v={}", item.id.video_id),
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
    pub video_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YTSearchSnippet {
    pub title: String,
}
