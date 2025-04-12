use anyhow::Result;
use serde::Deserialize;
use serenity::{
    all::{Context, GuildId, User},
    async_trait, futures::StreamExt as _,
};
use songbird::{input::File, tracks::Track};
use std::{path::PathBuf, sync::Arc};
use tokio::{sync::RwLock, task::JoinHandle};
use ytd_rs::Arg;
#[derive(Debug, Clone)]
pub struct MetaVideo {
    pub video: VideoType,
    // pub title: Arc<str>,
    pub author: Option<Author>,
    #[cfg(feature = "tts")]
    pub ttsmsg: Option<LazyLoadedVideo>,
}
#[derive(Debug, Clone)]
pub enum VideoType {
    Disk(Video),
    Url(VideoInfo),
}
impl VideoType {
    pub fn to_songbird(&self) -> Track {
        match self {
            VideoType::Disk(v) => v.to_songbird(),
            VideoType::Url(v) => v.to_songbird(),
        }
    }
    pub fn get_duration(&self) -> Option<f64> {
        match self {
            VideoType::Disk(v) => Some(v.duration()),
            VideoType::Url(v) => v.duration(),
        }
    }
    #[allow(dead_code)]
    pub fn get_title(&self) -> Arc<str> {
        match self {
            VideoType::Disk(v) => v.title(),
            VideoType::Url(v) => v.title(),
        }
    }
}
#[derive(Debug, Clone)]
pub struct Author {
    pub name: String,
    pub pfp_url: String,
}
impl Author {
    pub async fn from_user(ctx: &Context, user: &User, guild: Option<GuildId>) -> Option<Self> {
        let name = match guild {
            Some(g) => {
                let member = g.member(ctx, user.id).await.ok()?;
                member.display_name().to_string()
            }
            None => user.name.clone(),
        };
        let pfp_url = user
            .avatar_url()
            .unwrap_or_else(|| user.default_avatar_url());
        Some(Self { name, pfp_url })
    }
}
#[derive(Debug, Clone)]
pub struct LazyLoadedVideo {
    handle: Arc<RwLock<Option<JoinHandle<anyhow::Result<Video>>>>>,
    video: Arc<RwLock<Option<Video>>>,
}
impl LazyLoadedVideo {
    pub fn new(handle: JoinHandle<anyhow::Result<Video>>) -> Self {
        Self {
            handle: Arc::new(RwLock::new(Some(handle))),
            video: Arc::new(RwLock::new(None)),
        }
    }
    // pub async fn check(&mut self) -> anyhow::Result<Option<Video>> {
    //     let mut lock = self.handle.write().await;
    //     if let Some(handle) = lock.take() {
    //         if handle.is_finished() {
    //             let video = handle.await??;
    //             self.video.write().await.replace(video.clone());
    //             Ok(Some(video))
    //         } else {
    //             lock.replace(handle);
    //             Ok(None)
    //         }
    //     } else {
    //         Err(anyhow::anyhow!("Handle is None"))
    //     }
    // }
    pub async fn wait_for(&mut self) -> anyhow::Result<Video> {
        let mut lock = self.handle.write().await;
        if let Some(handle) = lock.take() {
            let video = handle.await??;
            self.video.write().await.replace(video.clone());
            Ok(video)
        } else {
            Err(anyhow::anyhow!("Handle is None"))
        }
    }
}
#[derive(Debug, Clone)]
pub struct Video {
    inner: Arc<InnerVideo>,
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
                // let mut path = crate::config::get_config().data_path.clone();
                // path.push("tmp");
                let path = crate::TEMP_PATH.clone();
                // std::fs::create_dir_all(&path)?;
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
    pub fn from_upload(
        path: PathBuf,
        title: &str,
        // url: String,
        // media_type: MediaType,
        // id: String,
    ) -> Result<Self> {
        let s = ffprobe::ffprobe(&path)?;
        let duration = s
            .streams
            .first()
            .and_then(|s| s.duration.as_ref())
            .and_then(|d| d.parse::<f64>().ok())
            .unwrap_or(0.0);
        Ok(Self {
            inner: Arc::new(InnerVideo {
                url: "N/A".into(),
                path,
                title: title.into(),
                duration,
                media_type: MediaType::Video,
                playlist_index: 0,
            }),
        })
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Audio,
    Video,
}
#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub(crate) title: Arc<str>,
    pub(crate) url: Arc<str>,
    pub(crate) duration: Option<f64>,
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
    pub fn to_songbird(&self) -> Track {
        Track::new(
            songbird::input::YoutubeDl::new(crate::WEB_CLIENT.clone(), self.url().to_string())
                .into(),
        )
    }
    pub async fn to_metavideo(&self) -> anyhow::Result<MetaVideo> {
        let v = crate::video::Video::get_video(&self.url, true, false)
            .await?
            .first()
            .ok_or(anyhow::anyhow!("Could not get video"))?
            .clone();
        #[cfg(feature = "tts")]
        let title = match &v {
            VideoType::Disk(v) => v.title(),
            VideoType::Url(v) => v.title(),
        };
        #[cfg(feature = "tts")]
        return Ok(MetaVideo {
            video: v,
            ttsmsg: Some(LazyLoadedVideo::new(tokio::spawn(crate::youtube::get_tts(
                Arc::clone(&title),
                None,
            )))),
            // title,
            author: None,
        });
        #[cfg(not(feature = "tts"))]
        return Ok(MetaVideo { video: v, title });
    }
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
    #[cfg(feature = "spotify")]
    if url.starts_with("https://spotify.com/") || url.starts_with("https://open.spotify.com/")
    {
        return get_spotify_shiz(url).await;
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
#[derive(Deserialize, Debug)]
pub struct RawVideo {
    #[serde(rename = "webpage_url")]
    pub url: String,
    pub title: String,
    pub duration: Option<f64>,
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

// https://open.spotify.com/playlist/{playlist_id} IGNORE QUERY PARAMS -> https://api.spotify.com/v1/playlists/{playlist_id}/tracks
// https://open.spotify.com/track/{track_id} IGNORE QUERY PARAMS -> https://api.spotify.com/v1/tracks/{track_id}
// https://open.spotify.com/album/{album_id} IGNORE QUERY PARAMS -> https://api.spotify.com/v1/albums/{album_id}/tracks
// https://open.spotify.com/artist/{artist_id} IGNORE QUERY PARAMS -> https://api.spotify.com/v1/artists/{artist_id}/top-tracks?market=US


#[cfg(feature = "spotify")]
pub async fn get_spotify_shiz(url: String) -> Result<Vec<RawVideo>> {
    use super::youtube::youtube_search;
    let token = crate::config::get_config().spotify_key().await?;
    let mut tokenized = url.trim_start_matches("https://").split('/');
    let uri = tokenized.next();
    let media_type = tokenized.next();
    let id: Option<&str> = try {
        let raw_id = tokenized.next()?;
        raw_id.split('?').next()?
    };
    
    if let (Some(uri), Some(media_type), Some(id)) = (uri, media_type, id) {
        if uri != "open.spotify.com" {
            return Err(anyhow::anyhow!("Invalid Spotify URL"));
        }
        
        match media_type {
            "track" => {
                let url = format!("https://api.spotify.com/v1/tracks/{}", id);
                let res = crate::WEB_CLIENT
                    .get(url.as_str())
                    .header("Authorization", format!("Bearer {}", token.clone()))
                    .send()
                    .await?;

                let text = res.text().await?;

                let parsed = serde_json::from_str::<spotify_types::RawSpotifyTrack>(&text);
                let spoofydata = match parsed {
                    Ok(data) => data,
                    Err(e) => {
                        log::error!("Failed to parse Spotify response: {}", e);
                        log::trace!("Response text: {}", text);
                        return Err(anyhow::anyhow!("Failed to parse Spotify response"));
                    }
                };

                match spoofydata {
                    spotify_types::RawSpotifyTrack::Error{ error: err } => {
                        Err(anyhow::anyhow!("Spotify API Error: {}", err.message))
                    }
                    spotify_types::RawSpotifyTrack::Data(spoofydata) => {
                        log::info!("Spoofydata: {:?}", spoofydata);
                        let youtube_vid = youtube_search(&spoofydata.nice_name(), 1).await?;
                        if youtube_vid.is_empty() {
                            return Err(anyhow::anyhow!("No videos found"));
                        }
                        let vid = youtube_vid.first().ok_or(anyhow::anyhow!("No videos found"))?;
                        Ok(vec![RawVideo {
                            url: vid.url.clone(),
                            title: vid.title.clone(),
                            duration: Some(vid.duration.unwrap_or(0.0)),
                        }])
                    }
                }
            }
            "playlist" => {
                let url = format!("https://api.spotify.com/v1/playlists/{}", id);
                let res = crate::WEB_CLIENT
                    .get(url.as_str())
                    .header("Authorization", format!("Bearer {}", token.clone()))
                    .send()
                    .await?;

                let text = res.text().await?;
                let value = {
                    let raw_val = serde_json::from_str::<serde_json::Value>(&text);
                    match raw_val {
                        Ok(val) => val,
                        Err(e) => {
                            log::error!("Failed to parse Spotify response: {}", e);
                            log::trace!("Response text: {}", text);
                            return Err(anyhow::anyhow!("Failed to parse Spotify response"));
                        }
                    }
                };
                // let album_cache = value.get("album").cloned().ok_or(anyhow::anyhow!("No album found"))?;
                let parsed = serde_json::from_value::<spotify_types::RawSpotifyPlaylist>(value);
                let spoofydata = match parsed {
                    Ok(data) => data,
                    Err(e) => {
                        log::error!("Failed to parse Spotify response: {}", e);
                        log::trace!("Response text: {}", text);
                        return Err(anyhow::anyhow!("Failed to parse Spotify response"));
                    }
                };
                match spoofydata {
                    spotify_types::RawSpotifyPlaylist::Error{ error: err } => {
                        Err(anyhow::anyhow!("Spotify API Error: {}", err.message))
                    }
                    spotify_types::RawSpotifyPlaylist::Data(spoofydata) => {
                        let spotify_types::PlaylistResponse {
                            tracks: spotify_types::PlaylistNextResponse { mut items, mut next },
                        } = spoofydata;
                        while let Some(next_url) = next {
                            let res = crate::WEB_CLIENT
                                .get(next_url.as_str())
                                .header("Authorization", format!("Bearer {}", token.clone()))
                                .send()
                                .await?;
                            let text = res.text().await?;
                            let val = {
                                let raw_val = serde_json::from_str::<serde_json::Value>(&text);
                                match raw_val {
                                    Ok(val) => val,
                                    Err(e) => {
                                        log::error!("Failed to parse Spotify response: {}", e);
                                        log::trace!("Response text: {}", text);
                                        return Err(anyhow::anyhow!("Failed to parse Spotify response"));
                                    }
                                }
                            };
                            // reinject the album data
                            // val["album"] = album_cache.clone();
                            let parsed = serde_json::from_value::<spotify_types::RawSpotifyPlaylistNext>(val);
                            let next_data = match parsed {
                                Ok(data) => data,
                                Err(e) => {
                                    log::error!("Failed to parse Spotify response: {}", e);
                                    log::trace!("Response text: {}", text);
                                    return Err(anyhow::anyhow!("Failed to parse Spotify response"));
                                }
                            };
                            match next_data {
                                spotify_types::RawSpotifyPlaylistNext::Error{ error: err } => {
                                    return Err(anyhow::anyhow!("Spotify API Error: {}", err.message));
                                }
                                spotify_types::RawSpotifyPlaylistNext::Data(spoofydata) => {
                                    let spotify_types::PlaylistNextResponse { items: next_items, next: next_next } = spoofydata;
                                    items.extend(next_items);
                                    next = next_next;
                                }
                            }
                        }
                        // log::trace!("Spotify Playlist Items: {:#?}", items);
                        
                        let vids = get_all_youtube(items.iter().map(|i| i.track.nice_name()).collect::<Vec<_>>()).await?;
                        if vids.is_empty() {
                            return Err(anyhow::anyhow!("No videos found"));
                        }
                        Ok(vids)
                    }
                }
            }
            "album" => {
                let url = format!("https://api.spotify.com/v1/albums/{}", id);
                let res = crate::WEB_CLIENT
                    .get(url.as_str())
                    .header("Authorization", format!("Bearer {}", token.clone()))
                    .send()
                    .await?;

                let text = res.text().await?;
                let value = {
                    let raw_val = serde_json::from_str::<serde_json::Value>(&text);
                    match raw_val {
                        Ok(val) => val,
                        Err(e) => {
                            log::error!("Failed to parse Spotify response: {}", e);
                            log::trace!("Response text: {}", text);
                            return Err(anyhow::anyhow!("Failed to parse Spotify response"));
                        }
                    }
                };
                let parsed = serde_json::from_value::<spotify_types::RawSpotifyAlbum>(value);
                let spoofydata = match parsed {
                    Ok(data) => data,
                    Err(e) => {
                        log::error!("Failed to parse Spotify response: {}", e);
                        log::trace!("Response text: {}", text);
                        return Err(anyhow::anyhow!("Failed to parse Spotify response"));
                    }
                };
                match spoofydata {
                    spotify_types::RawSpotifyAlbum::Error{ error: err } => {
                        Err(anyhow::anyhow!("Spotify API Error: {}", err.message))
                    }
                    spotify_types::RawSpotifyAlbum::Data(spoofydata) => {
                        let spotify_types::AlbumResponse {
                            // name: _,
                            tracks: spotify_types::AlbumNextResponse { mut items, mut next },
                        } = spoofydata;
                        while let Some(next_url) = next {
                            let res = crate::WEB_CLIENT
                                .get(next_url.as_str())
                                .header("Authorization", format!("Bearer {}", token.clone()))
                                .send()
                                .await?;
                            let text = res.text().await?;
                            let val = {
                                let raw_val = serde_json::from_str::<serde_json::Value>(&text);
                                match raw_val {
                                    Ok(val) => val,
                                    Err(e) => {
                                        log::error!("Failed to parse Spotify response: {}", e);
                                        log::trace!("Response text: {}", text);
                                        return Err(anyhow::anyhow!("Failed to parse Spotify response"));
                                    }
                                }
                            };
                            let parsed = serde_json::from_value::<spotify_types::RawSpotifyAlbumNext>(val);
                            let next_data = match parsed {
                                Ok(data) => data,
                                Err(e) => {
                                    log::error!("Failed to parse Spotify response: {}", e);
                                    log::trace!("Response text: {}", text);
                                    return Err(anyhow::anyhow!("Failed to parse Spotify response"));
                                }
                            };
                            match next_data {
                                spotify_types::RawSpotifyAlbumNext::Error{ error: err } => {
                                    return Err(anyhow::anyhow!("Spotify API Error: {}", err.message));
                                }
                                spotify_types::RawSpotifyAlbumNext::Data(spoofydata) => {
                                    let spotify_types::AlbumNextResponse { items: next_items, next: next_next } = spoofydata;
                                    items.extend(next_items);
                                    next = next_next;
                                }
                            }
                        }
                        // log::trace!("Spotify Album Items: {:#?}", items);
                        let vids = get_all_youtube(items.iter().map(|i| i.nice_name()).collect::<Vec<_>>()).await?;
                        if vids.is_empty() {
                            return Err(anyhow::anyhow!("No videos found"));
                        }
                        Ok(vids)
                    }
                }
            }
            "artist" => {
                let url = format!("https://api.spotify.com/v1/artists/{}/top-tracks?market=US", id);
                let res = crate::WEB_CLIENT
                    .get(url.as_str())
                    .header("Authorization", format!("Bearer {}", token.clone()))
                    .send()
                    .await?;

                let text = res.text().await?;
                let parsed = serde_json::from_str::<spotify_types::RawSpotifyArtist>(&text);
                let spoofydata = match parsed {
                    Ok(data) => data,
                    Err(e) => {
                        log::error!("Failed to parse Spotify response: {}", e);
                        log::trace!("Response text: {}", text);
                        return Err(anyhow::anyhow!("Failed to parse Spotify response"));
                    }
                };
                match spoofydata {
                    spotify_types::RawSpotifyArtist::Error{ error: err } => {
                        Err(anyhow::anyhow!("Spotify API Error: {}", err.message))
                    }
                    spotify_types::RawSpotifyArtist::Data(spoofydata) => {
                        let videos = get_all_youtube(spoofydata.tracks.iter().map(|i| i.nice_name()).collect::<Vec<_>>()).await?;
                        if videos.is_empty() {
                            return Err(anyhow::anyhow!("No videos found"));
                        }
                        Ok(videos)
                    }
                }
            }
            _ => {
                Err(anyhow::anyhow!("Invalid Spotify media type: {}", media_type))
            }
        }


        // let videos = get_spotify_song_titles(id).await?;
        // if videos.is_empty() {
        //     Err(anyhow::anyhow!("No videos found"))
        // } else {
        //     let mut vids = Vec::new();
        //     for video in videos {
        //         let vid = youtube_search(&video, 1).await?;
        //         if let Some(vid) = vid.first() {
        //             vids.push(
        //                 RawVideo {
        //                     url: vid.url.clone(),
        //                     title: vid.title.clone(),
        //                     duration: Some(vid.duration.unwrap_or(0.0)),
        //                 },
        //             );
        //         }
        //     }
        //     Ok(vids)
        // }
    } else {
        Err(anyhow::anyhow!("Invalid Spotify URL"))
    }
}

async fn get_all_youtube(names: Vec<String>) -> Result<Vec<RawVideo>> {
    use super::youtube::youtube_search;
    let mut futures = serenity::futures::stream::FuturesOrdered::new();
    for name in names {
        futures.push_back(tokio::spawn(async move {
            let vids = youtube_search(&name, 1).await;
            match vids {
                Ok(v) => {
                    if !v.is_empty() {
                        log::info!("Found video for {}: {:?}", name, v);
                        Some(RawVideo {
                            url: v[0].url.clone(),
                            title: v[0].title.clone(),
                            duration: v[0].duration,
                        })
                    } else {
                        log::warn!("No video found for {}", name);
                        None
                    }
                }
                Err(e) => {
                    log::error!("Error searching for {}: {}", name, e);
                    None
                }
            }
        }));
    }
    let mut vids = Vec::new();
    while let Some(result) = futures.next().await {
        match result {
            Ok(Some(video)) => {
                vids.push(video);
            }
            _ => {
                log::warn!("Failed to get video from future");
            }
        }
    }

    Ok(vids)
}

mod spotify_types {
    use serde::Deserialize;

    pub type RawSpotifyTrack = SpotifyResponse<TrackResponse>;
    pub type RawSpotifyPlaylist = SpotifyResponse<PlaylistResponse>;
    pub type RawSpotifyPlaylistNext = SpotifyResponse<PlaylistNextResponse>;
    pub type RawSpotifyAlbum = SpotifyResponse<AlbumResponse>;
    pub type RawSpotifyAlbumNext = SpotifyResponse<AlbumNextResponse>;
    pub type RawSpotifyArtist = SpotifyResponse<ArtistResponse>;

    #[derive(Deserialize, Debug)]
    #[serde(untagged)]
    pub enum SpotifyResponse<T> {
        Error{
            error: ResponseError,
        },
        Data(T),
    }

    #[derive(Deserialize, Debug)]
    pub struct ResponseError {
        // pub status: u32,
        pub message: String,
    }

    #[derive(Deserialize, Debug)]
    pub struct TrackResponse {
        pub album: TrackAlbum,
        pub artists: Vec<TrackArtist>,
        pub name: String,
    }

    #[derive(Deserialize, Debug)]
    pub struct TrackAlbum {
        pub name: String,
        // pub release_date: String,
    }

    #[derive(Deserialize, Debug)]
    pub struct TrackArtist {
        pub name: String,
    }

    impl TrackResponse {
        pub fn nice_name(&self) -> String {
            let test_name = self.name.to_lowercase();
            let test_album = self.album.name.to_lowercase();
            if test_name == test_album || test_album.contains("soundtrack") || test_album.contains("ost") {
                format!(
                    "{} by {}",
                    self.name,
                    self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
                )
            } else {
                format!(
                    "{} by {}",
                    self.name,
                    self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "),
                    // self.album.name
                )
            }
        }
    }

    #[derive(Deserialize, Debug)]
    pub struct PlaylistResponse {
        pub tracks: PlaylistNextResponse,
    }

    #[derive(Deserialize, Debug)]
    pub struct PlaylistNextResponse {
        pub next: Option<String>,
        pub items: Vec<PlaylistItemResponse>,
    }

    #[derive(Deserialize, Debug)]
    pub struct PlaylistItemResponse {
        pub track: TrackResponse,
    }

    #[derive(Deserialize, Debug)]
    pub struct AlbumResponse {
        // pub name: String,
        // pub release_date: String,
        pub tracks: AlbumNextResponse, 
    }

    #[derive(Deserialize, Debug)]
    pub struct AlbumNextResponse {
        pub next: Option<String>,
        pub items: Vec<AlbumTrackResponse>,
    }

    // #[derive(Deserialize, Debug)]
    // pub struct AlbumItemResponse {
    //     pub track: AlbumTrackResponse,
    // }

    #[derive(Deserialize, Debug)]
    pub struct AlbumTrackResponse {
        pub artists: Vec<TrackArtist>,
        pub name: String,
    }

    impl AlbumTrackResponse {
        pub fn nice_name(&self, /* album_name: &str */) -> String {
            format!(
                "{} by {}",
                self.name,
                self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "),
                // album_name
            )
        }
    }

    #[derive(Deserialize, Debug)]
    pub struct ArtistResponse {
        pub tracks: Vec<TrackResponse>,
    }

    // #[derive(Deserialize, Debug)]
    // pub struct TrackResponse {
    //     pub album: Album,
    //     pub artists: Vec<Artist>,
    //     pub name: String,
    //     // pub duration_ms: u32,
    // }

    // impl TrackResponse {
    //     pub fn nice_name(&self) -> String {
    //         let test_name = self.name.to_lowercase();
    //         let test_album = self.album.name.to_lowercase();
    //         if test_name == test_album || test_album.contains("soundtrack") || test_album.contains("ost") {
    //             format!(
    //                 "{} by {}",
    //                 self.name,
    //                 self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
    //             )
    //         } else {
    //             format!(
    //                 "{} by {} on {}",
    //                 self.name,
    //                 self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", "),
    //                 self.album.name
    //             )
    //         }
    //     }
    // }

    // #[derive(Deserialize, Debug)]
    // pub struct PlaylistResponse {
    //     #[serde(deserialize_with = "denest_tracks")]
    //     pub tracks: TracksWithNext,
    // }

    // #[derive(Deserialize, Debug)]
    // pub struct AlbumResponse {
    //     #[serde(deserialize_with = "denest_tracks")]
    //     pub tracks: TracksWithNext, 
    // }

    // #[derive(Deserialize, Debug)]
    // pub struct TracksWithNext {
    //     pub next: Option<String>,
    //     #[serde(deserialize_with = "denest_items")]
    //     pub items: Vec<TrackResponse>,
    // }

    // #[derive(Deserialize, Debug)]
    // pub struct ArtistResponse {
    //     pub tracks: Vec<TrackResponse>,
    // }
    
    // // deserializes the nested tracks object, reusable by re-extracting missing info if necessary
    // fn denest_tracks<'de, D>(deserializer: D) -> Result<TracksWithNext, D::Error>
    // where
    //     D: serde::Deserializer<'de>,
    // {
    //     let base: serde_json::Value = serde_json::Value::deserialize(deserializer)?;
    //     let mut tracks = base
    //         .get("items")
    //         .and_then(|items| items.as_array())
    //         .unwrap_or(&vec![])
    //         .iter()
    //         .map(|item| item.get("track").unwrap_or(item))
    //         .cloned()
    //         .collect::<Vec<_>>();
    //     let mut cached_album: Option<Value> = None;
    //     println!("Base: {:#?}", base);
        
    //     for track in tracks.iter_mut() {
    //         if track.get("album").is_none() {
    //             match cached_album {
    //                 Some(ref album) => {
    //                     track["album"] = album.clone();
    //                 }
    //                 None => {
    //                     cached_album = Some(json!({
    //                         "name": base.get("name").unwrap_or(&serde_json::Value::Null),
    //                         "release_date": base.get("release_date").unwrap_or(&serde_json::Value::Null)
    //                     }));
    //                     if let Some(ref album) = cached_album {
    //                         track["album"] = album.clone();
    //                     }
    //                 }
    //             }
    //         }
    //     }

    //     let tracks = serde_json::from_value::<Vec<TrackResponse>>(serde_json::Value::Array(tracks))
    //         .map_err(serde::de::Error::custom)?;
    //     Ok(TracksWithNext {
    //         next: base.get("next").and_then(|n| n.as_str()).map(|s| s.to_string()),
    //         items: tracks,
    //     })
    // }

    // fn denest_items<'de, D>(deserializer: D) -> Result<Vec<TrackResponse>, D::Error>
    // where
    //     D: serde::Deserializer<'de>,
    // {
    //     let base: serde_json::Value = serde_json::Value::deserialize(deserializer)?;
    //     // EITHER the data will be on each item, OR it will exist within the "track" field of each item
    //     // so we need to check for both and extract the data accordingly
    //     let items = base
    //         .as_array()
    //         .unwrap_or(&vec![])
    //         .iter()
    //         .filter_map(|item| {
    //             if let Some(track) = item.get("track") {
    //                 serde_json::from_value::<TrackResponse>(track.clone()).ok()
    //             } else {
    //                 serde_json::from_value::<TrackResponse>(item.clone()).ok()
    //             }
    //         })
    //         .collect::<Vec<_>>();
    //     Ok(items)
    // }

    // #[derive(Deserialize, Debug)]
    // pub struct Album {
    //     pub name: String,
    //     // pub release_date: String,
    // }

    // #[derive(Deserialize, Debug)]
    // pub struct Artist {
    //     pub name: String,
    // }
}

// let token = crate::config::get_config().spotify_api_key;
//     let url = format!("https://api.spotify.com/v1/tracks/{}", id);
//     let res = crate::WEB_CLIENT
//         .get(url.as_str())
//         .header("Authorization", format!("Bearer {}", token.clone()))
//         .send()
//         .await?;
//     let spoofydata = res.json::<RawSpotifyTrack>().await;
//     if let Ok(spoofy) = spoofydata {
//         Ok(vec![format!(
//             "{} - {}",
//             spoofy.name,
//             spoofy
//                 .artists
//                 .iter()
//                 .map(|a| a.name.as_str())
//                 .collect::<Vec<_>>()
//                 .join(", ")
//         )])
//     } else {
//         let url = format!("https://api.spotify.com/v1/albums/{}", id);
//         let res = crate::WEB_CLIENT
//             .get(url.as_str())
//             .header("Authorization", format!("Bearer {}", token.clone()))
//             .send()
//             .await?;
//         let spoofydata = res.json::<RawSpotifyAlbum>().await;
//         if let Ok(spoofy) = spoofydata {
//             Ok(spoofy
//                 .tracks
//                 .items
//                 .iter()
//                 .map(|t| {
//                     format!(
//                         "{} - {}",
//                         t.name,
//                         t.artists
//                             .iter()
//                             .map(|a| a.name.as_str())
//                             .collect::<Vec<_>>()
//                             .join(", ")
//                     )
//                 })
//                 .collect())
//         } else {
//             log::info!("spoofydata: {:?}", spoofydata);
//             Err(anyhow::anyhow!("Could not get spotify song title"))
//         }
//     }