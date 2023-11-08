use rand::Rng;

use serenity::futures::{SinkExt, StreamExt};
use serenity::prelude::Mutex;
use serenity::utils::Colour;
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

use crate::commands::music::{
    AudioPromiseCommand, MetaVideo, RawMessage, SpecificVolume, VideoType,
};
use crate::radio::AzuraCast;

use super::transcribe::TranscribeChannelHandler;
use super::MessageReference;

pub struct ControlData {
    pub call: Arc<Mutex<Call>>,
    pub rx: mpsc::UnboundedReceiver<(mpsc::UnboundedSender<String>, AudioPromiseCommand)>,
    pub msg: MessageReference,
    pub nothing_uri: Option<PathBuf>,
    pub transcribe: Arc<Mutex<TranscribeChannelHandler>>,
    pub settings: SettingsData,
    pub brk: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct SettingsData {
    // READ ONLY?
    pub volume: f64,
    pub radiovolume: f64,
    pub bitrate: Option<i64>,

    // CLICKABLE
    pub autoplay: bool,
    pub looped: bool,
    pub repeat: bool,
    pub shuffle: bool,
    pub pause: bool,
}

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            volume: 1.0,
            radiovolume: 0.33,
            autoplay: false,
            looped: false,
            repeat: false,
            shuffle: false,
            pause: false,
            bitrate: None,
        }
    }
}

