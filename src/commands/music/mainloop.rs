use super::settingsdata::SettingsData;
use super::transcribe::TranscribeChannelHandler;
use super::{MessageReference, OrAuto};
use crate::commands::music::{
    AudioPromiseCommand, MetaVideo, RawMessage, SpecificVolume, VideoType,
};
use crate::radio::AzuraCast;
use rand::Rng;
use serenity::all::*;
use songbird::driver::Bitrate;
use songbird::error::ControlError;
use songbird::input::{File, YoutubeDl};
use songbird::tracks::TrackHandle;
use songbird::Call;
use std::mem;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::Instant;
pub struct ControlData {
    pub call: Arc<Mutex<Call>>,
    pub rx: mpsc::UnboundedReceiver<(oneshot::Sender<String>, AudioPromiseCommand)>,
    pub msg: MessageReference,
    pub nothing_uri: Option<PathBuf>,
    pub transcribe: Arc<Mutex<TranscribeChannelHandler>>,
    pub settings: SettingsData,
    pub brk: bool,
}
#[allow(clippy::too_many_arguments)]
pub async fn the_lüüp(
    rawcall: Arc<Mutex<Call>>,
    rawrx: mpsc::UnboundedReceiver<(oneshot::Sender<String>, AudioPromiseCommand)>,
    rawtx: mpsc::UnboundedSender<(oneshot::Sender<String>, AudioPromiseCommand)>,
    rawmsg: MessageReference,
    rawlooptime: u64,
    rawnothing_uri: Option<PathBuf>,
    rawtranscribe: Arc<Mutex<TranscribeChannelHandler>>,
    http: Arc<http::Http>,
    log_source: String,
) {
    let (transcription_thread, kill_transcription_thread, mut recv_new_transcription) = {
        let transcribe = crate::voice_events::VoiceDataManager::new(
            Arc::clone(&rawcall),
            Arc::clone(&http),
            rawtx,
        )
        .await;
        let (killtranscribe, transcribereturn) = tokio::sync::mpsc::channel::<()>(1);
        let (transsender, transcribed) = mpsc::unbounded_channel::<(String, UserId)>();
        let trans = {
            let call = Arc::clone(&rawcall);
            tokio::task::spawn(crate::voice_events::transcription_thread(
                transcribe,
                transcribereturn,
                transsender,
                call,
            ))
        };
        (trans, killtranscribe, transcribed)
    };
    let log = Log::new(log_source);
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
        log.log("Setting bitrate").await;
        cl.set_bitrate(Bitrate::Auto);
    }
    let (msg_updater, update_msg) = mpsc::channel::<(SettingsData, EmbedData)>(8);
    let (manually_send, send_msg) = mpsc::unbounded_channel::<(String, UserId)>();
    let (killmsg, killrx) = tokio::sync::oneshot::channel::<()>();
    log.log("Spawning message updater").await;
    let msghandler = {
        let logger = log.clone();
        let msg = control.msg.clone();
        tokio::task::spawn(async move {
            let mut msg = msg;
            let mut killrx = killrx;
            let mut update_msg = update_msg;
            let mut send_msg = send_msg;
            loop {
                tokio::select! {
                    _ = &mut killrx => {
                        break;
                    }
                    shakesbutt = update_msg.recv() => {
                        if let Some(shakesbutt) = shakesbutt {
                            let mut shakesbutt = shakesbutt;
                            while let Ok(u) = update_msg.try_recv() {
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
                    manmsg = send_msg.recv() => {
                        if let Some((manmsg, user)) = manmsg {
                            if let Err(e) = msg.send_manually(manmsg, user).await {
                                logger.log(&format!("Error sending message: {}", e)).await;
                            }
                        }
                    }
                }
            }
            if let Err(e) = msg.final_cleanup().await {
                logger
                    .log(&format!("Error cleaning up message: {}", e))
                    .await;
            }
        })
    };
    let mut trackhandle: Option<TrackHandle> = None;
    let mut queue: Vec<MetaVideo> = Vec::new();
    let mut last_embed: Option<EmbedData> = None;
    let mut last_settings = None;
    let mut nothing_handle: Option<TrackHandle> = None;
    let mut tts_handle: Option<TrackHandle> = None;
    let mut skipmarker = false;
    let g_timeout_time = Duration::from_millis(100);
    let mut autoplay_thread: Option<tokio::task::JoinHandle<Option<MetaVideo>>> = None;
    log.log("Creating azuracast").await;
    let mut azuracast = match crate::Config::get().api_url {
        Some(ref url) => AzuraCast::new(url, log.clone(), g_timeout_time).await.ok(),
        None => None,
    };
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
    let (killsubthread, bekilled) =
        tokio::sync::oneshot::channel::<tokio::sync::oneshot::Sender<mpsc::Receiver<RawMessage>>>();
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
                        }
                        break;
                    }
                    msg = ttsrx.recv() => {
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
    loop {
        control.settings.log_empty = log.is_empty().await;
        tokio::select! {
            t = control.rx.recv() => {
                match t {
                    Some((snd, command)) => match command {
                        AudioPromiseCommand::RetrieveLog(secret) => {
                            let chunks = log.get_chunks_4k().await;
                            let mut string_chunks = chunks
                                .iter()
                                .map(|c| (c.s.clone(), c.end))
                                .collect::<Vec<(String, usize)>>();
                            let end = if string_chunks.len() > 5 {
                                string_chunks.truncate(5);
                                chunks[4].end - 1
                            } else {
                                chunks.last().map(|e| e.end).unwrap_or(0)
                            };
                            let r = secret
                                .send(
                                    string_chunks
                                        .into_iter()
                                        .map(|(s, _)| s)
                                        .collect::<Vec<String>>(),
                                )
                                .await;
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
                        AudioPromiseCommand::Stop(delay) => {
                            let r = snd.send(String::from("Stopped"));
                            if let Err(e) = r {
                                log.log(&format!("Error sending stop: {}\n", e)).await;
                            }
                            control.brk = true;
                            if let Some(delay) = delay {
                                tokio::time::sleep(delay).await;
                            }
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
                                        let r2 =
                                            snd.send(format!("Paused set to `{}`", control.settings.pause));
                                        if let Err(e) = r2 {
                                            log.log(&format!("Error sending pause: {}\n", e)).await;
                                        }
                                    }
                                }
                            } else if let Err(e) = snd.send(String::from("Nothing is playing")) {
                                log.log(&format!("Error updating message: {}\n", e)).await;
                            }
                        }
                        AudioPromiseCommand::Shuffle(shuffle) => {
                            let shuffle = shuffle.get_val(control.settings.shuffle);
                            if control.settings.shuffle != shuffle {
                                control.settings.shuffle = shuffle;
                                let r = snd.send(format!("Shuffle set to `{}`", control.settings.shuffle));
                                if let Err(e) = r {
                                    log.log(&format!("Error updating message: {}\n", e)).await;
                                }
                            } else {
                                let r =
                                    snd.send(format!("Shuffle is already `{}`", control.settings.shuffle));
                                if let Err(e) = r {
                                    log.log(&format!("Error updating message: {}\n", e)).await;
                                }
                            }
                        }
                        AudioPromiseCommand::Autoplay(autoplay) => {
                            let autoplay = autoplay.get_val(control.settings.autoplay);
                            if control.settings.autoplay != autoplay {
                                control.settings.autoplay = autoplay;
                                let r =
                                    snd.send(format!("Autoplay set to `{}`", control.settings.autoplay));
                                if let Err(e) = r {
                                    log.log(&format!("Error updating message: {}\n", e)).await;
                                }
                            } else {
                                let r = snd.send(format!(
                                    "Autoplay is already `{}`",
                                    control.settings.autoplay
                                ));
                                if let Err(e) = r {
                                    log.log(&format!("Error updating message: {}\n", e)).await;
                                }
                            }
                        }
                        AudioPromiseCommand::ReadTitles(read_titles) => {
                            let read_titles = read_titles.get_val(control.settings.read_titles);
                            if control.settings.read_titles != read_titles {
                                control.settings.read_titles = read_titles;
                                let r = snd.send(format!(
                                    "Read titles set to `{}`",
                                    control.settings.read_titles
                                ));
                                if let Err(e) = r {
                                    log.log(&format!("Error updating message: {}\n", e)).await;
                                }
                            } else {
                                let r = snd.send(format!(
                                    "Read titles is already `{}`",
                                    control.settings.read_titles
                                ));
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
                                let r =
                                    snd.send(format!("Repeat is already `{}`", control.settings.repeat));
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
                                    skipmarker = true;
                                }
                            } else if let Some(tts_handle) = tts_handle.as_mut() {
                                let r = tts_handle.stop();
                                if let Err(e) = r {
                                    log.log(&format!("Error skipping tts: {}\n", e)).await;
                                } else {
                                    let r2 = snd.send(String::from("Skipped"));
                                    if let Err(e) = r2 {
                                        log.log(&format!("Error sending skip: {}\n", e)).await;
                                    }
                                    skipmarker = true;
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
                                format!(
                                    "Radio volume set to `{}%`",
                                    control.settings.raw_radiovolume() * 100.0
                                )
                            } else {
                                control.settings.set_volume(v);
                                format!(
                                    "Song volume set to `{}%`",
                                    control.settings.raw_volume() * 100.0
                                )
                            };
                            let r = snd.send(msg);
                            if let Err(e) = r {
                                log.log(&format!("Error updating message: {}\n", e)).await;
                            }
                        }
                        AudioPromiseCommand::SpecificVolume(SpecificVolume::Volume(v)) => {
                            control.settings.set_volume(v);
                            let r = snd.send(format!(
                                "Song volume set to `{}%`",
                                control.settings.raw_volume() * 100.0
                            ));
                            if let Err(e) = r {
                                log.log(&format!("Error updating message: {}\n", e)).await;
                            }
                        }
                        AudioPromiseCommand::SpecificVolume(SpecificVolume::RadioVolume(v)) => {
                            control.settings.set_radiovolume(v);
                            let r = snd.send(format!(
                                "Radio volume set to `{}%`",
                                control.settings.raw_radiovolume() * 100.0
                            ));
                            if let Err(e) = r {
                                log.log(&format!("Error updating message: {}\n", e)).await;
                            }
                        }
                        AudioPromiseCommand::Remove(index) => {
                            let index = index - 1;
                            if index < queue.len() {
                                let mut v = queue.remove(index);
                                let r = v.delete().await;
                                if let Err(r) = r {
                                    log.log(&format!("Error removing `{}`: {}\n", v.title, r))
                                        .await;
                                    let r = snd.send(format!("Error removing `{}`: {}", v.title, r));
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
                                let r = snd.send(format!("Index out of range, max is `{}`", queue.len()));
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
                                    cl.set_bitrate(Bitrate::Auto);
                                }
                                OrAuto::Specific(bitrate) => {
                                    cl.set_bitrate(Bitrate::BitsPerSecond(bitrate as i32));
                                }
                            }
                            let r = snd.send(format!("Bitrate set to `{}`", bitrate));
                            if let Err(e) = r {
                                log.log(&format!("Error updating message: {}\n", e)).await;
                            }
                        }
                    },
                    None => {
                        log.log("rx closed").await;
                        break;
                    }
                }
                if let Some(embed) = last_embed.as_ref() {
                    last_settings = Some(control.settings.clone());
                    if let Err(e) = msg_updater
                        .send((control.settings.clone(), embed.clone()))
                        .await
                    {
                        log.log(&format!("Error sending update: {}\n", e)).await;
                    }
                }
            }
            _ = run_dur.tick() => {
                while let Ok((msg, user)) = recv_new_transcription.try_recv() {
                    if msg.trim().is_empty() {
                        continue;
                    }
                    if let Err(e) = manually_send.send((msg, user)) {
                        log.log(&format!("Error sending transcription: {}\n", e))
                            .await;
                    }
                }
                if let Some(current) = current_track.as_mut() {
                    if let Some(thandle) = trackhandle.as_mut() {
                        let playmode = tokio::time::timeout(g_timeout_time, thandle.get_info()).await;
                        match playmode {
                            Ok(Err(ref e)) => {
                                if matches!(e, ControlError::Finished) {
                                    let url = current_track.as_ref().and_then(|t| match t.video {
                                        VideoType::Disk(_) => None,
                                        VideoType::Url(ref y) => Some(y.url.clone()),
                                    });
                                    if control.settings.autoplay && queue.is_empty() {
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
                                                let vid = match r.first() {
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
                                        if control.settings.repeat && !skipmarker {
                                            queue.insert(0, t.clone());
                                        } else if control.settings.looped {
                                            queue.push(t.clone());
                                        } else {
                                            let r = t.delete().await;
                                            if let Err(e) = r {
                                                log.log(&format!("Error deleting video: {}\n", e)).await;
                                            }
                                        }
                                    }
                                    skipmarker = false;
                                    current_track = None;
                                    trackhandle = None;
                                    tts_handle = None;
                                } else {
                                    log.log(format!("playmode error: {:?}", playmode).as_str())
                                        .await;
                                }
                            }
                            Err(e) => {
                                log.log(&format!("Error getting track info, Timeout: {}\n", e))
                                    .await;
                            }
                            Ok(_) => {
                                if skipmarker {
                                    let _ = thandle.stop();
                                }
                            }
                        }
                    } else if let Some(tts) = tts_handle.as_mut() {
                        let r = tokio::time::timeout(g_timeout_time, tts.get_info()).await;
                        match r {
                            Ok(Ok(_)) => {}
                            Ok(Err(ref e)) => {
                                if matches!(e, ControlError::Finished) {
                                    let calllock = control.call.try_lock();
                                    if let Ok(mut clock) = calllock {
                                        let handle = clock.play_input(current.video.to_songbird());
                                        if let Err(e) = handle.set_volume(control.settings.volume() as f32)
                                        {
                                            log.log(&format!("Error setting volume: {}\n", e)).await;
                                        }
                                        trackhandle = Some(handle);
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
                            if let Some(tts) = current.ttsmsg.as_mut() {
                                let check = tts.check().await;
                                match check {
                                    Ok(Some(tts)) => {
                                        let r = File::new(tts.path).into();
                                        let handle = clock.play_input(r);
                                        if let Err(e) = handle.set_volume(control.settings.volume() as f32)
                                        {
                                            log.log(&format!("Error setting volume: {}\n", e)).await;
                                        }
                                        tts_handle = Some(handle);
                                    }
                                    Ok(None) => {}
                                    Err(e) => {
                                        let err = format!("Error checking tts: {}\n", e);
                                        if !err.contains("None") {
                                            log.log(&format!("Error checking tts: {}\n", e)).await;
                                        }
                                        let r: songbird::input::Input = match current.video.clone() {
                                            VideoType::Disk(v) => File::new(v.path).into(),
                                            VideoType::Url(v) => {
                                                YoutubeDl::new(crate::WEB_CLIENT.clone(), v.url).into()
                                            }
                                        };
                                        let handle = clock.play_input(r);
                                        if let Err(e) = handle.set_volume(control.settings.volume() as f32)
                                        {
                                            log.log(&format!("Error setting volume: {}\n", e)).await;
                                        }
                                        trackhandle = Some(handle);
                                    }
                                }
                            } else {
                                let a = YoutubeDl::new(
                                    crate::WEB_CLIENT.clone(),
                                    crate::Config::get().bumper_url,
                                );
                                let handle = clock.play_input(a.into());
                                if let Err(e) = handle.set_volume(control.settings.volume() as f32) {
                                    log.log(&format!("Error setting volume: {}\n", e)).await;
                                }
                                tts_handle = Some(handle);
                            }
                            #[cfg(not(feature = "tts"))]
                            {
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
                            let vid = match r.first() {
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
                                log.log(&format!("Error getting azuracast data: {}\n", e))
                                    .await;
                            }
                        }
                    }
                }
                if queue.is_empty() && current_track.is_none() {
                    control.settings.pause = false;
                    if nothing_handle.is_none() {
                        let r: songbird::input::Input = if let Some(uri) = control.nothing_uri.clone() {
                            File::new(uri).into()
                        } else {
                            YoutubeDl::new(crate::WEB_CLIENT.clone(), crate::Config::get().idle_url).into()
                        };
                        {
                            let mut clock = control.call.lock().await;
                            let handle = clock.play_input(r);
                            if let Err(e) = handle.set_volume(control.settings.radiovolume() as f32) {
                                log.log(&format!("Error setting volume: {}\n", e)).await;
                            }
                            if let Err(e) = handle.enable_loop() {
                                log.log(&format!("Error enabling loop: {}\n", e)).await;
                            }
                            nothing_handle = Some(handle);
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
                    embed.color = Some(Color::from_rgb(184, 29, 19));
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
                    }
                    let mut possible_body = String::new();
                    let mut total_duration: Option<u64> = try {
                        let mut total = 0;
                        if let Some(ref current) = current_track {
                            total += current.video.get_duration()?;
                        };
                        for track in queue.iter() {
                            total += track.video.get_duration()?;
                        }
                        total
                    };
                    if let Some(t) = current_track.as_ref() {
                        let mut time_left = t.video.get_duration();
                        embed.fields.push((
                            format!("Now Playing | {}", t.title),
                            match time_left {
                                Some(d) => friendly_duration(&Duration::from_secs(d)),
                                None => "live".to_owned(),
                            },
                            false,
                        ));
                        if let Some(handle) = trackhandle.as_ref() {
                            let info = tokio::time::timeout(g_timeout_time, handle.get_info()).await;
                            match info {
                                Ok(Ok(info)) => {
                                    if let Some(ref mut dur) = time_left {
                                        let secs_elapsed = info.position.as_secs_f64();
                                        if let Some(ref mut length) = total_duration {
                                            *length -= secs_elapsed as u64;
                                        }
                                        let percent_done = secs_elapsed / *dur as f64;
                                        *dur -= secs_elapsed as u64;
                                        let total_time_str = friendly_duration(&Duration::from_secs(*dur));
                                        possible_body.push_str(&format!(
                                            "\n{}\n[{} remaining]",
                                            get_bar(percent_done, 15),
                                            total_time_str
                                        ));
                                    }
                                }
                                Ok(Err(e)) => {
                                    log.log(&format!("Error getting track info: {}\n", e)).await;
                                }
                                Err(e) => {
                                    log.log(&format!("Error getting track info: {}\n", e)).await;
                                }
                            }
                        }
                        let total_length_str = match total_duration {
                            Some(d) => format!("{} remaining", friendly_duration(&Duration::from_secs(d))),
                            None => "One or more tracks is live".to_owned(),
                        };
                        if let Some(ref author) = t.author {
                            embed.footer = Some((
                                format!("Requested by {} | {}", author.name, total_length_str),
                                Some(author.pfp_url.clone()),
                            ));
                        } else {
                            embed.footer = Some((total_length_str, None));
                        }
                        if !queue.is_empty() {
                            for (i, track) in queue.iter().enumerate() {
                                embed.fields.push((
                                    format!("#{} | {}", i + 1, track.title.clone()),
                                    match track.video.get_duration() {
                                        Some(d) => friendly_duration(&Duration::from_secs(d)),
                                        None => "live".to_owned(),
                                    },
                                    false,
                                ));
                            }
                        }
                        embed.color = Some(Color::from_rgb(0, 132, 80));
                    } else {
                        for (i, track) in queue.iter().enumerate() {
                            embed.fields.push((
                                format!("#{} | {}", i + 1, track.title.clone()),
                                match track.video.get_duration() {
                                    Some(d) => friendly_duration(&Duration::from_secs(d)),
                                    None => "live".to_owned(),
                                },
                                false,
                            ));
                        }
                        let total_length_str = match total_duration {
                            Some(d) => format!("{} remaining", friendly_duration(&Duration::from_secs(d))),
                            None => "One or more tracks is live".to_owned(),
                        };
                        embed.footer = Some((total_length_str, None));
                        embed.color = Some(Color::from_rgb(253, 218, 22));
                    }
                    if !possible_body.is_empty() {
                        embed.body = Some(possible_body);
                    }
                }
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
                    let r = handle.set_volume(control.settings.radiovolume() as f32);
                    if let Err(e) = r {
                        log.log(&format!("Error setting volume: {}\n", e)).await;
                    }
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
                {
                    match tokio::time::timeout(g_timeout_time, control.call.lock()).await {
                        Ok(call) => {
                            if let Some(_c) = call.current_connection() {
                            } else {
                                log.log("No connection, breaking\n").await;
                                control.brk = true;
                            }
                        }
                        Err(_) => {
                            log.log("Call lock timed out").await;
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
    let (returner, gimme) = tokio::sync::oneshot::channel::<mpsc::Receiver<RawMessage>>();
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
        while t.delete().await.is_err() {
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
        let r = video.delete().await;
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
    if kill_transcription_thread.send(()).await.is_err() {
        log.log("Error sending kill_transcription_thread").await;
    } else if let Err(e) = transcription_thread.await {
        log.log(&format!("Error joining transcription_thread: {}\n", e))
            .await;
    }
    log.log("Gracefully exited").await;
    // if !log.is_empty().await {
    //     // log::info!("Final log:\n{}", log.get().await);
    // }
}
fn get_bar(percent_done: f64, length: usize) -> String {
    let emojis = [
        ["<:LE:1038954704744480898>", "<:LC:1038954708422885386>"],
        ["<:CE:1038954710184497203>", "<:CC:1038954696980824094>"],
        ["<:RE:1038954703033217285>", "<:RC:1038954706841649192>"],
    ];
    let mut bar = String::new();
    let percent_done = percent_done - (1.0 / length as f64);
    let mut first = true;
    let mut circled = false;
    for i in 0..length {
        let pos = i as f64 / length as f64;
        if first {
            if pos >= percent_done && !circled {
                bar.push_str(emojis[0][1]);
                circled = true;
            } else {
                bar.push_str(emojis[0][0]);
            }
            first = false;
        } else if i == length - 1 {
            if pos >= percent_done && !circled {
                bar.push_str(emojis[2][1]);
                circled = true;
            } else {
                bar.push_str(emojis[2][0]);
            }
        } else if pos >= percent_done && !circled {
            bar.push_str(emojis[1][1]);
            circled = true;
        } else {
            bar.push_str(emojis[1][0]);
        }
    }
    bar
}
#[derive(Debug, PartialEq, Clone)]
pub struct EmbedData {
    author: Option<String>,
    author_url: Option<String>,
    author_icon_url: Option<String>,
    color: Option<Color>,
    pub body: Option<String>,
    fields: Vec25<(String, String, bool)>,
    thumbnail: Option<String>,
    footer: Option<(String, Option<String>)>,
}
impl EmbedData {
    pub fn to_serenity(&self) -> CreateEmbed {
        let mut e = CreateEmbed::new();
        if let Some(ref author) = self.author {
            let mut author = CreateEmbedAuthor::new(author);
            if let Some(ref author_url) = self.author_url {
                author = author.url(author_url);
            }
            if let Some(ref author_icon_url) = self.author_icon_url {
                author = author.icon_url(author_icon_url);
            }
            e = e.author(author);
        }
        if let Some(color) = self.color {
            e = e.color(color);
        }
        if let Some(ref body) = self.body {
            e = e.description(body);
        }
        for (name, value, inline) in self.fields.0.iter() {
            e = e.field(name, value, *inline);
        }
        if let Some(ref thumbnail) = self.thumbnail {
            e = e.thumbnail(thumbnail);
        }
        if let Some((ref footer, ref footer_img)) = self.footer {
            let mut footer = CreateEmbedFooter::new(footer);
            if let Some(footer_img) = footer_img {
                footer = footer.icon_url(footer_img);
            }
            e = e.footer(footer);
        }
        e
    }
}
impl Default for EmbedData {
    fn default() -> Self {
        Self { author: Some("Invite me to your server!".to_owned()), author_url: Some("https://discord.com/oauth2/authorize?client_id=1035364346471133194&permissions=274881349696&scope=bot".to_owned()), author_icon_url: None, color: Some(Color::from_rgb(0, 0, 0)), body: None, fields: Vec25::new(), thumbnail: None, footer: Some(("Type /help for help".to_owned(), None)) }
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
    source: String,
    log: Arc<Mutex<(Vec<LogString>, Instant)>>,
}
impl Log {
    pub fn new(source: String) -> Self {
        Self {
            source,
            log: Arc::new(Mutex::new((Vec::new(), Instant::now()))),
        }
    }
    pub async fn log(&self, s: &str) {
        let mut d = self.log.lock().await;
        let t = d.1.elapsed();
        log::info!("[{}] {}: {}", t.as_secs_f64(), self.source, s);
        d.0.push(LogString {
            s: s.to_owned(),
            time: t,
        });
    }
    #[allow(dead_code)]
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
        let mut s = ChunkOfLog {
            s: String::new(),
            end: 0,
        };
        let mut v = Vec::new();
        for (i, l) in d.0.iter().enumerate() {
            let pretty = l.pretty() + "\n";
            if s.s.len() + pretty.len() > 4000 {
                s.end = i;
                {
                    let mut string = String::new();
                    s.s.trim().clone_into(&mut string);
                    std::mem::swap(&mut s.s, &mut string);
                }
                v.push(s);
                s = ChunkOfLog {
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
    pub async fn is_empty(&self) -> bool {
        let d = self.log.lock().await;
        d.0.is_empty()
    }
}
pub struct ChunkOfLog {
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
fn friendly_duration(dur: &std::time::Duration) -> String {
    let mut dur = dur.as_secs();
    let mut s = String::new();
    let centuries = dur / (365 * 24 * 60 * 60 * 100);
    dur -= centuries * (365 * 24 * 60 * 60 * 100);
    if centuries > 0 {
        s.push_str(&format!("{} century", centuries));
        if centuries > 1 {
            s.push('s');
        }
        s.push(' ');
    }
    let years = dur / (365 * 24 * 60 * 60);
    dur -= years * (365 * 24 * 60 * 60);
    if years > 0 {
        s.push_str(&format!("{} year", years));
        if years > 1 {
            s.push('s');
        }
        s.push(' ');
    }
    let days = dur / (24 * 60 * 60);
    dur -= days * (24 * 60 * 60);
    if days > 0 {
        s.push_str(&format!("{} day", days));
        if days > 1 {
            s.push('s');
        }
        s.push(' ');
    }
    let hours = dur / (60 * 60);
    dur -= hours * (60 * 60);
    if hours > 0 {
        s.push_str(&format!("{} hour", hours));
        if hours > 1 {
            s.push('s');
        }
        s.push(' ');
    }
    let minutes = dur / 60;
    dur -= minutes * 60;
    if minutes > 0 {
        s.push_str(&format!("{} minute", minutes));
        if minutes > 1 {
            s.push('s');
        }
        s.push(' ');
    }
    let seconds = dur;
    if seconds > 0 {
        s.push_str(&format!("{} second", seconds));
        if seconds > 1 {
            s.push('s');
        }
        s.push(' ');
    }
    s.trim().to_owned()
}
