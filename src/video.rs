use std::path::PathBuf;

use anyhow::Error;
use async_recursion::async_recursion;
use ytd_rs::Arg;

use crate::commands::music::VideoType;
#[cfg(feature = "spotify")]
use crate::youtube::get_spotify_song_title;
#[cfg(feature = "youtube-search")]
use crate::youtube::youtube_search;

#[derive(Debug, Clone)]
pub struct Video {
    pub url: String,
    pub path: PathBuf,
    pub title: String,
    pub duration: f64,
    pub video: bool,
    pub playlist_index: usize,
}

impl Video {
    #[async_recursion]
    pub async fn get_video(
        url: String,
        audio_only: bool,
        allow_playlist: bool,
    ) -> Result<Vec<VideoType>, anyhow::Error> {
        // if url is empty
        if url.is_empty() {
            return Self::get_video(crate::Config::get().bumper_url, audio_only, allow_playlist)
                .await;
        }
        // if url is spotify
        #[cfg(feature = "spotify")]
        if url.contains("spotify.com") {
            // get the title for the page
            return get_spotify_shiz(url).await;
        }

        #[cfg(not(feature = "download"))]
        if allow_playlist {
            let vid = crate::youtube::get_video_info(url.clone()).await;
            if let Ok(vid) = vid {
                return Ok(vec![VideoType::Url(vid)]);
            } else {
                return Err(anyhow::anyhow!("Could not get video info"));
            }
        }
        let id = nanoid::nanoid!(10);
        let mut path = crate::Config::get().data_path.clone();
        path.push("tmp");
        std::fs::create_dir_all(&path)?;
        let mut args = vec![
            Arg::new("--quiet"),
            Arg::new_with_arg(
                "--output",
                format!("{}_%(playlist_index)s.%(ext)s", id).as_str(),
            ),
            Arg::new("--embed-metadata"),
        ];
        if audio_only {
            args.push(Arg::new("-x"));
            args.push(Arg::new_with_arg("--audio-format", "mp3"));
        } else {
            args.push(Arg::new_with_arg("-S", "res,ext:mp4:m4a"));
            args.push(Arg::new_with_arg("--recode", "mp4"));
        }
        if !allow_playlist {
            args.push(Arg::new("--no-playlist"));
        }
        let ytd = ytd_rs::YoutubeDL::new(&path, args, url.as_str())?;
        let response = tokio::task::spawn_blocking(move || ytd.download()).await??;

        let file = response.output_dir();

        let mut videos = Vec::new();
        for entry in std::fs::read_dir(file)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let file_name = path.file_name().unwrap().to_str().unwrap();
                if file_name.starts_with(id.as_str()) {
                    videos.push(Self::from_path(path, url.clone(), audio_only, id.clone())?);
                }
            }
        }
        if videos.is_empty() {
            Err(anyhow::anyhow!("No videos found"))
        } else {
            videos.sort_by(|a, b| a.playlist_index.cmp(&b.playlist_index));
            Ok(videos.iter().map(|v| VideoType::Disk(v.clone())).collect())
        }
    }
    pub fn delete(&self) -> Result<(), anyhow::Error> {
        std::fs::remove_file(self.path.clone())?;
        Ok(())
    }
    pub fn from_path(
        path: PathBuf,
        url: String,
        audio_only: bool,
        id: String,
    ) -> Result<Self, Error> {
        let file_name = path.file_name().unwrap().to_str().unwrap();
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
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let video = !audio_only;

        let playlist_index = file_name
            .split('_')
            .nth(1)
            .unwrap()
            .split('.')
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap_or(0);
        Ok(Self {
            url,
            path: path.clone(),
            title: title.to_string(),
            duration,
            video,
            playlist_index,
        })
    }
}

#[cfg(feature = "spotify")]
pub async fn get_spotify_shiz(url: String) -> Result<Vec<VideoType>, Error> {
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
        // iterate over the videos, and search youtube for them
        let mut vids = Vec::new();
        for video in videos {
            let vid = youtube_search(video).await?;
            // get the first video
            if vid.is_empty() {
                continue;
            } else {
                vids.push(
                    Video::get_video(vid[0].clone().url, true, false)
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
