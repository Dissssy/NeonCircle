use rand::Rng;

use serenity::futures::{SinkExt, StreamExt};
use serenity::prelude::Mutex;
use serenity::utils::Colour;
use songbird::input::Restartable;
use songbird::tracks::{LoopState, TrackHandle};
use songbird::{create_player, Call};

use songbird::ffmpeg;
use songbird::ytdl;
use tokio::time::Instant;

use std::mem;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serenity::futures::channel::mpsc;

use crate::commands::music::{
    AudioPromiseCommand, MetaVideo, RawMessage, SpecificVolume, VideoType,
};
use crate::radio::AzuraCast;

use super::settingsdata::SettingsData;
use super::transcribe::TranscribeChannelHandler;
use super::{MessageReference, OrAuto};

pub struct ControlData {
    pub call: Arc<Mutex<Call>>,
    pub rx: mpsc::UnboundedReceiver<(
        serenity::futures::channel::oneshot::Sender<String>,
        AudioPromiseCommand,
    )>,
    pub msg: MessageReference,
    pub nothing_uri: Option<PathBuf>,
    pub transcribe: Arc<Mutex<TranscribeChannelHandler>>,
    pub settings: SettingsData,
    pub brk: bool,
}

pub async fn the_lüüp(
    rawcall: Arc<Mutex<Call>>,
    rawrx: mpsc::UnboundedReceiver<(
        serenity::futures::channel::oneshot::Sender<String>,
        AudioPromiseCommand,
    )>,
    rawmsg: MessageReference,
    rawlooptime: u64,
    rawnothing_uri: Option<PathBuf>,
    rawtranscribe: Arc<Mutex<TranscribeChannelHandler>>,
) {
    let log = Log::new();

    log.log("Starting loop").await;

    log.log("Creating control data").await;
    let mut control = ControlData {
        call: rawcall,
        rx: rawrx,
        msg: rawmsg,
        nothing_uri: rawnothing_uri,
        transcribe: rawtranscribe,
        settings: SettingsData::default(),
        brk: false,
    };
    {
        log.log("Locking call").await;
        let mut cl = control.call.lock().await;
        // cl.set_bitrate(Bitrate::BitsPerSecond(6_000));

        log.log("Setting bitrate").await;
        cl.set_bitrate(songbird::driver::Bitrate::Auto);
        log.log("Deafening").await;
        match cl.deafen(true).await {
            Ok(_) => {}
            Err(e) => {
                log.log(&format!("Error deafening: {:?}", e)).await;
            }
        };
    }
    let (mut msg_updater, update_msg) =
        serenity::futures::channel::mpsc::channel::<(SettingsData, EmbedData)>(8);
    let (killmsg, killrx) = tokio::sync::oneshot::channel::<()>();

    log.log("Spawning message updater").await;
    let msghandler = {
        let logger = log.clone();
        let msg = control.msg.clone();
        tokio::task::spawn(async move {
            let mut msg = msg;
            let mut killrx = killrx;
            let mut update_msg = update_msg;
            loop {
                tokio::select! {
                    _ = &mut killrx => {
                        break;
                    }
                    shakesbutt = update_msg.next() => {
                        // only get the latest
                        if let Some(shakesbutt) = shakesbutt {

                            let mut shakesbutt = shakesbutt;
                            while let Ok(Some(u)) = update_msg.try_next() {
                                shakesbutt = u;
                            }
                            let r = msg.update(shakesbutt.0, shakesbutt.1).await;
                            if let Err(e) = r {
                                logger.log(&format!("Error updating message: {}", e)).await;
                            }
                        } else {
                            logger.log("Error getting next message").await;
                            break;
                        }
                    }
                }
            }
            if let Err(e) = msg.delete().await {
                logger.log(&format!("Error deleting message: {}", e)).await;
            }
        })
    };

    let mut trackhandle: Option<TrackHandle> = None;
    let mut queue: Vec<MetaVideo> = Vec::new();
    // let mut paused = false;
    // let mut repeated = false;
    // let mut looped = false;
    // let mut transcribed = false;
    let mut last_embed: Option<EmbedData> = None;
    let mut last_settings = None;
    // let mut shuffled = false;
    let mut nothing_handle: Option<TrackHandle> = None;
    let mut tts_handle: Option<TrackHandle> = None;
    // let mut volume = 1.0;
    // let mut radiovolume = 0.33;
    let g_timeout_time = Duration::from_millis(100);
    // let mut autoplay = false;
    let mut autoplay_thread: Option<tokio::task::JoinHandle<Option<MetaVideo>>> = None;
    log.log("Creating azuracast").await;
    let mut azuracast = match crate::Config::get().api_url {
        Some(ref url) => AzuraCast::new(url, log.clone(), g_timeout_time).await.ok(),
        None => None,
    };
    // let mut brk = false;

    // let mut transcribe_handler = super::transcribe::Holder::new(Arc::clone(&control.call));
    // let mut transcribe_handler =
    //     super::transcribe::MetaTranscribeHandler::new(Arc::clone(&_t_arcmutex));
    let mut data: Option<crate::radio::Root> = None;

    let mut current_track: Option<MetaVideo> = None;

    log.log("Locking transcription listener").await;
    let ttsrx = match control.transcribe.lock().await.lock() {
        Ok(t) => t,
        Err(e) => {
            log.log(&format!("Error locking transcribe: {}", e)).await;
            return;
        }
    };

    let ttshandler = super::transcribe::Handler::new(Arc::clone(&control.call));

    // psuedocode

    // spawn thread for handling tts with ttshandler, listening for messages, sending them in, etc. provide a "kill" oneshot for shutting down the thread and stopping the ttshandler, the oneshot will contain a oneshot that can be used to return the RawMessage Sender back to us to deregister it

    let (killsubthread, bekilled) = tokio::sync::oneshot::channel::<
        tokio::sync::oneshot::Sender<serenity::futures::channel::mpsc::Receiver<RawMessage>>,
    >();

    log.log("Spawning tts thread").await;
    let subthread = {
        let logger = log.clone();
        tokio::task::spawn(async move {
            let mut ttsrx = ttsrx;
            let mut ttshandler = ttshandler;
            let mut bekilled = bekilled;
            loop {
                let mut interv = tokio::time::interval(Duration::from_millis(100));

                tokio::select! {
                    returnwhatsmine = &mut bekilled => {
                        ttshandler.stop().await;
                        if let Ok(ttsrxreturner) = returnwhatsmine {
                            if ttsrxreturner.send(ttsrx).is_err() {
                                logger.log("Error returning ttsrx AHHHHH").await;
                            };
                        } else {
                            // parent thread died, so we should too :(
                        }
                        break;
                    }
                    msg = ttsrx.next() => {
                        if let Some(msg) = msg {
                            if let Err(e) = ttshandler.update(vec![msg]).await {
                                logger.log(&format!("Error updating tts: {}", e)).await;
                            }
                        }
                    }
                    _ = interv.tick() => {
                        if let Err(e) = ttshandler.shift().await {
                            logger.log(&format!("Error shifting tts: {}", e)).await;
                        }
                    }
                }
            }
        })
    };

    let mut run_dur = tokio::time::interval(tokio::time::Duration::from_millis(rawlooptime));
    // let mut testint = 0;
    loop {
        control.settings.log_empty = log.is_empty().await;
        tokio::select! {
            t = control.rx.next() => {
                match t {
                    Some((snd, command)) => {
                        // println!("Got command: {:?}", command);
                        match command {
                            AudioPromiseCommand::RetrieveLog(mut secret) => {
                                let chunks = log.get_chunks_4k().await;

                                let mut string_chunks = chunks.iter().map(|c| (c.s.clone(), c.end)).collect::<Vec<(String, usize)>>();

                                // limit string_chunks to 5 and get the end usize of the final chunk
                                let end = if string_chunks.len() > 5 {
                                    string_chunks.truncate(5);
                                    chunks[4].end - 1
                                } else {
                                    chunks.last().map(|e| e.end).unwrap_or(0)
                                };

                                let r = secret.send(string_chunks.into_iter().map(|(s, _)| s).collect::<Vec<String>>()).await;
                                if let Err(e) = r {
                                    log.log(&format!("Error sending log: {}\n", e)).await;
                                }
                                let r = snd.send("Log sent!".to_owned());
                                if let Err(e) = r {
                                    log.log(&format!("Error sending log: {}\n", e)).await;
                                }
                                log.clear_until(end).await;
                            }
                            AudioPromiseCommand::Play(videos) => {
                                for v in videos {
                                    queue.push(v);
                                }

                                let r = snd.send(String::from("Added to queue"));
                                if let Err(e) = r {
                                    log.log(&format!("Error sending play: {}\n", e)).await;
                                }
                            }
                            AudioPromiseCommand::Stop => {
                                let r = snd.send(String::from("Stopped"));
                                if let Err(e) = r {
                                    log.log(&format!("Error sending stop: {}\n", e)).await;
                                }
                                control.brk = true;
                            }
                            AudioPromiseCommand::Paused(paused) => {
                                let val = paused.get_val(control.settings.pause);

                                if let Some(trackhandle) = trackhandle.as_mut() {
                                    if control.settings.pause != val {
                                        control.settings.pause = val;

                                        let r = if control.settings.pause {
                                            trackhandle.pause()
                                        } else {
                                            trackhandle.play()
                                        };

                                        if let Err(e) = r {
                                            log.log(&format!("Error pausing track: {}\n", e)).await;
                                        } else {
                                            let r2 = snd.send(format!("Paused set to `{}`", control.settings.pause));
                                            if let Err(e) = r2 {
                                                log.log(&format!("Error sending pause: {}\n", e)).await;
                                            }
                                        }
                                    }
                                } else if let Err(e) = snd.send(String::from("Nothing is playing")) {
                                    log.log(&format!("Error updating message: {}\n", e)).await;
                                }
                            }
                            // AudioPromiseCommand::Resume => {
                            //     if let Some(trackhandle) = trackhandle.as_mut() {
                            //         control.settings.pause = false;
                            //         let r = trackhandle.play();
                            //         if r.is_ok() {
                            //             let r2 = snd.send(String::from("Resumed"));
                            //             if let Err(e) = r2 {
                            //                 log.log(&format!("Error sending resume: {}\n", e)).await;
                            //             }
                            //         } else {
                            //             log.log(&format!("Error resuming track: {}\n", r)).await;
                            //         }
                            //     } else {
                            //         let r = snd.send(String::from("Nothing is playing"));
                            //         if let Err(e) = r {
                            //             log.log(&format!("Error updating message: {}\n", e)).await;
                            //         }
                            //     }
                            // }
                            AudioPromiseCommand::Shuffle(shuffle) => {
                                let shuffle = shuffle.get_val(control.settings.shuffle);
                                if control.settings.shuffle != shuffle {
                                    control.settings.shuffle = shuffle;
                                    let r = snd.send(format!("Shuffle set to `{}`", control.settings.shuffle));
                                    if let Err(e) = r {
                                        log.log(&format!("Error updating message: {}\n", e)).await;
                                    }
                                } else {
                                    let r = snd.send(format!("Shuffle is already `{}`", control.settings.shuffle));
                                    if let Err(e) = r {
                                        log.log(&format!("Error updating message: {}\n", e)).await;
                                    }
                                }
                            }
                            // AudioPromiseCommand::Transcribe(transcribe, id) => {
                            //     if transcribed != transcribe {
                            //         transcribed = transcribe;
                            //         let r = snd.send(format!("Transcribe set to `{}`", transcribed));
                            //         if let Err(e) = r {
                            //             log.log(&format!("Error updating message: {}\n", e)).await;
                            //         }
                            //         // if transcribed {
                            //         //     msg.last_processed = Some(id);
                            //         // }
                            //     } else {
                            //         let r =
                            //             snd.send(format!("Transcribe is already `{}`", transcribed));
                            //         if let Err(e) = r {
                            //             log.log(&format!("Error updating message: {}\n", e)).await;
                            //         }
                            //     }
                            // }
                            AudioPromiseCommand::Autoplay(autoplay) => {
                                let autoplay = autoplay.get_val(control.settings.autoplay);
                                if control.settings.autoplay != autoplay {
                                    control.settings.autoplay = autoplay;
                                    let r = snd.send(format!("Autoplay set to `{}`", control.settings.autoplay));
                                    if let Err(e) = r {
                                        log.log(&format!("Error updating message: {}\n", e)).await;
                                    }
                                } else {
                                    let r = snd.send(format!("Autoplay is already `{}`", control.settings.autoplay));
                                    if let Err(e) = r {
                                        log.log(&format!("Error updating message: {}\n", e)).await;
                                    }
                                }
                            }
                            AudioPromiseCommand::Loop(looped) => {
                                let looped = looped.get_val(control.settings.looped);
                                if control.settings.looped != looped {
                                    control.settings.looped = looped;
                                    let r = snd.send(format!("Loop set to `{}`", control.settings.looped));
                                    if let Err(e) = r {
                                        log.log(&format!("Error updating message: {}\n", e)).await;
                                    }
                                } else {
                                    let r = snd.send(format!("Loop is already `{}`", control.settings.looped));
                                    if let Err(e) = r {
                                        log.log(&format!("Error updating message: {}\n", e)).await;
                                    }
                                }
                            }
                            AudioPromiseCommand::Repeat(repeat) => {
                                let repeat = repeat.get_val(control.settings.repeat);
                                if control.settings.repeat != repeat {
                                    control.settings.repeat = repeat;
                                    let r = snd.send(format!("Repeat set to `{}`", control.settings.repeat));
                                    if let Err(e) = r {
                                        log.log(&format!("Error updating message: {}\n", e)).await;
                                    }
                                } else {
                                    let r = snd.send(format!("Repeat is already `{}`", control.settings.repeat));
                                    if let Err(e) = r {
                                        log.log(&format!("Error updating message: {}\n", e)).await;
                                    }
                                }
                            }
                            AudioPromiseCommand::Skip => {
                                if let Some(trackhandle) = trackhandle.as_mut() {
                                    let r = trackhandle.stop();

                                    if let Err(e) = r {
                                        log.log(&format!("Error skipping track: {}\n", e)).await;

                                    } else {
                                        let r2 = snd.send(String::from("Skipped"));
                                        if let Err(e) = r2 {
                                            log.log(&format!("Error sending skip: {}\n", e)).await;
                                        }
                                    }

                                } else {
                                    let r = snd.send(String::from("Nothing is playing"));
                                    if let Err(e) = r {
                                        log.log(&format!("Error updating message: {}\n", e)).await;
                                    }
                                }
                            }
                            AudioPromiseCommand::Volume(v) => {
                                let msg = if nothing_handle.is_some() {
                                    control.settings.set_radiovolume(v);
                                    format!("Radio volume set to `{}%`", control.settings.radiovolume() * 100.0)
                                } else {
                                    control.settings.set_volume(v);
                                    format!("Song volume set to `{}%`", control.settings.volume() * 100.0)
                                };

                                let r = snd.send(msg);
                                if let Err(e) = r {
                                    log.log(&format!("Error updating message: {}\n", e)).await;
                                }
                            }
                            AudioPromiseCommand::SpecificVolume(SpecificVolume::Volume(v)) => {
                                control.settings.set_volume(v);

                                let r = snd.send(format!("Song volume set to `{}%`", control.settings.volume() * 100.0));
                                if let Err(e) = r {
                                    log.log(&format!("Error updating message: {}\n", e)).await;
                                }
                            }
                            AudioPromiseCommand::SpecificVolume(SpecificVolume::RadioVolume(v)) => {
                                control.settings.set_radiovolume(v);

                                let r = snd.send(format!("Radio volume set to `{}%`", control.settings.radiovolume() * 100.0));
                                if let Err(e) = r {
                                    log.log(&format!("Error updating message: {}\n", e)).await;
                                }
                            }
                            AudioPromiseCommand::Remove(index) => {
                                let index = index - 1;
                                if index < queue.len() {
                                    let mut v = queue.remove(index);
                                    let r = v.delete();
                                    if let Err(r) = r {
                                        log.log(&format!("Error removing `{}`: {}\n", v.title, r)).await;
                                        let r =
                                            snd.send(format!("Error removing `{}`: {}", v.title, r));
                                        if let Err(e) = r {
                                            log.log(&format!("Error updating message: {}\n", e)).await;
                                        }
                                    } else {
                                        let r = snd.send(format!("Removed `{}`", v.title));
                                        if let Err(e) = r {
                                            log.log(&format!("Error updating message: {}\n", e)).await;
                                        }
                                    }
                                } else {
                                    let r = snd.send(format!(
                                        "Index out of range, max is `{}`",
                                        queue.len()
                                    ));
                                    if let Err(e) = r {
                                        log.log(&format!("Error updating message: {}\n", e)).await;
                                    }
                                }
                            }
                            AudioPromiseCommand::SetBitrate(bitrate) => {
                                let mut cl = control.call.lock().await;
                                control.settings.bitrate = bitrate;

                                match bitrate {
                                    OrAuto::Auto => {
                                        cl.set_bitrate(songbird::driver::Bitrate::Auto);
                                    },
                                    OrAuto::Specific(bitrate) => {
                                        cl.set_bitrate(songbird::driver::Bitrate::BitsPerSecond(bitrate as i32));
                                    }
                                }
                                // cl.set_bitrate(songbird::driver::Bitrate::BitsPerSecond(bitrate as i32));
                                let r = snd.send(format!("Bitrate set to `{}`", bitrate));
                                if let Err(e) = r {
                                    log.log(&format!("Error updating message: {}\n", e)).await;
                                }
                            }
                        }
                    }
                    None => {
                        log.log("rx closed").await;
                        break;
                    }
                }
                // dispatch request to update message

                if let Some(embed) = last_embed.as_ref() {
                    last_settings = Some(control.settings.clone());
                    if let Err(e) = msg_updater.send((control.settings.clone(), embed.clone())).await {
                        log.log(&format!("Error sending update: {}\n", e)).await;
                    }
                }
            }
            _ = run_dur.tick() => {
                // log.log(&format!("REALLY LONG STRING FOR TESTING PURPOSES IN THE LOG {}", testint)).await;
                // testint += 1;
                if let Some(current) = current_track.as_mut() {
                    if let Some(thandle) = trackhandle.as_mut() {
                        let playmode = tokio::time::timeout(g_timeout_time, thandle.get_info()).await;

                        match playmode {
                            Ok(Err(e)) => {
                                if e == songbird::tracks::TrackError::Finished {
                                    let url = current_track.as_ref().and_then(|t| match t.video {
                                        VideoType::Disk(_) => None,
                                        VideoType::Url(ref y) => Some(y.url.clone()),
                                    });

                                    if control.settings.autoplay && queue.is_empty() {
                                        // get a new song to play next as well
                                        if let Some(url) = url {
                                            autoplay_thread = Some(tokio::spawn(async move {
                                                let r = match tokio::time::timeout(
                                                    Duration::from_secs(2),
                                                    crate::youtube::get_recommendations(url, 1),
                                                )
                                                .await
                                                {
                                                    Ok(r) => r,
                                                    Err(_) => {
                                                        return None;
                                                    }
                                                };

                                                let vid = match r.get(0) {
                                                    Some(v) => v,
                                                    None => {
                                                        return None;
                                                    }
                                                };

                                                vid.to_metavideo().await.ok()
                                            }));
                                        }
                                    };

                                    let mut t = None;
                                    mem::swap(&mut current_track, &mut t);
                                    if let Some(t) = t.as_mut() {
                                        if control.settings.repeat {
                                            queue.insert(0, t.clone());
                                        } else if control.settings.looped {
                                            queue.push(t.clone());
                                        } else {
                                            let r = t.delete();
                                            if let Err(e) = r {
                                                log.log(&format!("Error deleting video: {}\n", e)).await;
                                            }
                                        }
                                    }
                                    current_track = None;
                                    trackhandle = None;
                                    tts_handle = None;
                                } else {
                                    log.log(format!("playmode error: {:?}", playmode).as_str()).await;
                                }
                            }
                            Err(e) => {
                                log.log(&format!("Error getting track info, Timeout: {}\n", e)).await;
                            }
                            _ => {}
                        }
                    } else if let Some(tts) = tts_handle.as_mut() {
                        let r = tokio::time::timeout(g_timeout_time, tts.get_info()).await;

                        match r {
                            Ok(Ok(_)) => {},
                            Ok(Err(e)) => {
                                if e == songbird::tracks::TrackError::Finished {
                                    let calllock = control.call.try_lock();
                                    if let Ok(mut clock) = calllock {
                                        let r = match current.video.clone() {
                                            VideoType::Disk(v) => {
                                                tokio::time::timeout(
                                                    Duration::from_secs(30),
                                                    ffmpeg(&v.path),
                                                )
                                                .await
                                            }
                                            VideoType::Url(v) => {
                                                tokio::time::timeout(Duration::from_secs(30), ytdl(&v.url))
                                                    .await
                                            }
                                        };
                                        match r {
                                            Ok(Ok(src)) => {
                                                let (mut audio, handle) = create_player(src);
                                                audio.set_volume(control.settings.volume() as f32);
                                                clock.play(audio);
                                                trackhandle = Some(handle);
                                            },
                                            Ok(Err(e)) => {
                                                log.log(&format!(
                                                    "Error playing track: {}\n",
                                                    e
                                                )).await;
                                            }
                                            Err(e) => {
                                                log.log(&format!("Timeout procced: {}\n", e)).await;

                                            }
                                        }
                                    }
                                } else {
                                    log.log(&format!("Error getting tts info: {:?}\n", r)).await;
                                }
                            }
                            Err(e) => {
                                log.log(&format!("Error getting tts info: {}\n", e)).await;
                            }
                        }
                    } else {
                        let calllock = control.call.try_lock();
                        if let Ok(mut clock) = calllock {
                            #[cfg(feature = "tts")]
                            if let Some(tts) = current.ttsmsg.as_ref() {
                                let r = tokio::time::timeout(g_timeout_time, ffmpeg(&tts.path))
                                    .await;

                                match r {
                                    Ok(Ok(r)) => {
                                        let (mut audio, handle) = create_player(r);
                                        audio.set_volume(control.settings.volume() as f32);
                                        clock.play(audio);
                                        tts_handle = Some(handle);
                                    },
                                    Ok(Err(e)) => {
                                        if let Ok(h) = ytdl(crate::Config::get().bumper_url.as_str()).await {
                                            let (mut audio, handle) = create_player(h);
                                            audio.set_volume(control.settings.volume() as f32);
                                            clock.play(audio);
                                            tts_handle = Some(handle);
                                        } else {
                                            log.log(&format!("Error playing track: {}\n", e)).await;
                                        }
                                    }
                                    Err(e) => {
                                        if let Ok(h) = ytdl(crate::Config::get().bumper_url.as_str()).await {
                                            let (mut audio, handle) = create_player(h);
                                            audio.set_volume(control.settings.volume() as f32);
                                            clock.play(audio);
                                            tts_handle = Some(handle);
                                        } else {
                                            log.log(&format!("Timeout procced: {}\n", e)).await;
                                        }
                                    }
                                }
                            } else {
                                // let (mut audio, handle) = create_player(
                                //     ytdl(crate::Config::get().bumper_url.as_str())
                                //         .await
                                // );
                                // audio.set_volume(control.settings.volume() as f32);
                                // clock.play(audio);
                                // tts_handle = Some(handle);

                                if let Ok(h) = ytdl(crate::Config::get().bumper_url.as_str()).await {
                                    let (mut audio, handle) = create_player(h);
                                    audio.set_volume(control.settings.volume() as f32);
                                    clock.play(audio);
                                    tts_handle = Some(handle);
                                }
                            }
                            #[cfg(not(feature = "tts"))]
                            {
                                // let (mut audio, handle) = create_player(
                                //     ytdl(crate::Config::get().bumper_url.as_str())
                                //         .await
                                // );
                                // audio.set_volume(control.settings.volume() as f32);
                                // clock.play(audio);
                                // tts_handle = Some(handle);

                                if let Ok(h) = ytdl(crate::Config::get().bumper_url.as_str()) {
                                    let (mut audio, handle) = create_player(h);
                                    audio.set_volume(control.settings.volume() as f32);
                                    clock.play(audio);
                                    tts_handle = Some(handle);
                                }
                            }
                        }
                    }
                } else if !queue.is_empty() {
                    let index = if control.settings.shuffle {
                        let maxnum = if control.settings.looped {
                            queue.len() - 1
                        } else {
                            queue.len()
                        };

                        if maxnum > 0 {
                            rand::thread_rng().gen_range(0..maxnum)
                        } else {
                            0
                        }

                        // rand::thread_rng().gen_range(0..queue.len())
                    } else {
                        0
                    };
                    let vid = queue.remove(index);
                    current_track = Some(vid);
                }

                if queue.is_empty() && control.settings.autoplay && autoplay_thread.is_none() {
                    let url = current_track.as_ref().and_then(|t| match t.video {
                        VideoType::Disk(_) => None,
                        VideoType::Url(ref y) => Some(y.url.clone()),
                    });
                    if let Some(url) = url {
                        autoplay_thread = Some(tokio::spawn(async move {
                            let r = match tokio::time::timeout(
                                Duration::from_secs(2),
                                crate::youtube::get_recommendations(url, 1),
                            )
                            .await
                            {
                                Ok(r) => r,
                                Err(_) => {
                                    return None;
                                }
                            };

                            let vid = match r.get(0) {
                                Some(v) => v,
                                None => {
                                    return None;
                                }
                            };

                            vid.to_metavideo().await.ok()
                        }));
                    }
                }

                let mut embed = EmbedData::default();

                if let Some(ref mut azuracast) = azuracast {
                    if let Ok(d) = tokio::time::timeout(g_timeout_time, azuracast.fast_data()).await {
                        match d {
                            Ok(d) => {
                                data = Some(d);
                            }
                            Err(e) => {
                                log.log(&format!("Error getting azuracast data: {}\n", e)).await;
                            }
                        }
                    }
                }

                if queue.is_empty() && current_track.is_none() {
                    control.settings.pause = false;
                    if nothing_handle.is_none() {
                        let r = if let Some(uri) = control.nothing_uri.clone() {
                            tokio::time::timeout(g_timeout_time, Restartable::ffmpeg(uri, false)).await
                        } else {
                            // tokio::time::timeout(g_timeout_time, Restartable::ytdl("https://www.youtube.com/watch?v=xy_NKN75Jhw", false)).await
                            tokio::time::timeout(
                                Duration::from_secs(2),
                                Restartable::ffmpeg(crate::Config::get().idle_url, false),
                            )
                            .await
                        };

                        match r {
                            Ok(Ok(src)) => {
                                let (mut audio, handle) = create_player(src.into());
                                let calllock = control.call.try_lock();
                                match calllock {
                                    Ok(mut clock) => {
                                        if let Err(e) = audio.set_loops(LoopState::Infinite) {
                                            log.log(&format!("Error setting loop: {}\n", e)).await;
                                        }
                                        // audio.set_volume(volume / 5.);
                                        audio.set_volume(control.settings.radiovolume() as f32);
                                        clock.play(audio);
                                        nothing_handle = Some(handle);
                                    },
                                    Err(e) => {
                                        log.log(&format!(
                                            "Error locking call: {}\n",
                                            e
                                        )).await;
                                    }
                                }
                            }
                            Ok(Err(e)) => {
                                log.log(&format!(
                                    "Error playing nothing: {}\nfile_uri: {:?}",
                                    e,
                                    control.nothing_uri.clone()
                                )).await;
                            }
                            Err(e) => {
                                log.log(&format!(
                                    "Error playing nothing: {}\nfile_uri: {:?}",
                                    e,
                                    control.nothing_uri.clone()
                                )).await;
                            }
                        }
                    }
                    let mut possible_body = "Queue is empty, use `/play` to play something!".to_owned();

                    if let Some(ref data) = data {
                        possible_body = format!(
                            "{}\nIn the meantime, enjoy these fine tunes from `{}`",
                            possible_body, data.station.name,
                        );
                        embed.fields.push((
                            "Now Playing".to_owned(),
                            format!(
                                "{} - {}",
                                data.now_playing.song.title, data.now_playing.song.artist
                            ),
                            false,
                        ));
                        embed.thumbnail = Some(data.now_playing.song.art.clone());

                        embed.fields.push((
                            "Next Up:".to_string(),
                            format!(
                                "{} - {}",
                                data.playing_next.song.title, data.playing_next.song.artist
                            ),
                            true,
                        ));
                    };
                    if !possible_body.is_empty() {
                        embed.body = Some(possible_body);
                    }
                    // let r = tokio::time::timeout(g_timeout_time, msg.update(&msgtext)).await;
                    // if let Ok(r) = r {
                    //     if let Err(e) = r {
                    //         log.log(&format!.await(
                    //             "Error updating message: {:?}. probably got deleted, sending a new one",
                    //             e
                    //         ));
                    //         let j = format!("{:?}", e).to_lowercase();

                    //         if j.contains("unknown message") {
                    //             let r = tokio::time::timeout(g_timeout_time, msg.send_new()).await;
                    //             if let Ok(r) = r {
                    //                 if let Err(e) = r {
                    //                     log.log(&format!("Error sending new message: {:?}", e)).await;
                    //                 }
                    //             } else {
                    //                 log.log(&format!.await(
                    //                     "Error sending new message: {:?}",
                    //                     r
                    //                 ));
                    //             }
                    //         }
                    //     }
                    // } else {
                    //     log.log(&format!("Error updating message: {}\n", r)).await;
                    // }
                    embed.color = Some(Colour::from_rgb(184, 29, 19));
                } else {
                    if let Some(ref data) = data {
                        embed.author = Some(format!(
                            "{} - {} playing on {}",
                            data.now_playing.song.title, data.now_playing.song.artist, data.station.name
                        ));
                        embed.author_icon_url = Some(data.now_playing.song.art.clone());
                    }
                    if nothing_handle.is_some() {
                        if let Some(handle) = nothing_handle.as_mut() {
                            let r = handle.stop();
                            if let Err(e) = r {
                                log.log(&format!("Error stopping nothing: {}\n", e)).await;
                            }
                        }
                        nothing_handle = None;

                        // tokio::time::sleep(Duration::from_millis(50)).await;
                    }

                    // let mut message = String::new();
                    let mut possible_body = String::new();
                    // if control.settings.pause {
                    //     possible_body.log("<:pause:1038954686402789518>");
                    // }
                    // if control.settings.repeat {
                    //     possible_body.log("<:Sliderfix09:1038954711585390654>");
                    // }
                    // if control.settings.looped {
                    //     possible_body.log("<:loop:1038954691318526024>");
                    // }
                    // if control.settings.shuffle {
                    //     possible_body.log("<:shuffle:1038954690114764880>");
                    // }
                    // if control.settings.autoplay {
                    //     possible_body.log("<a:speEEEeeeEEEeeen:1108745209451397171>");
                    // }
                    if let Some(t) = current_track.as_ref() {
                        // message.log(&format!("Playing: `{}` ", t.title));
                        embed
                            .fields
                            .push(("Now Playing".to_owned(), t.title.clone(), false));
                        if let Some(handle) = trackhandle.as_ref() {
                            let info = tokio::time::timeout(g_timeout_time, handle.get_info()).await;

                            match info {
                                Ok(Ok(info)) => {
                                    match t.video.clone() {
                                        VideoType::Disk(v) => {
                                            let percent_done = info.position.as_secs_f64() / v.duration;
                                            // let bar = (percent_done * 20.0).round() as usize;
                                            // message.log(&format!("\n`[{:20}]`", "=".repeat(bar)));
                                            possible_body
                                                .push_str(&format!("\n{}", get_bar(percent_done, 20)));
                                        }
                                        VideoType::Url(_) => {}
                                    };
                                }
                                Ok(Err(e)) => {
                                    log.log(&format!(
                                        "Error getting track info: {}\n",
                                        e
                                    )).await;
                                }
                                Err(e) => {
                                    log.log(&format!(
                                        "Error getting track info: {}\n",
                                        e
                                    )).await;
                                }
                            }
                        }

                        // message.push('\n');

                        if !queue.is_empty() {
                            // message.log("Queue:\n");
                            // message.log("```\n");
                            for (i, track) in queue.iter().enumerate() {
                                // message.log(&format!("{}. {}\n", i + 1, track.title));
                                embed
                                    .fields
                                    .push((format!("{}:", i + 1), track.title.clone(), true));
                            }
                            // message.log("```");
                        }
                        embed.color = Some(Colour::from_rgb(0, 132, 80));
                    } else {
                        // message.log("Queue:\n");
                        // message.log("```\n");
                        for (i, track) in queue.iter().enumerate() {
                            // message.log(&format!("{}. {}\n", i + 1, track.title));
                            embed
                                .fields
                                .push((format!("{}:", i + 1), track.title.clone(), true));
                        }
                        // message.log("```");
                        embed.color = Some(Colour::from_rgb(253, 218, 22));
                    }
                    if !possible_body.is_empty() {
                        embed.body = Some(possible_body);
                    }
                }
                // if transcribed {
                //     match msg.get_messages().await {
                //         Ok(msgs) => {
                //             if let Err(e) = transcribe_handler.update(msgs).await {
                //                 log.log(&format!("Error updating transcribe: {}\n", e)).await;
                //             }
                //         }
                //         Err(e) => {
                //             log.log(&format!("Error getting messages: {}\n", e)).await;
                //         }
                //     }
                // }

                // let r = tokio::time::timeout(g_timeout_time, msg.update(embed)).await;

                let send_now = match last_embed {
                    Some(ref last_embed) => last_embed != &embed,
                    None => true,
                } || match last_settings {
                    Some(ref last_settings) => last_settings != &control.settings,
                    None => true,
                };

                if send_now {
                    last_embed = Some(embed.clone());
                    last_settings = Some(control.settings.clone());
                    if let Err(e) = msg_updater.send((control.settings.clone(), embed)).await {
                        log.log(&format!("Error sending update: {}\n", e)).await;
                    }
                }

                // match r {
                //     Ok(Ok(_)) => {}
                //     Ok(Err(e)) => {
                //         log.log(&format!("Error updating message: {}\n", e)).await;
                //     }
                //     Err(e) => {
                //         log.log(&format!("Error updating message, Timeout: {}\n", e)).await;
                //     }
                // }

                if let Some(handle) = trackhandle.as_mut() {
                    let r = handle.set_volume(control.settings.volume() as f32);
                    if let Err(e) = r {
                        let s = format!("Error setting volume: {}\n", e);
                        if !s.contains("track ended") {
                            log.log(&s).await;
                        }
                    }
                }
                if let Some(handle) = nothing_handle.as_mut() {
                    // let r = handle.set_volume(volume / 5.);
                    let r = handle.set_volume(control.settings.radiovolume() as f32);
                    if let Err(e) = r {
                        log.log(&format!("Error setting volume: {}\n", e)).await;
                    }
                }

                // get messages from channel? something something uh oh


                // no longer printing log, maybe add button to show it?
                // if !log.is_empty() {
                //     log.clear();
                // }

                let mut finished = false;
                if let Some(h) = &autoplay_thread {
                    if h.is_finished() {
                        finished = true;
                    }
                }

                if finished {
                    let thread = autoplay_thread.take();
                    if let Some(thread) = thread {
                        let res = thread.await;
                        match res {
                            Err(e) => {
                                log.log(&format!("Error in autoplay thread: {}\n", e)).await;
                            }
                            Ok(v) => {
                                if let Some(v) = v {
                                    queue.push(v);
                                }
                            }
                        };
                        autoplay_thread = None;
                    }
                }

                // tokio::time::sleep(std::time::Duration::from_millis(looptime)).await;
                {
                    match tokio::time::timeout(g_timeout_time, control.call.lock()).await {
                        Ok(call) => {
                            if let Some(_c) = call.current_connection() {
                                // maybe do something someday
                            } else {
                                log.log("No connection, breaking\n").await;
                                control.brk = true;
                            }
                        }
                        Err(_) => {
                            log.log(&format!("Call lock timed out")).await;
                        }
                    }
                }
                if control.brk {
                    break;
                }
            }
        }
    }
    log.log("SHUTTING DOWN").await;
    // transcribe_handler.stop().await;

    let (returner, gimme) =
        tokio::sync::oneshot::channel::<serenity::futures::channel::mpsc::Receiver<RawMessage>>();
    if killsubthread.send(returner).is_err() {
        log.log("Error sending killsubthread").await;
    }

    if let Err(e) = subthread.await {
        log.log(&format!("Error joining subthread: {}\n", e)).await;
    }

    match gimme.await {
        Ok(gimme) => {
            if let Err(e) = control.transcribe.lock().await.unlock(gimme).await {
                log.log(&format!("Error unlocking transcribe: {}\n", e))
                    .await;
            }
        }
        Err(e) => {
            log.log(&format!("Error getting ttsrx: {}\n", e)).await;
        }
    }
    let mut calllock = control.call.lock().await;
    control.rx.close();
    calllock.stop();
    if let Err(e) = calllock.leave().await {
        log.log(&format!("Error leaving voice channel: {}\n", e))
            .await;
    }
    if let Some(t) = trackhandle.as_mut() {
        let r = t.stop();
        if let Err(e) = r {
            log.log(&format!("Error stopping track: {}\n", e)).await;
        }
    }
    if let Some(t) = current_track.as_mut() {
        let mut tries = 10;
        tokio::time::sleep(Duration::from_millis(100)).await;
        while t.delete().is_err() {
            tokio::time::sleep(Duration::from_millis(100)).await;
            tries -= 1;
            log.log(&format!("Failed to delete file, {} tries left", tries))
                .await;
            if tries == 0 {
                log.log("Failed to delete file, giving up").await;
                break;
            }
        }
    }
    for video in queue.iter_mut() {
        let r = video.delete();
        if let Err(e) = r {
            log.log(&format!("Error deleting video: {}\n", e)).await;
        }
    }
    let r = calllock.leave().await;
    if let Err(e) = r {
        log.log(&format!("Error leaving voice channel: {}\n", e))
            .await;
    }

    if killmsg.send(()).is_err() {
        log.log("Error sending killmsg").await;
    } else if let Err(e) = msghandler.await {
        log.log(&format!("Error joining msghandler: {}\n", e)).await;
    }

    log.log("Gracefully exited").await;

    if !log.is_empty().await {
        eprintln!("Final log:\n{}", log.get().await);
    }
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

#[derive(Debug, PartialEq, Clone)]
pub struct EmbedData {
    author: Option<String>,
    author_url: Option<String>,
    author_icon_url: Option<String>,
    color: Option<Colour>,
    body: Option<String>,
    fields: Vec25<(String, String, bool)>,
    thumbnail: Option<String>,
    footer: Option<String>,
}
impl EmbedData {
    pub(crate) fn write_into(&self, e: &mut serenity::builder::CreateEmbed) {
        e.author(|a| {
            if let Some(ref author) = self.author {
                a.name(author);
            }
            if let Some(ref author_url) = self.author_url {
                a.url(author_url);
            }
            if let Some(ref author_icon_url) = self.author_icon_url {
                a.icon_url(author_icon_url);
            }
            a
        });
        if let Some(color) = self.color {
            e.color(color);
        }
        if let Some(ref body) = self.body {
            e.description(body);
        }
        for (name, value, inline) in self.fields.0.iter() {
            e.field(name, value, *inline);
        }
        if let Some(ref thumbnail) = self.thumbnail {
            e.thumbnail(thumbnail);
        }
        if let Some(ref footer) = self.footer {
            e.footer(|f| f.text(footer));
        }
    }
}

impl Default for EmbedData {
    fn default() -> Self {
        Self {
            author: Some("Invite me to your server!".to_owned()),
            author_url: Some("https://discord.com/oauth2/authorize?client_id=1035364346471133194&permissions=274881349696&scope=bot".to_owned()),
            author_icon_url: None,
            color: Some(Colour::from_rgb(0, 0, 0)),
            body: None,
            fields: Vec25::new(),
            thumbnail: None,
            footer: Some("Type /help for help".to_owned()),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct Vec25<T>(Vec<T>);

impl<T> Vec25<T> {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    pub fn push(&mut self, item: T) {
        if self.0.len() < 25 {
            self.0.push(item);
        }
    }
}

#[derive(Clone)]
pub struct Log {
    log: Arc<Mutex<(Vec<LogString>, Instant)>>,
}

impl Log {
    pub fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new((Vec::new(), Instant::now()))),
        }
    }
    pub async fn log(&self, s: &str) {
        let mut d = self.log.lock().await;
        let t = d.1.elapsed();
        d.0.push(LogString {
            s: s.to_owned(),
            time: t,
        });
    }
    pub async fn get(&self) -> String {
        let d = self.log.lock().await;
        d.0.iter()
            .map(|l| l.pretty())
            .collect::<Vec<String>>()
            .join("\n")
    }
    pub async fn clear_until(&self, rm: usize) {
        let mut d = self.log.lock().await;
        if rm >= d.0.len() {
            d.0.clear();
            return;
        }
        d.0.drain(0..=rm);
    }

    pub async fn get_chunks_4k(&self) -> Vec<ChunkOfLog> {
        let d = self.log.lock().await;
        // basically the same as get, except start a new string if adding the next logstring would make the current string longer than 4k
        let mut s = ChunkOfLog {
            start: 0,
            s: String::new(),
            end: 0,
        };
        let mut v = Vec::new();
        for (i, l) in d.0.iter().enumerate() {
            let pretty = l.pretty() + "\n";
            if s.s.len() + pretty.len() > 4000 {
                s.end = i;
                s.s = s.s.trim().to_owned();
                v.push(s);
                s = ChunkOfLog {
                    start: i + 1,
                    s: String::new(),
                    end: 0,
                };
            }
            s.s.push_str(&pretty);
        }
        s.end = d.0.len();
        v.push(s);
        v
    }
    // pub async fn clear(&self) {
    //     let mut d = self.log.lock().await;
    //     d.0.clear();
    // }
    pub async fn is_empty(&self) -> bool {
        let d = self.log.lock().await;
        d.0.is_empty()
    }
}

pub struct ChunkOfLog {
    pub start: usize,
    pub s: String,
    pub end: usize,
}

pub struct LogString {
    s: String,
    time: Duration,
}

impl LogString {
    pub fn pretty(&self) -> String {
        let mut s = String::new();
        for line in self.s.lines() {
            s.push_str(&format!("[{:?}] {}", self.time, line));
        }
        s
    }
}
