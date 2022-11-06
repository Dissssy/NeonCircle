use anyhow::Error;

pub async fn search(query: String) -> Vec<VideoInfo> {
    let url = format!("https://www.youtube.com/results?search_query={}", query);
    let client = reqwest::Client::new();
    let res = client.get(url.as_str()).send().await;
    let mut videos = Vec::new();
    if let Ok(res) = res {
        // attempt to parse out video ids from the html by splitting on the youtube watch url
        let text = res.text().await;
        // dump the html to a file
        // let mut file = File::create("youtube.html").unwrap();
        // file.write_all(text.as_ref().unwrap().as_bytes()).unwrap();
        if let Ok(text) = text {
            let split = text.split("{\"url\":\"/watch?v=");
            let mut h = Vec::new();
            for (i, s) in split.map(|s| s.to_owned()).enumerate() {
                if i > 1 {
                    break;
                }
                h.push(tokio::task::spawn(async move {
                    // now we split on the next quotation mark
                    let split = s.split('\"').next();
                    // the first element is the video id
                    if let Some(id) = split {
                        // ensure id does not contain any invalid characters
                        if !id.chars().any(|c| c == '>' || c == ' ' || c == '/' || c == '\\') {
                            let url = format!("https://www.youtube.com/watch?v={}", id);
                            let vid = get_video_info(url).await;
                            // println!("Found video: {:?}", vid);
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
    // println!("{:?}", videos);
    videos
}

pub async fn get_video_info(url: String) -> Result<VideoInfo, Error> {
    // get the youtube video page, and parse it for the title tag
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
            // title is in the <title> tag
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
    // pub id: String,
    pub url: String,
}
