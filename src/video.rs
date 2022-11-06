use std::path::PathBuf;

use ytd_rs::Arg;

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
    pub async fn get_video(url: String, audio_only: bool, allow_playlist: bool) -> Result<Vec<Self>, anyhow::Error> {
        // println!("Getting video");
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
        // find every file in the directory that starts with the id
        let mut videos = Vec::new();
        for entry in std::fs::read_dir(file)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let file_name = path.file_name().unwrap().to_str().unwrap();
                if file_name.starts_with(id.as_str()) {
                    let tag = audiotags::Tag::new().read_from_path(&path)?;
                    let title = tag.title().unwrap_or(&id);
                    let s = ffprobe::ffprobe(&path)?;
                    let duration = s.streams[0].duration.as_ref().unwrap().parse::<f64>().unwrap();
                    let video = !audio_only;
                    // parse the playlist index out of the filename, it is between _ and .
                    let playlist_index = file_name.split('_').nth(1).unwrap().split('.').next().unwrap().parse::<usize>().unwrap_or(0);
                    let video = Self {
                        url: url.clone(),
                        path: path.clone(),
                        title: title.to_string(),
                        duration,
                        video,
                        playlist_index,
                    };
                    videos.push(video);
                }
            }
        }
        if videos.is_empty() {
            Err(anyhow::anyhow!("No videos found"))
        } else {
            // sort the videos by playlist index, lowest first
            videos.sort_by(|a, b| a.playlist_index.cmp(&b.playlist_index));
            Ok(videos)
        }
        // let file = file.read_dir().unwrap().find(|f| {
        //     let f = f.as_ref().unwrap();
        //     let f = f.file_name();
        //     let f = f.to_str().unwrap();
        //     f.starts_with(id.as_str())
        // });
        // if let Some(file) = file {
        //     let file = file?;
        //     let path = file.path();
        //     let tag = audiotags::Tag::new().read_from_path(&path)?;
        //     let title = tag.title().unwrap_or(&id);
        //     let duration = tag.duration().unwrap_or(Duration::from_secs(0).as_secs_f64());
        //     let video = !audio_only;
        //     Ok(Self {
        //         url: url.to_string(),
        //         path,
        //         title: title.to_owned(),
        //         duration,
        //         video,
        //     })
        // } else {
        //     Err(anyhow::anyhow!("file not found"))
        // }
    }
    pub fn delete(&self) -> Result<(), anyhow::Error> {
        std::fs::remove_file(self.path.clone())?;
        Ok(())
    }
}
