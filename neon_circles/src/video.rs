#![allow(dead_code)]
use anyhow::Result;
#[cfg(feature = "spotify")]
pub async fn get_spotify_shiz(url: String) -> Result<Vec<common::video::VideoType>> {
    use common::{
        video::Video,
        youtube::{get_spotify_song_title, youtube_search},
    };
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
