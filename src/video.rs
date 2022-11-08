use std::path::PathBuf;

use anyhow::Error;
use ytd_rs::Arg;

use crate::commands::music::VideoType;

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
    pub async fn get_video(url: String, audio_only: bool, allow_playlist: bool) -> Result<Vec<VideoType>, anyhow::Error> {
        #[cfg(not(feature = "download"))]
        if allow_playlist {
            let vid = get_video_info(url.clone()).await;
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
            Arg::new_with_arg("--output", format!("{}_%(playlist_index)s.%(ext)s", id).as_str()),
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
    pub fn from_path(path: PathBuf, url: String, audio_only: bool, id: String) -> Result<Self, Error> {
        let file_name = path.file_name().unwrap().to_str().unwrap();
        let tag = audiotags::Tag::new().read_from_path(&path);
        let title = if let Ok(tag) = tag.as_ref() { tag.title().unwrap_or(&id) } else { &id };
        let s = ffprobe::ffprobe(&path)?;
        let duration = s.streams[0].duration.as_ref().unwrap().parse::<f64>().unwrap();
        let video = !audio_only;

        let playlist_index = file_name.split('_').nth(1).unwrap().split('.').next().unwrap().parse::<usize>().unwrap_or(0);
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
