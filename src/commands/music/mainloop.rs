use rand::Rng;

use serenity::prelude::Mutex;
use songbird::input::Restartable;
use songbird::tracks::{LoopState, TrackHandle};
use songbird::{create_player, Call};

use songbird::ffmpeg;
use songbird::ytdl;

use std::mem;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serenity::futures::channel::mpsc;

use crate::commands::music::{AudioPromiseCommand, MetaVideo, VideoType};

use super::MessageReference;

pub async fn the_lüüp(
    call: Arc<Mutex<Call>>,
    rx: &mut mpsc::UnboundedReceiver<(mpsc::UnboundedSender<String>, AudioPromiseCommand)>,
    msg: MessageReference,
    looptime: u64,
    nothing_uri: Option<PathBuf>,
) {
    let mut msg = msg.clone();
    let mut log = String::new();
    let mut trackhandle: Option<TrackHandle> = None;
    let mut queue: Vec<MetaVideo> = Vec::new();
    let mut paused = false;
    let mut looped = false;
    let mut shuffled = false;
    let mut nothing_handle: Option<TrackHandle> = None;
    let mut tts_handle: Option<TrackHandle> = None;
    let mut volume = 1.0;

    let mut current_track: Option<MetaVideo> = None;
    loop {
        let command = rx.try_next();
        if let Ok(Some((snd, command))) = command {
            match command {
                AudioPromiseCommand::Play(videos) => {
                    for v in videos {
                        queue.push(v);
                    }

                    snd.unbounded_send(String::from("Added to queue")).unwrap();
                }
                AudioPromiseCommand::Stop => {
                    let r = snd.unbounded_send(String::from("Stopped"));
                    if let Err(e) = r {
                        log.push_str(&format!("Error sending stop: {}\r", e));
                    }
                    break;
                }
                AudioPromiseCommand::Pause => {
                    if let Some(trackhandle) = trackhandle.as_mut() {
                        paused = true;
                        let r = trackhandle.pause();
                        if r.is_ok() {
                            let r2 = snd.unbounded_send(String::from("Paused"));
                            if let Err(e) = r2 {
                                log.push_str(&format!("Error sending pause: {}\r", e));
                            }
                        } else {
                            log.push_str(&format!("Error pausing track: {}\r", r.unwrap_err()));
                        }
                    } else {
                        let r = snd.unbounded_send(String::from("Nothing is playing"));
                        if let Err(e) = r {
                            log.push_str(&format!("Error updating message: {}\r", e));
                        }
                    }
                }
                AudioPromiseCommand::Resume => {
                    if let Some(trackhandle) = trackhandle.as_mut() {
                        paused = false;
                        let r = trackhandle.play();
                        if r.is_ok() {
                            let r2 = snd.unbounded_send(String::from("Resumed"));
                            if let Err(e) = r2 {
                                log.push_str(&format!("Error sending resume: {}\r", e));
                            }
                        } else {
                            log.push_str(&format!("Error resuming track: {}\r", r.unwrap_err()));
                        }
                    } else {
                        let r = snd.unbounded_send(String::from("Nothing is playing"));
                        if let Err(e) = r {
                            log.push_str(&format!("Error updating message: {}\r", e));
                        }
                    }
                }
                AudioPromiseCommand::Shuffle(shuffle) => {
                    if shuffled != shuffle {
                        shuffled = shuffle;
                        let r = snd.unbounded_send(format!("Shuffle set to `{}`", shuffled));
                        if let Err(e) = r {
                            log.push_str(&format!("Error updating message: {}\r", e));
                        }
                    } else {
                        let r = snd.unbounded_send(format!("Shuffle is already `{}`", shuffled));
                        if let Err(e) = r {
                            log.push_str(&format!("Error updating message: {}\r", e));
                        }
                    }
                }
                AudioPromiseCommand::Loop(loopi) => {
                    if looped != loopi {
                        looped = loopi;
                        let r = snd.unbounded_send(format!("Loop set to `{}`", looped));
                        if let Err(e) = r {
                            log.push_str(&format!("Error updating message: {}\r", e));
                        }
                    } else {
                        let r = snd.unbounded_send(format!("Loop is already `{}`", looped));
                        if let Err(e) = r {
                            log.push_str(&format!("Error updating message: {}\r", e));
                        }
                    }
                }
                AudioPromiseCommand::Skip => {
                    if let Some(trackhandle) = trackhandle.as_mut() {
                        let r = trackhandle.stop();
                        if r.is_ok() {
                            let r2 = snd.unbounded_send(String::from("Skipped"));
                            if let Err(e) = r2 {
                                log.push_str(&format!("Error sending skip: {}\r", e));
                            }
                        } else {
                            log.push_str(&format!("Error skipping track: {}\r", r.unwrap_err()));
                        }
                    } else {
                        let r = snd.unbounded_send(String::from("Nothing is playing"));
                        if let Err(e) = r {
                            log.push_str(&format!("Error updating message: {}\r", e));
                        }
                    }
                }
                AudioPromiseCommand::Volume(v) => {
                    volume = v;

                    let r = snd.unbounded_send(format!("Volume set to `{}%`", volume * 100.0));
                    if let Err(e) = r {
                        log.push_str(&format!("Error updating message: {}\r", e));
                    }
                }
                AudioPromiseCommand::Remove(index) => {
                    let index = index - 1;
                    if index < queue.len() {
                        let mut v = queue.remove(index);
                        let r = v.delete();
                        if let Err(r) = r {
                            log.push_str(&format!("Error removing `{}`: {}\r", v.title, r));
                            let r =
                                snd.unbounded_send(format!("Error removing `{}`: {}", v.title, r));
                            if let Err(e) = r {
                                log.push_str(&format!("Error updating message: {}\r", e));
                            }
                        } else {
                            let r = snd.unbounded_send(format!("Removed `{}`", v.title));
                            if let Err(e) = r {
                                log.push_str(&format!("Error updating message: {}\r", e));
                            }
                        }
                    } else {
                        let r = snd.unbounded_send(format!(
                            "Index out of range, max is `{}`",
                            queue.len()
                        ));
                        if let Err(e) = r {
                            log.push_str(&format!("Error updating message: {}\r", e));
                        }
                    }
                }
            }
        } else {
        }

        if let Some(current) = current_track.as_mut() {
            if let Some(thandle) = trackhandle.as_mut() {
                let playmode =
                    tokio::time::timeout(Duration::from_secs(2), thandle.get_info()).await;
                if let Ok(playmode) = playmode {
                    if let Err(playmode) = playmode {
                        if playmode == songbird::tracks::TrackError::Finished {
                            let mut t = None;
                            mem::swap(&mut current_track, &mut t);
                            if let Some(t) = t.as_mut() {
                                if looped {
                                    queue.push(t.clone());
                                } else {
                                    let r = t.delete();
                                    if let Err(e) = r {
                                        log.push_str(&format!("Error deleting video: {}\n", e));
                                    }
                                }
                            }
                            current_track = None;
                            trackhandle = None;
                            tts_handle = None;
                        } else {
                            log.push_str(format!("playmode error: {:?}", playmode).as_str());
                        }
                    }
                } else {
                    log.push_str(&format!("playmode timeout: {}\r", playmode.unwrap_err()));
                }
            } else if let Some(tts) = tts_handle.as_mut() {
                let r = tokio::time::timeout(Duration::from_secs(2), tts.get_info()).await;
                if let Ok(r) = r {
                    if let Err(r) = r {
                        if r == songbird::tracks::TrackError::Finished {
                            let calllock = call.try_lock();
                            if let Ok(mut clock) = calllock {
                                let r = match current.video.clone() {
                                    VideoType::Disk(v) => {
                                        tokio::time::timeout(
                                            Duration::from_secs(2),
                                            ffmpeg(&v.path),
                                        )
                                        .await
                                    }
                                    VideoType::Url(v) => {
                                        tokio::time::timeout(Duration::from_secs(2), ytdl(&v.url))
                                            .await
                                    }
                                };
                                if let Ok(r) = r {
                                    if let Ok(src) = r {
                                        let (mut audio, handle) = create_player(src);
                                        audio.set_volume(volume);
                                        clock.play(audio);
                                        trackhandle = Some(handle);
                                    } else {
                                        log.push_str(&format!(
                                            "Error playing track: {}\r",
                                            r.unwrap_err()
                                        ));
                                    }
                                } else {
                                    log.push_str(&format!(
                                        "Error playing track: {}\r",
                                        r.unwrap_err()
                                    ));
                                }
                            }
                        } else {
                            log.push_str(&format!("Error getting tts info: {:?}\r", r));
                        }
                    }
                } else {
                    log.push_str(&format!("Error getting tts info: {}\r", r.unwrap_err()));
                }
            } else {
                let calllock = call.try_lock();
                if let Ok(mut clock) = calllock {
                    #[cfg(feature = "tts")]
                    if let Some(tts) = current.ttsmsg.as_ref() {
                        let r = tokio::time::timeout(Duration::from_secs(2), ffmpeg(&tts.path))
                            .await
                            .unwrap();
                        if let Ok(r) = r {
                            let (mut audio, handle) = create_player(r);
                            audio.set_volume(volume);
                            clock.play(audio);
                            tts_handle = Some(handle);
                        } else {
                            let (mut audio, handle) = create_player(
                                ytdl(crate::Config::get().bumper_url.as_str())
                                    .await
                                    .unwrap(),
                            );
                            audio.set_volume(volume);
                            clock.play(audio);
                            tts_handle = Some(handle);
                        }
                    } else {
                        let (mut audio, handle) = create_player(
                            ytdl(crate::Config::get().bumper_url.as_str())
                                .await
                                .unwrap(),
                        );
                        audio.set_volume(volume);
                        clock.play(audio);
                        tts_handle = Some(handle);
                    }
                    #[cfg(not(feature = "tts"))]
                    {
                        let (mut audio, handle) = create_player(
                            ytdl(crate::Config::get().bumper_url.as_str())
                                .await
                                .unwrap(),
                        );
                        audio.set_volume(volume);
                        clock.play(audio);
                        tts_handle = Some(handle);
                    }
                }
            }
        } else if !queue.is_empty() {
            let index = if shuffled {
                rand::thread_rng().gen_range(0..queue.len())
            } else {
                0
            };
            current_track = Some(queue.remove(index));
        }

        if queue.is_empty() && current_track.is_none() {
            if nothing_handle.is_none() {
                let r = if let Some(uri) = nothing_uri.clone() {
                    tokio::time::timeout(Duration::from_secs(2), Restartable::ffmpeg(uri, false))
                        .await
                } else {
                    // tokio::time::timeout(Duration::from_secs(2), Restartable::ytdl("https://www.youtube.com/watch?v=xy_NKN75Jhw", false)).await
                    tokio::time::timeout(
                        Duration::from_secs(2),
                        Restartable::ffmpeg(crate::Config::get().idle_url, false),
                    )
                    .await
                };

                if let Ok(r) = r {
                    if let Ok(src) = r {
                        let (mut audio, handle) = create_player(src.into());
                        let calllock = call.try_lock();
                        if let Ok(mut clock) = calllock {
                            audio.set_loops(LoopState::Infinite).unwrap();
                            audio.set_volume(volume / 5.);
                            clock.play(audio);
                            nothing_handle = Some(handle);
                        } else {
                            log.push_str(&format!(
                                "Error locking call: {}\r",
                                calllock.unwrap_err()
                            ));
                        }
                    } else {
                        log.push_str(&format!(
                            "Error playing nothing: {}\nfile_uri: {:?}",
                            r.unwrap_err(),
                            nothing_uri.clone()
                        ));
                    }
                } else {
                    log.push_str(&format!(
                        "Error playing nothing: {}\nfile_uri: {:?}",
                        r.unwrap_err(),
                        nothing_uri.clone()
                    ));
                }
            }
            let r = tokio::time::timeout(
                Duration::from_secs(2),
                msg.update("Queue is empty, use `/play` to play something!"),
            )
            .await;
            if let Ok(r) = r {
                if let Err(e) = r {
                    log.push_str(&format!(
                        "Error updating message: {:?}. probably got deleted, sending a new one",
                        e
                    ));
                    let j = format!("{:?}", e).to_lowercase();

                    if j.contains("unknown message") {
                        let r = tokio::time::timeout(Duration::from_secs(2), msg.send_new()).await;
                        if let Ok(r) = r {
                            if let Err(e) = r {
                                log.push_str(&format!("Error sending new message: {:?}", e));
                            }
                        } else {
                            log.push_str(&format!(
                                "Error sending new message: {:?}",
                                r.unwrap_err()
                            ));
                        }
                    }
                }
            } else {
                log.push_str(&format!("Error updating message: {}\r", r.unwrap_err()));
            }
        } else {
            if nothing_handle.is_some() {
                if let Some(handle) = nothing_handle.as_mut() {
                    let r = handle.stop();
                    if let Err(e) = r {
                        log.push_str(&format!("Error stopping nothing: {}\n", e));
                    }
                }
                nothing_handle = None;

                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            let mut message = String::new();
            if let Some(t) = current_track.as_ref() {
                message.push_str(&format!("Playing: `{}` ", t.title));
                if paused {
                    message.push_str("<:pause:1038954686402789518>");
                }
                if looped {
                    message.push_str("<:loop:1038954691318526024>");
                }
                if shuffled {
                    message.push_str("<:shuffle:1038954690114764880>");
                }
                if let Some(handle) = trackhandle.as_ref() {
                    let info =
                        tokio::time::timeout(Duration::from_secs(2), handle.get_info()).await;
                    if let Ok(info) = info {
                        if let Ok(info) = info {
                            match t.video.clone() {
                                VideoType::Disk(v) => {
                                    let percent_done = info.position.as_secs_f64() / v.duration;
                                    // println!("{}% done", percent_done);
                                    // let bar = (percent_done * 20.0).round() as usize;
                                    // message.push_str(&format!("\n`[{:20}]`", "=".repeat(bar)));
                                    message.push_str(&format!("\n{}", get_bar(percent_done, 20)));
                                }
                                VideoType::Url(_) => {}
                            };
                        }
                    } else {
                        log.push_str(&format!(
                            "Error getting track info: {}\r",
                            info.unwrap_err()
                        ));
                    }
                }
                message.push('\n');

                if !queue.is_empty() {
                    // message.push_str("Queue:\n");
                    message.push_str("```\n");
                    for (i, track) in queue.iter().enumerate() {
                        message.push_str(&format!("{}. {}\n", i + 1, track.title));
                    }
                    message.push_str("```");
                }
            } else {
                // message.push_str("Queue:\n");
                message.push_str("```\n");
                for (i, track) in queue.iter().enumerate() {
                    message.push_str(&format!("{}. {}\n", i + 1, track.title));
                }
                message.push_str("```");
            }
            let r = tokio::time::timeout(Duration::from_secs(2), msg.update(&message)).await;
            if let Ok(r) = r {
                if let Err(e) = r {
                    log.push_str(&format!(
                        "Error updating message: {:?}. probably got deleted, sending a new one",
                        e
                    ));
                    let j = format!("{:?}", e).to_lowercase();

                    if j.contains("unknown message") {
                        let r = msg.send_new().await;
                        if let Err(e) = r {
                            log.push_str(&format!("Error sending new message: {:?}", e));
                        }
                    }
                }
            } else {
                log.push_str(&format!("Error updating message: {}\r", r.unwrap_err()));
            }
        }

        if let Some(handle) = trackhandle.as_mut() {
            let r = handle.set_volume(volume);
            if let Err(e) = r {
                log.push_str(&format!("Error setting volume: {}\r", e));
            }
        }
        if let Some(handle) = nothing_handle.as_mut() {
            let r = handle.set_volume(volume / 5.);
            if let Err(e) = r {
                log.push_str(&format!("Error setting volume: {}\r", e));
            }
        }
        if !log.is_empty() {
            println!("{}", log);
            log.clear();
        }
        tokio::time::sleep(std::time::Duration::from_millis(looptime)).await;
        let mut brk = false;
        {
            let calllock = call.try_lock();
            if let Ok(clock) = calllock {
                if clock.current_connection().is_none() {
                    brk = true;
                }
            }
        }
        if brk {
            break;
        }
    }

    let mut calllock = call.lock().await;
    rx.close();
    calllock.stop();
    if let Some(t) = trackhandle.as_mut() {
        let r = t.stop();
        if let Err(e) = r {
            log.push_str(&format!("Error stopping track: {}\n", e));
        }
    }
    if let Some(t) = current_track.as_mut() {
        let mut tries = 10;
        tokio::time::sleep(Duration::from_millis(100)).await;
        while t.delete().is_err() {
            tokio::time::sleep(Duration::from_millis(100)).await;
            tries -= 1;
            log.push_str(&format!("Failed to delete file, {} tries left", tries));
            if tries == 0 {
                log.push_str("Failed to delete file, giving up");
                break;
            }
        }
    }
    for video in queue.iter_mut() {
        let r = video.delete();
        if let Err(e) = r {
            log.push_str(&format!("Error deleting video: {}\n", e));
        }
    }
    let r = calllock.leave().await;
    if let Err(e) = r {
        log.push_str(&format!("Error leaving voice channel: {}\n", e));
    }

    if !log.is_empty() {
        println!("Final log: {}", log);
    }
    let r = msg.delete().await;
    if let Err(e) = r {
        println!("Error deleting message: {}", e);
    }
    println!("Gracefully exited");
}

fn get_bar(percent_done: f64, length: usize) -> String {
    let emojis = vec![
        vec!["<:LE:1038954704744480898>", "<:LC:1038954708422885386>"],
        vec!["<:CE:1038954710184497203>", "<:CC:1038954696980824094>"],
        vec!["<:RE:1038954703033217285>", "<:RC:1038954706841649192>"],
    ];
    let mut bar = String::new();

    // subtract 1/length from percent_done to make sure the bar is always full
    let percent_done = percent_done - (1.0 / length as f64);

    // create a bar of length length
    // if we are on the first position, we need to use the left emoji set (the first in the vec)
    // if we are on the last position, we need to use the right emoji set (the last in the vec)
    // if we just passed percent_done, we need to use the C emoji (the second in the position's vec)
    // if we are before or after percent_done, we need to use the E emoji (the first in the position's vec)
    let mut first = true;
    let mut circled = false;
    for i in 0..length {
        let pos = i as f64 / length as f64;
        if first {
            // we use the left emoji set
            if pos >= percent_done && !circled {
                bar.push_str(emojis[0][1]);
                circled = true;
            } else {
                bar.push_str(emojis[0][0]);
            }
            first = false;
        } else if i == length - 1 {
            // we use the right emoji set
            if pos >= percent_done && !circled {
                bar.push_str(emojis[2][1]);
                circled = true;
            } else {
                bar.push_str(emojis[2][0]);
            }
        } else {
            // we use the center emoji set
            if pos >= percent_done && !circled {
                bar.push_str(emojis[1][1]);
                circled = true;
            } else {
                bar.push_str(emojis[1][0]);
            }
        }
    }
    bar
}
