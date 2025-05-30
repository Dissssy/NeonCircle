use crate::video::Video;
use anyhow::Result;
#[allow(dead_code)]
fn get_speech(text: &str) -> Result<Video> {
    let config = crate::config::get_config();
    let output = std::process::Command::new("node")
        .arg(config.sam_path)
        .arg(text)
        .output()?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to run command: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    // let mut path = config.data_path.clone();
    // path.push("tmp");
    let mut path = crate::TEMP_PATH.clone();
    let id = nanoid::nanoid!(10);
    let name = format!("{}-tts.wav", id);
    path.push(&name);
    std::fs::write(&path, output.stdout)?;
    Video::from_path(path, "n/a".to_owned(), crate::video::MediaType::Audio, id)
}
