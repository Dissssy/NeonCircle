#![allow(dead_code)]
#[cfg(feature = "spotify")]
use crate::youtube::get_spotify_song_title;
#[cfg(feature = "youtube-search")]
use crate::youtube::youtube_search;
use crate::{commands::music::VideoType, youtube::VideoInfo};
use anyhow::Result;
use serde::Deserialize;
use serenity::async_trait;
use songbird::{input::File, tracks::Track};
use std::{path::PathBuf, sync::Arc};
use ytd_rs::Arg;
#[derive(Debug, Clone)]
pub struct Video {
    inner: Arc<InnerVideo>,
}
#[derive(Debug)]
struct InnerVideo {
    pub url: Arc<str>,
    pub path: PathBuf,
    pub title: Arc<str>,
    pub duration: f64,
    pub media_type: MediaType,
    pub playlist_index: usize,
}
impl Drop for InnerVideo {
    fn drop(&mut self) {
        log::trace!("Dropping video: {}", self.title);
        if let Err(e) = std::fs::remove_file(&self.path) {
            log::error!("Failed to delete video: {}", e);
        }
    }
}
#[derive(Deserialize, Debug)]
pub struct RawVideo {
    #[serde(rename = "webpage_url")]
    pub url: String,
    pub title: String,
    pub duration: Option<f64>,
}
async fn get_videos(url: &str, allow_search: bool) -> Result<Vec<RawVideo>> {
    let mut bot_path = crate::config::get_config().data_path.clone();
    bot_path.push("cookies.txt");
    let url = if !(url.starts_with("http://") || url.starts_with("https://")) {
        if !allow_search {
            return Err(anyhow::anyhow!("Invalid URL found"));
        }
        let vids = crate::youtube::youtube_search(url, 1).await?;
        if vids.is_empty() {
            return Err(anyhow::anyhow!("No videos found"));
        }
        vids.first()
            .ok_or(anyhow::anyhow!("No videos found"))?
            .url
            .clone()
    } else {
        url.to_string()
    };
    log::trace!("URL: {}", url);
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(anyhow::anyhow!("Invalid URL found after search query"));
    }
    let output = if bot_path.exists() {
        tokio::process::Command::new("yt-dlp")
            .args([
                "--cookies",
                match bot_path.to_str() {
                    Some(p) => p,
                    None => return Err(anyhow::anyhow!("No path")),
                },
            ])
            .arg("--flat-playlist")
            .args(["-O", "%(.{webpage_url,title,duration,uploader})j"])
            .arg("--force-ipv4")
            .arg(url)
            .output()
            .await?
    } else {
        tokio::process::Command::new("yt-dlp")
            .arg("--flat-playlist")
            .args(["-O", "%(.{webpage_url,title,duration,uploader})j"])
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
                    log::error!("Failed to parse line: {}\nError: {}", line, e);
                }
                None
            }
        })
        .collect::<Vec<RawVideo>>();
    Ok(vids)
}
impl Video {
    pub fn url(&self) -> Arc<str> {
        Arc::clone(&self.inner.url)
    }
    pub fn path(&self) -> PathBuf {
        self.inner.path.clone()
    }
    pub fn title(&self) -> Arc<str> {
        Arc::clone(&self.inner.title)
    }
    pub fn duration(&self) -> f64 {
        self.inner.duration
    }
    pub fn media_type(&self) -> MediaType {
        self.inner.media_type
    }
    pub fn playlist_index(&self) -> usize {
        self.inner.playlist_index
    }
    pub fn to_songbird(&self) -> Track {
        Track::new(File::new(self.path()).into())
    }
    pub async fn get_video(
        url: &str,
        allow_playlist: bool,
        allow_search: bool,
    ) -> Result<Vec<VideoType>> {
        let now = std::time::Instant::now();
        let mut v = get_videos(url, allow_search).await?;
        log::info!("Took {}ms to get videos", now.elapsed().as_millis());
        if v.is_empty() {
            return Err(anyhow::anyhow!("No videos found"));
        }
        if !allow_playlist {
            v = vec![v.remove(0)];
        }
        Ok(v.iter()
            .map(|v| {
                VideoType::Url(VideoInfo::new(
                    v.title.clone().into(),
                    v.url.clone().into(),
                    v.duration,
                ))
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
                let mut path = crate::config::get_config().data_path.clone();
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
                let mut bot_path = crate::config::get_config().data_path.clone();
                bot_path.push("cookies.txt");
                if bot_path.exists() {
                    args.push(Arg::new_with_arg(
                        "--cookies",
                        match bot_path.to_str() {
                            Some(p) => p,
                            None => return Err(anyhow::anyhow!("No path")),
                        },
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
                    videos.sort_by_key(|a| a.playlist_index());
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
        let duration = s
            .streams
            .first()
            .and_then(|s| s.duration.as_ref())
            .and_then(|d| d.parse::<f64>().ok())
            .unwrap_or(0.0);
        let playlist_index = file_name
            .split('_')
            .nth(1)
            .and_then(|s| s.split('.').next().and_then(|s| s.parse::<usize>().ok()))
            .unwrap_or(0);
        Ok(Self {
            inner: Arc::new(InnerVideo {
                url: url.into(),
                path,
                title: title.into(),
                duration,
                media_type,
                playlist_index,
            }),
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
                    return Some(songbird::Event::Track(songbird::TrackEvent::End));
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
            if let Some(vid) = vid.first() {
                vids.push(
                    Video::get_video(&vid.url, false, true)
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
    let mut path = crate::config::get_config().data_path.clone();
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
