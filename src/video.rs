#![allow(dead_code)]
#[cfg(feature = "spotify")]
use crate::youtube::get_spotify_song_title;
#[cfg(feature = "youtube-search")]
use crate::youtube::youtube_search;
use crate::{commands::music::VideoType, youtube::VideoInfo};
use anyhow::Result;
use serde::Deserialize;
use serenity::async_trait;
use std::path::PathBuf;
use ytd_rs::Arg;
#[derive(Debug, Clone)]
pub struct Video {
    pub url: String,
    pub path: PathBuf,
    pub title: String,
    pub duration: f64,
    pub media_type: MediaType,
    pub playlist_index: usize,
}
#[derive(Deserialize, Debug)]
pub struct RawVideo {
    #[serde(rename = "webpage_url")]
    pub url: String,
    pub title: String,
    pub duration: u32,
}
async fn get_videos(url: &str, allow_search: bool) -> Result<Vec<RawVideo>> {
    let mut bot_path = crate::Config::get().data_path.clone();
    bot_path.push("cookies.txt");
    let url = if !(url.starts_with("http://") || url.starts_with("https://")) {
        if !allow_search {
            return Err(anyhow::anyhow!("Invalid URL found"));
        }
        let vids = crate::youtube::youtube_search(url, 1).await?;
        if vids.is_empty() {
            return Err(anyhow::anyhow!("No videos found"));
        }
        match vids.first().map(|v| (v.to_raw(), v)) {
            Some((Some(v), _)) => return Ok(vec![v]),
            Some((None, v)) => v.url.to_string(),
            None => return Err(anyhow::anyhow!("No videos found")),
        }
    } else {
        url.to_string()
    };
    println!("URL: {}", url);
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(anyhow::anyhow!("Invalid URL found after search query"));
    }
    let output = if bot_path.exists() {
        tokio::process::Command::new("yt-dlp")
            .args(["--cookies", bot_path.to_str().expect("No path")])
            .arg("--flat-playlist")
            .arg("--dump-json")
            .arg("--force-ipv4")
            .arg(url)
            .output()
            .await?
    } else {
        tokio::process::Command::new("yt-dlp")
            .arg("--flat-playlist")
            .arg("--dump-json")
            .arg("--force-ipv4")
            .arg(url)
            .output()
            .await?
    };
    let output = String::from_utf8(output.stdout)?;
    let vids = output
        .split('\n')
        .flat_map(|line| match serde_json::from_str::<RawVideo>(line) {
            Ok(v) => Some(v),
            Err(e) => {
                if !line.trim().is_empty() {
                    println!("Error: {}", e);
                }
                None
            }
        })
        .collect::<Vec<RawVideo>>();
    Ok(vids)
}
impl Video {
    pub fn to_songbird(&self) -> songbird::input::Input {
        songbird::input::File::new(self.path.clone()).into()
    }
    pub async fn get_video(
        url: &str,
        allow_playlist: bool,
        allow_search: bool,
    ) -> Result<Vec<VideoType>> {
        let now = std::time::Instant::now();
        let mut v = get_videos(url, allow_search).await?;
        println!("Took {}ms to get videos", now.elapsed().as_millis());
        if v.is_empty() {
            return Err(anyhow::anyhow!("No videos found"));
        }
        if !allow_playlist {
            v = vec![v.remove(0)];
        }
        Ok(v.iter()
            .map(|v| {
                VideoType::Url(VideoInfo {
                    title: v.title.clone(),
                    url: v.url.clone(),
                    duration: Some(v.duration as u64),
                })
            })
            .collect::<Vec<VideoType>>())
    }
    pub async fn download_video(
        url: &str,
        media_type: MediaType,
        spoiler: bool,
        max_filesize: &str,
    ) -> Result<VideoType> {
        let v = Self::get_video(url, false, false).await?;
        let v = v.first().ok_or(anyhow::anyhow!("No videos found"))?;
        match v {
            VideoType::Disk(_) => Err(anyhow::anyhow!("Video already downloaded")),
            VideoType::Url(_) => {
                let id = format!(
                    "{}{}",
                    if spoiler { "SPOILER_" } else { "" },
                    nanoid::nanoid!(10)
                );
                let mut path = crate::Config::get().data_path.clone();
                path.push("tmp");
                std::fs::create_dir_all(&path)?;
                let mut args = vec![
                    Arg::new("--no-playlist"),
                    Arg::new("--quiet"),
                    Arg::new_with_arg(
                        "--output",
                        format!("{}_%(playlist_index)s.%(ext)s", id).as_str(),
                    ),
                    Arg::new("--embed-metadata"),
                ];
                let mut bot_path = crate::Config::get().data_path.clone();
                bot_path.push("cookies.txt");
                if bot_path.exists() {
                    args.push(Arg::new_with_arg(
                        "--cookies",
                        bot_path.to_str().expect("No path"),
                    ));
                }
                match media_type {
                    MediaType::Audio => {
                        args.push(Arg::new("-x"));
                        args.push(Arg::new_with_arg("--audio-format", "mp3"));
                    }
                    MediaType::Video => {
                        args.push(Arg::new_with_arg("-S", "res,ext:mp4:m4a"));
                        args.push(Arg::new_with_arg("--recode", "mp4"));
                    }
                }
                let ytd = ytd_rs::YoutubeDL::new(&path, args.clone(), url)?;
                let response = match tokio::task::spawn_blocking(move || ytd.download()).await? {
                    Ok(r) => r,
                    Err(_) => match media_type {
                        MediaType::Audio => {
                            args.retain(|a| {
                                a.to_string()
                                    != Arg::new_with_arg(
                                        "-f",
                                        format!("best[filesize<={}]", max_filesize).as_str(),
                                    )
                                    .to_string()
                            });
                            let ytd = ytd_rs::YoutubeDL::new(&path, args, url)?;
                            tokio::task::spawn_blocking(move || ytd.download()).await??
                        }
                        MediaType::Video => {
                            return Err(anyhow::anyhow!("Failed to download video"))
                        }
                    },
                };
                let file = response.output_dir();
                let mut videos = Vec::new();
                for entry in std::fs::read_dir(file)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.is_file() {
                        let file_name = path
                            .file_name()
                            .ok_or(anyhow::anyhow!("No Path"))?
                            .to_str()
                            .ok_or(anyhow::anyhow!("No Path"))?;
                        if file_name.starts_with(id.as_str()) {
                            run_preprocessor(&path).await?;
                            videos.push(Self::from_path(
                                path,
                                url.to_owned(),
                                media_type,
                                id.clone(),
                            )?);
                        }
                    }
                }
                if videos.is_empty() {
                    Err(anyhow::anyhow!("No videos found"))
                } else {
                    videos.sort_by(|a, b| a.playlist_index.cmp(&b.playlist_index));
                    Ok(videos
                        .iter()
                        .map(|v| VideoType::Disk(v.clone()))
                        .collect::<Vec<VideoType>>()
                        .first()
                        .cloned()
                        .ok_or(anyhow::anyhow!("No videos found"))?)
                }
            }
        }
    }
    pub fn delete(&self) -> Result<()> {
        std::fs::remove_file(self.path.clone())?;
        Ok(())
    }
    pub fn from_path(
        path: PathBuf,
        url: String,
        media_type: MediaType,
        id: String,
    ) -> Result<Self> {
        let file_name = match path.file_name().and_then(|f| f.to_str()) {
            Some(f) => f,
            None => return Err(anyhow::anyhow!("No file name")),
        };
        let tag = audiotags::Tag::new().read_from_path(&path);
        let title = if let Ok(tag) = tag.as_ref() {
            tag.title().unwrap_or(&id)
        } else {
            &id
        };
        let s = ffprobe::ffprobe(&path)?;
        let duration = s.streams[0]
            .duration
            .as_ref()
            .and_then(|d| d.parse::<f64>().ok())
            .unwrap_or(0.0);
        let playlist_index = file_name
            .split('_')
            .nth(1)
            .and_then(|s| s.split('.').next().and_then(|s| s.parse::<usize>().ok()))
            .unwrap_or(0);
        Ok(Self {
            url,
            path: path.clone(),
            title: title.to_string(),
            duration,
            media_type,
            playlist_index,
        })
    }
    pub async fn delete_when_finished(self, handle: songbird::tracks::TrackHandle) -> Result<()> {
        handle.add_event(
            songbird::events::Event::Track(songbird::events::TrackEvent::End),
            self,
        )?;
        Ok(())
    }
}
#[async_trait]
impl songbird::EventHandler for Video {
    async fn act(&self, ctx: &songbird::EventContext<'_>) -> Option<songbird::Event> {
        if let songbird::EventContext::Track(track) = ctx {
            for (state, _handle) in *track {
                if state.playing.is_done() {
                    if let Err(e) = self.delete() {
                        println!("Failed to delete file: {}", e);
                    }
                }
            }
        }
        None
    }
}
#[cfg(feature = "spotify")]
pub async fn get_spotify_shiz(url: String) -> Result<Vec<VideoType>> {
    let id = url
        .split('/')
        .last()
        .ok_or_else(|| anyhow::anyhow!("Invalid spotify URL"))?
        .split('?')
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid spotify URL"))?
        .to_string();
    let videos = get_spotify_song_title(id).await?;
    if videos.is_empty() {
        Err(anyhow::anyhow!("No videos found"))
    } else {
        let mut vids = Vec::new();
        for video in videos {
            let vid = youtube_search(&video, 1).await?;
            if vid.is_empty() {
                continue;
            } else {
                vids.push(
                    Video::get_video(&vid[0].url, false, true)
                        .await?
                        .first()
                        .ok_or_else(|| anyhow::anyhow!("No videos found"))?
                        .clone(),
                );
            }
        }
        Ok(vids)
    }
}
async fn run_preprocessor(filepath: &PathBuf) -> Result<()> {
    let mut path = crate::Config::get().data_path.clone();
    path.push("preprocessor.sh");
    if path.exists() {
        let mut cmd = tokio::process::Command::new(path);
        cmd.arg(filepath);
        cmd.spawn()?.wait().await?;
    }
    Ok(())
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Audio,
    Video,
}
