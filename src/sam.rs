// all we have to do is run the command "node {config.sam_path} {text}" and stdout will be a wav file with the speech

use crate::video::Video;
use anyhow::Result;

pub fn get_speech(text: &str) -> Result<Video> {
    let config = crate::Config::get();
    let output = std::process::Command::new("node").arg(config.sam_path).arg(text).output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("Failed to run command: {}", String::from_utf8_lossy(&output.stderr)));
    }

    // we're gonna write this wav file to a temp file
    let mut path = config.data_path.clone();
    path.push("tmp");
    let id = nanoid::nanoid!(10);
    let name = format!("{}-tts.wav", id);
    path.push(&name);

    std::fs::write(&path, output.stdout)?;

    Video::from_path(path, "n/a".to_owned(), true, id)
}
