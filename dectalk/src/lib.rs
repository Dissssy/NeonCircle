// dectalk binary at {module path}/files/say
// say -fo stdout:raw -a "hello" | ffmpeg -f s16le -ar 11025 -ac 1 -i - -f wav /tmp/{nanoid as long as it doesn't exist}.wav

use std::{path::PathBuf, process::Stdio, str::FromStr};

use common::{
    anyhow::{anyhow, Result},
    lazy_static::lazy_static,
    nanoid,
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt as _},
        process::Command,
    },
    video::Video,
};

lazy_static! {
    static ref DECTALK_BINARY: PathBuf = {
        let path = PathBuf::from_str("/git/alrightguysnewprojecttime/dectalk/files/say")
            .expect("Failed to get module path");
        assert!(path.exists(), "dectalk binary not found");
        path
    };
}

pub async fn get_speech(text: &str) -> Result<Video> {
    // let config = crate::config::get_config();
    // let output = Command::new("node")
    //     .arg(config.sam_path)
    //     .arg(text)
    //     .output()?;
    // if !output.status.success() {
    //     return Err(anyhow::anyhow!(
    //         "Failed to run command: {}",
    //         String::from_utf8_lossy(&output.stderr)
    //     ));
    // }
    // // let mut path = config.data_path.clone();
    // // path.push("tmp");
    // let mut path = crate::TEMP_PATH.clone();
    // let id = nanoid::nanoid!(10);
    // let name = format!("{}-tts.wav", id);
    // path.push(&name);
    // std::fs::write(&path, output.stdout)?;
    // Video::from_path(path, "n/a".to_owned(), crate::video::MediaType::Audio, id)

    let id = format!("{}-dectalk", nanoid::nanoid!(10));
    let path = common::TEMP_PATH.join(format!("{}.wav", id));

    let raw_bytes = {
        let mut dectalk = Command::new(&*DECTALK_BINARY)
            .args(["-fo", "stdout:raw"])
            .args(["-a", text])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = dectalk
            .stdout
            .as_mut()
            .ok_or_else(|| anyhow!("Failed to get dectalk stdout"))?;

        let mut dectalk_result = Vec::new();
        stdout.read_to_end(&mut dectalk_result).await?;

        let status = dectalk.wait().await?;
        if !status.success() {
            let stderr = dectalk
                .stderr
                .as_mut()
                .ok_or_else(|| anyhow!("Failed to get dectalk stderr"))?;

            let mut out = String::new();
            stderr.read_to_string(&mut out).await?;
            return Err(anyhow!("Failed to run dectalk: {}", out));
        }

        dectalk_result
    };

    // the output is raw 16-bit signed little-endian PCM, 11025 Hz, mono audio
    let mut ffmpeg = Command::new("ffmpeg")
        .args(["-f", "s16le"])
        .args(["-ar", "11025"])
        .args(["-ac", "1"])
        .args(["-i", "-"])
        .args(["-f", "wav"])
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    {
        let stdin = ffmpeg
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("Failed to get ffmpeg stdin"))?;

        stdin.write_all(&raw_bytes).await?;
    }

    let status = ffmpeg.wait().await?;
    if !status.success() {
        let stderr = ffmpeg
            .stderr
            .as_mut()
            .ok_or_else(|| anyhow!("Failed to get ffmpeg stderr"))?;

        let mut out = String::new();
        stderr.read_to_string(&mut out).await?;
        return Err(anyhow!("Failed to run ffmpeg: {}", out));
    }

    Video::from_path(path, "n/a".to_owned(), common::video::MediaType::Audio, id)
}

// #[cfg(test)]
// mod tests {
//     use common::tokio;

//     use super::*;

//     #[tokio::test]
//     async fn test_get_speech() {
//         let video = get_speech("This is a test")
//             .await
//             .expect("Failed to get speech");
//         assert!(video.path().exists(), "video path does not exist");
//         // mv to /git/alrightguysnewprojecttime/dectalk/ for testing
//         // tokio::fs::copy(
//         //     video.path(),
//         //     "/git/alrightguysnewprojecttime/dectalk/hello.wav",
//         // )
//         // .await
//         // .expect("Failed to copy file");
//         tokio::fs::remove_file(video.path())
//             .await
//             .expect("Failed to remove file");
//     }
// }
