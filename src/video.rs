#![allow(dead_code)]

use std::path::PathBuf;

use anyhow::Error;
// use async_recursion::async_recursion;
use serde::Deserialize;
use ytd_rs::Arg;

#[cfg(feature = "spotify")]
use crate::youtube::get_spotify_song_title;
#[cfg(feature = "youtube-search")]
use crate::youtube::youtube_search;
use crate::{commands::music::VideoType, youtube::VideoInfo};

#[derive(Debug, Clone)]
pub struct Video {
    pub url: String,
    pub path: PathBuf,
    pub title: String,
    pub duration: f64,
    pub video: bool,
    pub playlist_index: usize,
}

#[derive(Deserialize, Debug)]
struct RawVideo {
    #[serde(rename = "webpage_url")]
    url: String,
    title: String,
    duration: u32,
}

async fn get_videos(url: &str) -> Result<Vec<RawVideo>, anyhow::Error> {
    let mut bot_path = crate::Config::get().data_path.clone();
    bot_path.push("cookies.txt");

    let output = if bot_path.exists() {
        tokio::process::Command::new("yt-dlp")
            .args(["--cookies", bot_path.to_str().unwrap()])
            .args(["--default-search", "ytsearch"])
            .arg("--dump-json")
            .arg(url)
            .output()
            .await?
    } else {
        tokio::process::Command::new("yt-dlp")
            .args(["--default-search", "ytsearch"])
            .arg("--dump-json")
            .arg(url)
            .output()
            .await?
    };

    // let output = tokio::process::Command::new("yt-dlp")
    //     .arg("--dump-json")
    //     .arg(url)
    //     .output()
    //     .await?;

    // let output =
    //     .args(args.as_slice())
    //     .output()
    //     .await?;

    // turn stdout into a string
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
    pub async fn get_video(
        url: String,
        // audio_only: bool,
        allow_playlist: bool,
    ) -> Result<Vec<VideoType>, anyhow::Error> {
        let mut v = get_videos(url.clone().as_str()).await?;
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
                })
            })
            .collect::<Vec<VideoType>>())
    }
    pub async fn download_video(
        url: String,
        audio_only: bool,
        spoiler: bool,
        max_filesize: &str,
    ) -> Result<VideoType, anyhow::Error> {
        let v = Self::get_video(url.clone(), false).await?;
        let v = v.get(0).ok_or(anyhow::anyhow!("No videos found"))?;
        // convert to downloaded video type
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
                    // Arg::new_with_arg("--max-filesize", max_filesize),
                    Arg::new_with_arg(
                        "--output",
                        format!("{}_%(playlist_index)s.%(ext)s", id).as_str(),
                    ),
                    Arg::new("--embed-metadata"),
                ];

                // if bot directory/cookies.txt exists, include it
                let mut bot_path = crate::Config::get().data_path.clone();
                bot_path.push("cookies.txt");
                if bot_path.exists() {
                    args.push(Arg::new_with_arg("--cookies", bot_path.to_str().unwrap()));
                }
                // println!("{:?}", args);
                // println!("{:?}", bot_path);

                if audio_only {
                    args.push(Arg::new("-x"));
                    args.push(Arg::new_with_arg("--audio-format", "mp3"));
                } else {
                    // args.push(Arg::new_with_arg(
                    //     "-f",
                    //     format!("best[filesize<={}]", max_filesize).as_str(),
                    // ));
                    args.push(Arg::new_with_arg("-S", "res,ext:mp4:m4a"));
                    args.push(Arg::new_with_arg("--recode", "mp4"));
                }

                let ytd = ytd_rs::YoutubeDL::new(&path, args.clone(), url.as_str())?;
                let response = match tokio::task::spawn_blocking(move || ytd.download()).await? {
                    Ok(r) => r,
                    Err(_) => {
                        if !audio_only {
                            args.retain(|a| {
                                a.to_string()
                                    != Arg::new_with_arg(
                                        "-f",
                                        format!("best[filesize<={}]", max_filesize).as_str(),
                                    )
                                    .to_string()
                            });
                            let ytd = ytd_rs::YoutubeDL::new(&path, args, url.as_str())?;
                            tokio::task::spawn_blocking(move || ytd.download()).await??
                        } else {
                            return Err(anyhow::anyhow!("Failed to download audio"));
                        }
                    }
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
                                url.clone(),
                                audio_only,
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
                        .get(0)
                        .cloned()
                        .ok_or(anyhow::anyhow!("No videos found"))?)
                }
            }
        }
    }
    // #[async_recursion]
    // pub async fn get_video(
    //     url: String,
    //     audio_only: bool,
    //     allow_playlist: bool,
    // ) -> Result<Vec<VideoType>, anyhow::Error> {
    //     // if url is empty
    //     if url.is_empty() {
    //         return Self::get_video(crate::Config::get().bumper_url, audio_only, allow_playlist)
    //             .await;
    //     }
    //     // if url is spotify
    //     #[cfg(feature = "spotify")]
    //     if url.contains("spotify.com") {
    //         // get the title for the page
    //         return get_spotify_shiz(url).await;
    //     }

    //     #[cfg(not(feature = "download"))]
    //     if allow_playlist {
    //         let vid = crate::youtube::get_video_info(url.clone()).await;
    //         if let Ok(vid) = vid {
    //             return Ok(vec![VideoType::Url(vid)]);
    //         } else {
    //             return Err(anyhow::anyhow!("Could not get video info"));
    //         }
    //     }
    //     let id = nanoid::nanoid!(10);
    //     let mut path = crate::Config::get().data_path.clone();
    //     path.push("tmp");
    //     std::fs::create_dir_all(&path)?;
    //     let mut args = vec![
    //         Arg::new("--quiet"),
    //         Arg::new_with_arg(
    //             "--output",
    //             format!("{}_%(playlist_index)s.%(ext)s", id).as_str(),
    //         ),
    //         Arg::new("--embed-metadata"),
    //     ];
    //     if audio_only {
    //         args.push(Arg::new("-x"));
    //         args.push(Arg::new_with_arg("--audio-format", "mp3"));
    //     } else {
    //         args.push(Arg::new_with_arg("-S", "res,ext:mp4:m4a"));
    //         args.push(Arg::new_with_arg("--recode", "mp4"));
    //     }
    //     if !allow_playlist {
    //         args.push(Arg::new("--no-playlist"));
    //     }
    //     let ytd = ytd_rs::YoutubeDL::new(&path, args, url.as_str())?;
    //     let response = tokio::task::spawn_blocking(move || ytd.download()).await??;

    //     let file = response.output_dir();

    //     let mut videos = Vec::new();
    //     for entry in std::fs::read_dir(file)? {
    //         let entry = entry?;
    //         let path = entry.path();
    //         if path.is_file() {
    //             let file_name = path
    //                 .file_name()
    //                 .ok_or(anyhow::anyhow!("No Path"))?
    //                 .to_str()
    //                 .ok_or(anyhow::anyhow!("No Path"))?;
    //             if file_name.starts_with(id.as_str()) {
    //                 videos.push(Self::from_path(path, url.clone(), audio_only, id.clone())?);
    //             }
    //         }
    //     }
    //     if videos.is_empty() {
    //         Err(anyhow::anyhow!("No videos found"))
    //     } else {
    //         videos.sort_by(|a, b| a.playlist_index.cmp(&b.playlist_index));
    //         Ok(videos.iter().map(|v| VideoType::Disk(v.clone())).collect())
    //     }
    // }
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
                    Video::get_video(vid[0].clone().url, false)
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

async fn run_preprocessor(filepath: &PathBuf) -> Result<(), Error> {
    // run the preprocessor script located in the data folder if it exists and is executable
    let mut path = crate::Config::get().data_path.clone();
    path.push("preprocessor.sh");
    if path.exists() {
        let mut cmd = tokio::process::Command::new(path);
        cmd.arg(filepath);
        cmd.spawn()?.wait().await?;
    }
    Ok(())
}
