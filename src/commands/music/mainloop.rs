use rand::Rng;

use serenity::prelude::Mutex;
use songbird::input::Restartable;
use songbird::tracks::{LoopState, TrackHandle};
use songbird::{create_player, Call};

#[cfg(feature = "download")]
use songbird::ffmpeg;
#[cfg(not(feature = "download"))]
use songbird::ytdl;

#[cfg(not(feature = "download"))]
use crate::youtube::VideoInfo;

use std::mem;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serenity::futures::channel::mpsc;

use crate::commands::music::AudioPromiseCommand;
#[cfg(feature = "download")]
use crate::video::Video;

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
    #[cfg(feature = "download")]
    let mut queue: Vec<Video> = Vec::new();
    #[cfg(not(feature = "download"))]
    let mut queue: Vec<VideoInfo> = Vec::new();
    let mut paused = false;
    let mut looped = false;
    let mut shuffled = false;
    let mut nothing_handle: Option<TrackHandle> = None;
    let mut volume = 1.0;
    // let mut promises: Vec<Option<JoinHandle<Result<Video, Error>>>> = Vec::new();
    #[cfg(feature = "download")]
    let mut current_track: Option<Video> = None;
    #[cfg(not(feature = "download"))]
    let mut current_track: Option<VideoInfo> = None;
    loop {
        let command = rx.try_next();
        if let Ok(Some((snd, command))) = command {
            match command {
                AudioPromiseCommand::Play(videos) => {
                    #[cfg(feature = "download")]
                    for v in videos {
                        queue.push(v);
                    }
                    #[cfg(not(feature = "download"))]
                    queue.push(videos);
                    // if msg.is_none() {
                    //     msg = Some(message);
                    // } else {
                    //     let r = message.delete().await;
                    //     if let Err(e) = r {
                    //         log.push_str(&format!("Error deleting message: {}\r", e));
                    //     }
                    // }
                    snd.unbounded_send(String::from("Added to queue")).unwrap();
                }
                AudioPromiseCommand::Stop => {
                    // if let Some(msg) = msg.as_mut() {
                    //     mem::swap(msg, &mut message);
                    //     let r = message.delete().await;
                    //     if let Err(e) = r {
                    //         log.push_str(&format!("Error deleting message: {}\r", e));
                    //     }
                    // }
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
                    // shuffle is true if we want to shuffle, false if we want to unshuffle
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
                    // looped is true if we want to loop, false if we want to unloop
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
                    // send success message
                    let r = snd.unbounded_send(format!("Volume set to `{}%`", volume * 100.0));
                    if let Err(e) = r {
                        log.push_str(&format!("Error updating message: {}\r", e));
                    }
                }
                AudioPromiseCommand::Remove(index) => {
                    let index = index - 1;
                    if index < queue.len() {
                        let r = snd.unbounded_send(format!("Removed `{}`", queue.remove(index).title));
                        if let Err(e) = r {
                            log.push_str(&format!("Error updating message: {}\r", e));
                        }
                    } else {
                        let r = snd.unbounded_send(format!("Index out of range, max is `{}`", queue.len()));
                        if let Err(e) = r {
                            log.push_str(&format!("Error updating message: {}\r", e));
                        }
                    }
                }
            }
        } else {
            // println!("NONE");
        }
        if let Some(t) = trackhandle.as_mut() {
            let playmode = tokio::time::timeout(Duration::from_secs(2), t.get_info()).await;
            if let Ok(playmode) = playmode {
                if let Err(playmode) = playmode {
                    if playmode == songbird::tracks::TrackError::Finished {
                        trackhandle = None;
                        if current_track.is_some() {
                            let mut t = None;
                            mem::swap(&mut current_track, &mut t);
                            if let Some(t) = t.as_mut() {
                                if looped {
                                    queue.push(t.clone());
                                } else {
                                    #[cfg(feature = "download")]
                                    {
                                        let r = t.delete();
                                        if let Err(e) = r {
                                            log.push_str(&format!("Error deleting video: {}\n", e));
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        log.push_str(format!("playmode error: {:?}", playmode).as_str());
                    }
                }
            } else {
                log.push_str(&format!("playmode timeout: {}\r", playmode.unwrap_err()));
            }
        } else {
            let calllock = call.try_lock();
            if let Ok(mut clock) = calllock {
                if !queue.is_empty() {
                    let index = if shuffled { rand::thread_rng().gen_range(0..queue.len()) } else { 0 };
                    let track = queue.remove(index);
                    #[cfg(feature = "download")]
                    let r = tokio::time::timeout(Duration::from_secs(2), ffmpeg(&track.path)).await;
                    #[cfg(not(feature = "download"))]
                    let r = tokio::time::timeout(Duration::from_secs(2), ytdl(&track.url)).await;
                    if let Ok(r) = r {
                        if let Ok(src) = r {
                            let (audio, handle) = create_player(src);
                            current_track = Some(track);
                            clock.play(audio);
                            trackhandle = Some(handle);
                        } else {
                            log.push_str(&format!("Error playing track: {}\n", r.unwrap_err()));
                        }
                    } else {
                        log.push_str(&format!("Error playing track: {}\n", r.unwrap_err()));
                    }
                }
            }
        }
        if queue.is_empty() && current_track.is_none() {
            if nothing_handle.is_none() {
                // play the nothing_uri with ffmpeg
                let r = if let Some(uri) = nothing_uri.clone() {
                    tokio::time::timeout(Duration::from_secs(2), Restartable::ffmpeg(uri, false)).await
                } else {
                    tokio::time::timeout(Duration::from_secs(2), Restartable::ytdl("https://www.youtube.com/watch?v=xy_NKN75Jhw", false)).await
                };
                // let r = Restartable::;
                if let Ok(r) = r {
                    if let Ok(src) = r {
                        let (mut audio, handle) = create_player(src.into());
                        let calllock = call.try_lock();
                        if let Ok(mut clock) = calllock {
                            audio.set_loops(LoopState::Infinite).unwrap();
                            clock.play(audio);
                            nothing_handle = Some(handle);
                            // println!("playing nothing");
                            // println!("{:?}", nothing_uri);
                        } else {
                            log.push_str(&format!("Error locking call: {}\r", calllock.unwrap_err()));
                        }
                    } else {
                        log.push_str(&format!("Error playing nothing: {}\nfile_uri: {:?}", r.unwrap_err(), nothing_uri.clone()));
                    }
                } else {
                    log.push_str(&format!("Error playing nothing: {}\nfile_uri: {:?}", r.unwrap_err(), nothing_uri.clone()));
                }
            }
            let r = tokio::time::timeout(Duration::from_secs(2), msg.update("Queue is empty, use `/play` to play something!")).await;
            if let Ok(r) = r {
                if let Err(e) = r {
                    log.push_str(&format!("Error updating message: {:?}. probably got deleted, sending a new one", e));
                    let j = format!("{:?}", e).to_lowercase();
                    // println!("THE ERROR MESSAGE IS `{}` HOW DOESNT IT CONTAIN UNKNOWN MESSAGE", j);
                    if j.contains("unknown message") {
                        let r = tokio::time::timeout(Duration::from_secs(2), msg.send_new()).await;
                        if let Ok(r) = r {
                            if let Err(e) = r {
                                log.push_str(&format!("Error sending new message: {:?}", e));
                            }
                        } else {
                            log.push_str(&format!("Error sending new message: {:?}", r.unwrap_err()));
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
                // wait 100ms for the nothing to stop
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            // message structured like this:
            // Playing: `<current track>`
            // Queue: (only if there is a queue)
            // ```
            // 1. <track 1>
            // 2. <track 2>
            // 3. <track 3> (repeat until end of queue. ensure the message is no longer than 1000 characters)
            // ```
            let mut message = String::new();
            if let Some(t) = current_track.as_ref() {
                message.push_str(&format!("Playing: `{}` ", t.title));
                if paused {
                    message.push_str(":pause_button:");
                }
                if looped {
                    message.push_str(":arrows_counterclockwise:");
                }
                if shuffled {
                    message.push_str(":twisted_rightwards_arrows:");
                }
                #[cfg(feature = "download")]
                if let Some(handle) = trackhandle.as_ref() {
                    let info = tokio::time::timeout(Duration::from_secs(2), handle.get_info()).await;
                    if let Ok(info) = info {
                        if let Ok(info) = info {
                            let percent_done = info.position.as_secs_f64() / t.duration;
                            let bar = (percent_done * 20.0).round() as usize;
                            message.push_str(&format!("\n`[{:20}]`", "=".repeat(bar)));
                            // message.push_str(&format!("{:?}", info));
                            // info.position is the current position in the track as a Duration
                        }
                    } else {
                        log.push_str(&format!("Error getting track info: {}\r", info.unwrap_err()));
                    }
                }
                message.push('\n');

                if !queue.is_empty() {
                    message.push_str("Queue:\n```\n");
                    for (i, track) in queue.iter().enumerate() {
                        message.push_str(&format!("{}. {}\n", i + 1, track.title));
                    }
                    message.push_str("```");
                }
            } else {
                message.push_str("Queue:\n```\n");
                for (i, track) in queue.iter().enumerate() {
                    message.push_str(&format!("{}. {}\n", i + 1, track.title));
                }
                message.push_str("```");
            }
            let r = tokio::time::timeout(Duration::from_secs(2), msg.update(&message)).await;
            if let Ok(r) = r {
                if let Err(e) = r {
                    log.push_str(&format!("Error updating message: {:?}. probably got deleted, sending a new one", e));
                    let j = format!("{:?}", e).to_lowercase();
                    // println!("THE ERROR MESSAGE IS `{}` HOW DOESNT IT CONTAIN UNKNOWN MESSAGE", j);
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
        // set the volume for the current track and the nothing track
        if let Some(handle) = trackhandle.as_mut() {
            let r = handle.set_volume(volume);
            if let Err(e) = r {
                log.push_str(&format!("Error setting volume: {}\n", e));
            }
        }
        if let Some(handle) = nothing_handle.as_mut() {
            let r = handle.set_volume(volume);
            if let Err(e) = r {
                log.push_str(&format!("Error setting volume: {}\n", e));
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
    // println!("exiting audio promise");
    // stop the current track
    let mut calllock = call.lock().await;
    rx.close();
    calllock.stop();
    if let Some(t) = trackhandle.as_mut() {
        let r = t.stop();
        if let Err(e) = r {
            log.push_str(&format!("Error stopping track: {}\n", e));
        }
    }
    #[cfg(feature = "download")]
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
    #[cfg(feature = "download")]
    for video in queue {
        let r = video.delete();
        if let Err(e) = r {
            log.push_str(&format!("Error deleting video: {}\n", e));
        }
    }
    let r = calllock.leave().await;
    if let Err(e) = r {
        log.push_str(&format!("Error leaving voice channel: {}\n", e));
    }
    // println!("Stopped");
    if !log.is_empty() {
        println!("Final log: {}", log);
    }
    let r = msg.delete().await;
    if let Err(e) = r {
        println!("Error deleting message: {}", e);
    }
    println!("Gracefully exited");
}