pub async fn the_lüüp(
    rawcall: Arc<Mutex<Call>>,
    rawrx: mpsc::UnboundedReceiver<(mpsc::UnboundedSender<String>, AudioPromiseCommand)>,
    rawmsg: MessageReference,
    rawlooptime: u64,
    rawnothing_uri: Option<PathBuf>,
    rawtranscribe: Arc<Mutex<TranscribeChannelHandler>>,
) {
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
        let mut cl = control.call.lock().await;
        // cl.set_bitrate(Bitrate::BitsPerSecond(6_000));
        cl.set_bitrate(songbird::driver::Bitrate::Auto);
        match cl.deafen(true).await {
            Ok(_) => {}
            Err(e) => {
                println!("Error deafening: {:?}", e);
            }
        };
    }
    let (mut msg_updater, update_msg) =
        serenity::futures::channel::mpsc::channel::<(SettingsData, EmbedData)>(8);
    let (killmsg, killrx) = tokio::sync::oneshot::channel::<()>();

    let msghandler = {
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

                        // println!("Updating message");
                        if let Some(shakesbutt) = shakesbutt {

                            let mut shakesbutt = shakesbutt;
                            while let Ok(Some(u)) = update_msg.try_next() {
                                shakesbutt = u;
                            }
                            // println!("Updating message");
                            let r = msg.update(shakesbutt.0, shakesbutt.1).await;
                            if let Err(e) = r {
                                println!("Error updating message: {}", e);
                            }
                        } else {
                            println!("Error getting shakesbutt");
                            break;
                        }
                    }
                }
            }
            if let Err(e) = msg.delete().await {
                println!("Error deleting message: {}", e);
            }
        })
    };

    let mut log = String::new();
    let mut trackhandle: Option<TrackHandle> = None;
    let mut queue: Vec<MetaVideo> = Vec::new();
    // let mut paused = false;
    // let mut repeated = false;
    // let mut looped = false;
    // let mut transcribed = false;
    let mut last_embed = None;
    let mut last_settings = None;
    // let mut shuffled = false;
    let mut nothing_handle: Option<TrackHandle> = None;
    let mut tts_handle: Option<TrackHandle> = None;
    // let mut volume = 1.0;
    // let mut radiovolume = 0.33;
    // let mut autoplay = false;
    let mut autoplay_thread: Option<tokio::task::JoinHandle<Option<MetaVideo>>> = None;
    let mut azuracast = match crate::Config::get().api_url {
        Some(ref url) => AzuraCast::new(url).await.ok(),
        None => None,
    };
    // let mut brk = false;

    // let mut transcribe_handler = super::transcribe::Holder::new(Arc::clone(&control.call));
    // let mut transcribe_handler =
    //     super::transcribe::MetaTranscribeHandler::new(Arc::clone(&_t_arcmutex));
    let mut data = None;
    let g_timeout_time = Duration::from_millis(10000);

    let mut current_track: Option<MetaVideo> = None;

    let ttsrx = control.transcribe.lock().await.lock().unwrap();
    let ttshandler = super::transcribe::Handler::new(Arc::clone(&control.call));

    // psuedocode

    // spawn thread for handling tts with ttshandler, listening for messages, sending them in, etc. provide a "kill" oneshot for shutting down the thread and stopping the ttshandler, the oneshot will contain a oneshot that can be used to return the RawMessage Sender back to us to deregister it

    let (killsubthread, bekilled) = tokio::sync::oneshot::channel::<
        tokio::sync::oneshot::Sender<serenity::futures::channel::mpsc::Receiver<RawMessage>>,
    >();

    let subthread = tokio::task::spawn(async move {
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
                            println!("Error returning ttsrx AHHHHH");
                        };
                    } else {
                        // parent thread died, so we should too :(
                    }
                    break;
                }
                msg = ttsrx.next() => {
                    if let Some(msg) = msg {
                        if let Err(e) = ttshandler.update(vec![msg]).await {
                            println!("Error updating tts: {}", e);
                        }
                    }
                }
                _ = interv.tick() => {
                    if let Err(e) = ttshandler.shift().await {
                        println!("Error shifting tts: {}", e);
                    }
                }
            }
        }
    });

    let mut run_dur = tokio::time::interval(tokio::time::Duration::from_millis(rawlooptime));

    loop {
        tokio::select! {
            t = control.rx.next() => {
                match t {
                    Some((snd, command)) => {
                        // println!("Got command: {:?}", command);
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
                                control.brk = true;
                            }
                            AudioPromiseCommand::Pause => {
                                if let Some(trackhandle) = trackhandle.as_mut() {
                                    control.settings.pause = true;
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
                                    control.settings.pause = false;
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
                                if control.settings.shuffle != shuffle {
                                    control.settings.shuffle = shuffle;
                                    let r = snd.unbounded_send(format!("Shuffle set to `{}`", control.settings.shuffle));
                                    if let Err(e) = r {
                                        log.push_str(&format!("Error updating message: {}\r", e));
                                    }
                                } else {
                                    let r = snd.unbounded_send(format!("Shuffle is already `{}`", control.settings.shuffle));
                                    if let Err(e) = r {
                                        log.push_str(&format!("Error updating message: {}\r", e));
                                    }
                                }
                            }
                            // AudioPromiseCommand::Transcribe(transcribe, id) => {
                            //     if transcribed != transcribe {
                            //         transcribed = transcribe;
                            //         let r = snd.unbounded_send(format!("Transcribe set to `{}`", transcribed));
                            //         if let Err(e) = r {
                            //             log.push_str(&format!("Error updating message: {}\r", e));
                            //         }
                            //         // if transcribed {
                            //         //     msg.last_processed = Some(id);
                            //         // }
                            //     } else {
                            //         let r =
                            //             snd.unbounded_send(format!("Transcribe is already `{}`", transcribed));
                            //         if let Err(e) = r {
                            //             log.push_str(&format!("Error updating message: {}\r", e));
                            //         }
                            //     }
                            // }
                            AudioPromiseCommand::Autoplay(autoplayi) => {
                                if control.settings.autoplay != autoplayi {
                                    control.settings.autoplay = autoplayi;
                                    let r = snd.unbounded_send(format!("Autoplay set to `{}`", control.settings.autoplay));
                                    if let Err(e) = r {
                                        log.push_str(&format!("Error updating message: {}\r", e));
                                    }
                                } else {
                                    let r = snd.unbounded_send(format!("Autoplay is already `{}`", control.settings.autoplay));
                                    if let Err(e) = r {
                                        log.push_str(&format!("Error updating message: {}\r", e));
                                    }
                                }
                            }
                            AudioPromiseCommand::Loop(loopi) => {
                                if control.settings.looped != loopi {
                                    control.settings.looped = loopi;
                                    let r = snd.unbounded_send(format!("Loop set to `{}`", control.settings.looped));
                                    if let Err(e) = r {
                                        log.push_str(&format!("Error updating message: {}\r", e));
                                    }
                                } else {
                                    let r = snd.unbounded_send(format!("Loop is already `{}`", control.settings.looped));
                                    if let Err(e) = r {
                                        log.push_str(&format!("Error updating message: {}\r", e));
                                    }
                                }
                            }
                            AudioPromiseCommand::Repeat(repeati) => {
                                if control.settings.repeat != repeati {
                                    control.settings.repeat = repeati;
                                    let r = snd.unbounded_send(format!("Repeat set to `{}`", control.settings.repeat));
                                    if let Err(e) = r {
                                        log.push_str(&format!("Error updating message: {}\r", e));
                                    }
                                } else {
                                    let r = snd.unbounded_send(format!("Repeat is already `{}`", control.settings.repeat));
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
                                let msg = if nothing_handle.is_some() {
                                    control.settings.radiovolume = v;
                                    format!("Radio volume set to `{}%`", control.settings.radiovolume * 100.0)
                                } else {
                                    control.settings.volume = v;
                                    format!("Song volume set to `{}%`", control.settings.volume * 100.0)
                                };

                                let r = snd.unbounded_send(msg);
                                if let Err(e) = r {
                                    log.push_str(&format!("Error updating message: {}\r", e));
                                }
                            }
                            AudioPromiseCommand::SpecificVolume(SpecificVolume::Volume(v)) => {
                                control.settings.volume = v;

                                let r = snd.unbounded_send(format!("Song volume set to `{}%`", control.settings.volume * 100.0));
                                if let Err(e) = r {
                                    log.push_str(&format!("Error updating message: {}\r", e));
                                }
                            }
                            AudioPromiseCommand::SpecificVolume(SpecificVolume::RadioVolume(v)) => {
                                control.settings.radiovolume = v;

                                let r = snd.unbounded_send(format!("Radio volume set to `{}%`", control.settings.radiovolume * 100.0));
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
                            AudioPromiseCommand::SetBitrate(bitrate) => {
                                let mut cl = control.call.lock().await;
                                control.settings.bitrate = Some(bitrate);
                                cl.set_bitrate(songbird::driver::Bitrate::BitsPerSecond(bitrate as i32));
                                let r = snd.unbounded_send(format!("Bitrate set to `{}`", bitrate));
                                if let Err(e) = r {
                                    log.push_str(&format!("Error updating message: {}\r", e));
                                }
                            }
                        }
                    }
                    None => {
                        println!("rx closed");
                        break;
                    }
                }
            }
            _ = run_dur.tick() => {
                // println!("Tick");
                if let Some(current) = current_track.as_mut() {
                    if let Some(thandle) = trackhandle.as_mut() {
                        let playmode = tokio::time::timeout(g_timeout_time, thandle.get_info()).await;
                        if let Ok(playmode) = playmode {
                            if let Err(playmode) = playmode {
                                if playmode == songbird::tracks::TrackError::Finished {
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
                        let r = tokio::time::timeout(g_timeout_time, tts.get_info()).await;
                        if let Ok(r) = r {
                            if let Err(r) = r {
                                if r == songbird::tracks::TrackError::Finished {
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
                                        if let Ok(r) = r {
                                            if let Ok(src) = r {
                                                let (mut audio, handle) = create_player(src);
                                                audio.set_volume(control.settings.volume as f32);
                                                clock.play(audio);
                                                trackhandle = Some(handle);
                                            } else {
                                                log.push_str(&format!(
                                                    "Error playing track: {}\r",
                                                    r.unwrap_err()
                                                ));
                                            }
                                        } else {
                                            log.push_str(&format!("Timeout procced: {}\r", r.unwrap_err()));
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
                        let calllock = control.call.try_lock();
                        if let Ok(mut clock) = calllock {
                            #[cfg(feature = "tts")]
                            if let Some(tts) = current.ttsmsg.as_ref() {
                                let r = tokio::time::timeout(g_timeout_time, ffmpeg(&tts.path))
                                    .await
                                    .unwrap();
                                if let Ok(r) = r {
                                    let (mut audio, handle) = create_player(r);
                                    audio.set_volume(control.settings.volume as f32);
                                    clock.play(audio);
                                    tts_handle = Some(handle);
                                } else {
                                    let (mut audio, handle) = create_player(
                                        ytdl(crate::Config::get().bumper_url.as_str())
                                            .await
                                            .unwrap(),
                                    );
                                    audio.set_volume(control.settings.volume as f32);
                                    clock.play(audio);
                                    tts_handle = Some(handle);
                                }
                            } else {
                                let (mut audio, handle) = create_player(
                                    ytdl(crate::Config::get().bumper_url.as_str())
                                        .await
                                        .unwrap(),
                                );
                                audio.set_volume(control.settings.volume as f32);
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
                                audio.set_volume(control.settings.volume as f32);
                                clock.play(audio);
                                tts_handle = Some(handle);
                            }
                        }
                    }
                } else if !queue.is_empty() {
                    let index = if control.settings.shuffle {
                        rand::thread_rng().gen_range(0..queue.len())
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
                    if let Ok(d) = tokio::time::timeout(g_timeout_time, azuracast.slow_data()).await {
                        data = Some(d);
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

                        if let Ok(r) = r {
                            if let Ok(src) = r {
                                let (mut audio, handle) = create_player(src.into());
                                let calllock = control.call.try_lock();
                                if let Ok(mut clock) = calllock {
                                    audio.set_loops(LoopState::Infinite).unwrap();
                                    // audio.set_volume(volume / 5.);
                                    audio.set_volume(control.settings.radiovolume as f32);
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
                                    control.nothing_uri.clone()
                                ));
                            }
                        } else {
                            log.push_str(&format!(
                                "Error playing nothing: {}\nfile_uri: {:?}",
                                r.unwrap_err(),
                                control.nothing_uri.clone()
                            ));
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
                    //         log.push_str(&format!(
                    //             "Error updating message: {:?}. probably got deleted, sending a new one",
                    //             e
                    //         ));
                    //         let j = format!("{:?}", e).to_lowercase();

                    //         if j.contains("unknown message") {
                    //             let r = tokio::time::timeout(g_timeout_time, msg.send_new()).await;
                    //             if let Ok(r) = r {
                    //                 if let Err(e) = r {
                    //                     log.push_str(&format!("Error sending new message: {:?}", e));
                    //                 }
                    //             } else {
                    //                 log.push_str(&format!(
                    //                     "Error sending new message: {:?}",
                    //                     r.unwrap_err()
                    //                 ));
                    //             }
                    //         }
                    //     }
                    // } else {
                    //     log.push_str(&format!("Error updating message: {}\r", r.unwrap_err()));
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
                                log.push_str(&format!("Error stopping nothing: {}\n", e));
                            }
                        }
                        nothing_handle = None;

                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }

                    // let mut message = String::new();
                    let mut possible_body = String::new();
                    // if control.settings.pause {
                    //     possible_body.push_str("<:pause:1038954686402789518>");
                    // }
                    // if control.settings.repeat {
                    //     possible_body.push_str("<:Sliderfix09:1038954711585390654>");
                    // }
                    // if control.settings.looped {
                    //     possible_body.push_str("<:loop:1038954691318526024>");
                    // }
                    // if control.settings.shuffle {
                    //     possible_body.push_str("<:shuffle:1038954690114764880>");
                    // }
                    // if control.settings.autoplay {
                    //     possible_body.push_str("<a:speEEEeeeEEEeeen:1108745209451397171>");
                    // }
                    if let Some(t) = current_track.as_ref() {
                        // message.push_str(&format!("Playing: `{}` ", t.title));
                        embed
                            .fields
                            .push(("Now Playing".to_owned(), t.title.clone(), false));
                        if let Some(handle) = trackhandle.as_ref() {
                            let info = tokio::time::timeout(g_timeout_time, handle.get_info()).await;
                            if let Ok(info) = info {
                                if let Ok(info) = info {
                                    match t.video.clone() {
                                        VideoType::Disk(v) => {
                                            let percent_done = info.position.as_secs_f64() / v.duration;
                                            // println!("{}% done", percent_done);
                                            // let bar = (percent_done * 20.0).round() as usize;
                                            // message.push_str(&format!("\n`[{:20}]`", "=".repeat(bar)));
                                            possible_body
                                                .push_str(&format!("\n{}", get_bar(percent_done, 20)));
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

                        // message.push('\n');

                        if !queue.is_empty() {
                            // message.push_str("Queue:\n");
                            // message.push_str("```\n");
                            for (i, track) in queue.iter().enumerate() {
                                // message.push_str(&format!("{}. {}\n", i + 1, track.title));
                                embed
                                    .fields
                                    .push((format!("{}:", i + 1), track.title.clone(), true));
                            }
                            // message.push_str("```");
                        }
                        embed.color = Some(Colour::from_rgb(0, 132, 80));
                    } else {
                        // message.push_str("Queue:\n");
                        // message.push_str("```\n");
                        for (i, track) in queue.iter().enumerate() {
                            // message.push_str(&format!("{}. {}\n", i + 1, track.title));
                            embed
                                .fields
                                .push((format!("{}:", i + 1), track.title.clone(), true));
                        }
                        // message.push_str("```");
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
                //                 log.push_str(&format!("Error updating transcribe: {}\r", e));
                //             }
                //         }
                //         Err(e) => {
                //             log.push_str(&format!("Error getting messages: {}\r", e));
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
                    // println!("Sending update");
                    last_embed = Some(embed.clone());
                    last_settings = Some(control.settings.clone());
                    if let Err(e) = msg_updater.send((control.settings.clone(), embed)).await {
                        log.push_str(&format!("Error sending update: {}\r", e));
                    }
                }

                // match r {
                //     Ok(Ok(_)) => {}
                //     Ok(Err(e)) => {
                //         log.push_str(&format!("Error updating message: {}\r", e));
                //     }
                //     Err(e) => {
                //         log.push_str(&format!("Error updating message, Timeout: {}\r", e));
                //     }
                // }

                if let Some(handle) = trackhandle.as_mut() {
                    let r = handle.set_volume(control.settings.volume as f32);
                    if let Err(e) = r {
                        let s = format!("Error setting volume: {}\r", e);
                        if !s.contains("track ended") {
                            log.push_str(&s);
                        }
                    }
                }
                if let Some(handle) = nothing_handle.as_mut() {
                    // let r = handle.set_volume(volume / 5.);
                    let r = handle.set_volume(control.settings.radiovolume as f32);
                    if let Err(e) = r {
                        log.push_str(&format!("Error setting volume: {}\r", e));
                    }
                }

                // get messages from channel? something something uh oh

                if !log.is_empty() {
                    println!("{}", log);
                    log.clear();
                }

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
                                log.push_str(&format!("Error in autoplay thread: {}\n", e));
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
                                // println!("{:?}", c)
                            } else {
                                log.push_str("No connection, breaking\n");
                                control.brk = true;
                            }
                        }
                        Err(_) => {
                            log.push_str(&format!("Call lock timed out"));
                        }
                    }
                }
                if control.brk {
                    break;
                }
            }
        }
    }
    println!("SHUTTING DOWN!");
    // transcribe_handler.stop().await;

    let (returner, gimme) =
        tokio::sync::oneshot::channel::<serenity::futures::channel::mpsc::Receiver<RawMessage>>();
    if killsubthread.send(returner).is_err() {
        println!("Error sending killsubthread");
    }

    if let Err(e) = subthread.await {
        println!("Error joining subthread: {}", e);
    }

    match gimme.await {
        Ok(gimme) => {
            if let Err(e) = control.transcribe.lock().await.unlock(gimme).await {
                log.push_str(&format!("Error unlocking transcribe: {}\n", e));
            }
        }
        Err(e) => {
            println!("Error getting ttsrx: {}", e);
        }
    }
    let mut calllock = control.call.lock().await;
    control.rx.close();
    calllock.stop();
    if let Err(e) = calllock.leave().await {
        log.push_str(&format!("Error leaving voice channel: {}\n", e));
    }
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

    if killmsg.send(()).is_err() {
        log.push_str("Error sending killmsg");
    } else if let Err(e) = msghandler.await {
        log.push_str(&format!("Error joining msghandler: {}\n", e));
    }

    if !log.is_empty() {
        println!("Final log: {}", log);
    }
    // let r = msg.delete().await;
    // if let Err(e) = r {
    //     println!("Error deleting message: {}", e);
    // }

    // println!("Gracefully exited");
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
