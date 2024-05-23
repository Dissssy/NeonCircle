use super::settingsdata::SettingsData;
use super::transcribe::{TranscribeChannelHandler, TranscriptionThread};
use super::{Author, MessageReference, OrAuto};
use crate::commands::music::{AudioPromiseCommand, MetaVideo, RawMessage, SpecificVolume};
use crate::radio::Root;
use crate::voice_events::{OptionalTimeout, PostSomething};
use anyhow::Result;
use rand::Rng;
use serenity::all::{ChannelId, Color, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, UserId};
use serenity::async_trait;
use songbird::driver::Bitrate;
use songbird::input::{File, YoutubeDl};
use songbird::tracks::{Track, TrackHandle, TrackState};
use songbird::{Call, EventContext};
use std::future::Future;
use std::mem;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex, RwLock};
use tokio::time::Instant;
pub struct ControlData {
    pub call: Arc<Mutex<Call>>,
    pub rx: mpsc::UnboundedReceiver<(oneshot::Sender<String>, AudioPromiseCommand)>,
    pub msg: MessageReference,
    pub nothing_uri: Option<PathBuf>,
    pub transcribe: Arc<RwLock<TranscribeChannelHandler>>,
    pub settings: SettingsData,
    pub log: Log,
}
pub async fn the_lüüp(
    mut transcription: TranscriptionThread,
    mut control: ControlData,
    this_bot_id: UserId,
) {
    let log = control.log.clone();
    let mut current_channel = control.msg.channel_id;
    log.log("Starting loop").await;
    log.log("Creating control data").await;
    {
        log.log("Locking call").await;
        let mut cl = control.call.lock().await;
        log.log("Setting bitrate").await;
        cl.set_bitrate(Bitrate::Auto);
    }
    let (msg_updater, update_msg) = mpsc::channel::<(SettingsData, EmbedData)>(8);
    let (change_channel, mut change_rx) = tokio::sync::broadcast::channel::<ChannelId>(1);
    let (manually_send, send_msg) = mpsc::unbounded_channel::<(PostSomething, UserId)>();
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
                            if let Err(e) = msg.update(shakesbutt.0, shakesbutt.1).await {
                                logger.log(&format!("Error updating message: {}", e)).await;
                            }
                        } else {
                            logger.log("Error getting next message").await;
                            break;
                        }
                    }
                    channel = change_rx.recv() => {
                        if let Ok(channel) = channel {
                            if let Err(e) = msg.change_channel(channel).await {
                                logger.log(&format!("Error changing channel: {}", e)).await;
                            }
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
    let mut queue: Vec<SuperHandle> = Vec::new();
    let mut current_song: Option<SuperHandle> = None;
    let mut current_handle: Option<HandleMetadata> = None;
    let mut last_embed: Option<EmbedData> = None;
    let mut last_settings = None;
    let mut nothing_handle: Option<TrackHandle> = None;
    // let g_timeout_time = Duration::from_millis(100);
    log.log("Creating azuracast").await;
    // let mut azuracast = match crate::Config::get().api_url {
    //     Some(ref url) => AzuraCast::new(url, log.clone(), g_timeout_time).await.ok(),
    //     None => None,
    // };
    let (mut azuracast_updates, mut azuracast_data) =
        match crate::global_data::azuracast::resubscribe().await {
            Ok((a, b)) => (Some(a), Some(b)),
            Err(e) => {
                log.log(&format!("Error resubscribing to azuracast: {}", e))
                    .await;
                (None, None)
            }
        };
    log.log("Locking transcription listener").await;
    let ttsrx = control.transcribe.write().await.get_receiver();
    let ttshandler = super::transcribe::Handler::new(Arc::clone(&control.call));
    let (killsubthread, bekilled) = tokio::sync::oneshot::channel::<
        tokio::sync::oneshot::Sender<tokio::sync::broadcast::Receiver<RawMessage>>,
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
                        }
                        break;
                    }
                    msg = ttsrx.recv() => {
                        match msg {
                            Ok(msg) => {
                                if let Err(e) = ttshandler.update(vec![msg]).await {
                                    logger.log(&format!("Error updating tts: {}", e)).await;
                                }
                            }
                            Err(e) => {
                                logger.log(&format!("Error receiving tts: {}", e)).await;
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
    let mut next_index = 0;
    let mut connection_events = DisconnectEvents::register(&control.call).await;
    let mut pending_reconnect = OptionalTimeout::new(std::time::Duration::from_secs(30));
    let mut pending_disconnect = {
        OptionalTimeout::new(
            crate::global_data::guild_config::GuildConfig::get(control.msg.guild_id)
                .get_empty_channel_timeout(),
        )
    };
    let mut rerun = OptionalTimeout::new(std::time::Duration::from_millis(10));
    rerun.begin_now();
    loop {
        control.settings.log_empty = log.is_empty().await;
        tokio::select! {
            t = control.rx.recv() => {
                match t {
                    Some((snd, command)) => match command {
                        AudioPromiseCommand::RetrieveLog(log_snd) => {
                            let chunks = log.get_chunks_4k().await;
                            let mut string_chunks = chunks
                                .iter()
                                .map(|c| (c.s.clone(), c.end))
                                .collect::<Vec<(String, usize)>>();
                            let end = if string_chunks.len() > 5 {
                                string_chunks.truncate(5);
                                chunks.get(4).map(|e| e.end).unwrap_or(0)
                            } else {
                                chunks.last().map(|e| e.end).unwrap_or(0)
                            };
                            if let Err(e) = log_snd
                                .send(
                                    string_chunks
                                        .into_iter()
                                        .map(|(s, _)| s)
                                        .collect::<Vec<String>>(),
                                )
                                .await {
                                log.log(&format!("Error sending log: {}\n", e)).await;
                            }
                            if let Err(e) = snd.send("Log sent!".to_owned()) {
                                log.log(&format!("Error sending log: {}\n", e)).await;
                            }
                            log.clear_until(end).await;
                        }
                        AudioPromiseCommand::Play(videos) => {
                            for v in videos {
                                queue.push(match SuperHandle::new(&control.call, v, control.settings.song_volume()).await {
                                    Ok(h) => h,
                                    Err(e) => {
                                        log.log(&format!("Error creating handle: {}\n", e)).await;
                                        continue;
                                    }
                                });
                            }
                            if let Err(e) = snd.send(String::from("Added to queue")) {
                                log.log(&format!("Error sending play: {}\n", e)).await;
                            }
                            next_index = if control.settings.shuffle {
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
                        }
                        AudioPromiseCommand::Stop(delay) => {
                            if let Err(e) = snd.send(String::from("Stopped")) {
                                log.log(&format!("Error sending stop: {}\n", e)).await;
                            }
                            if let Some(delay) = delay {
                                tokio::time::sleep(delay).await;
                            }
                            break;
                        }
                        AudioPromiseCommand::Paused(paused) => {
                            let val = paused.get_val(control.settings.pause);
                            if let Some(handle) = current_handle.as_ref() {
                                if control.settings.pause != val {
                                    control.settings.pause = val;
                                    let trackhandle = handle.get_handle();
                                    if control.settings.pause {
                                        if let Err(e) = trackhandle.pause() {
                                            log.log(&format!("Error pausing track: {}\n", e)).await;
                                        }
                                    } else if let Err(e) = trackhandle.play() {
                                        log.log(&format!("Error resuming track: {}\n", e)).await;
                                    };
                                    if let Err(e) = snd.send(format!("Paused set to `{}`", control.settings.pause)) {
                                        log.log(&format!("Error sending pause: {}\n", e)).await;
                                    }
                                }
                            } else if let Err(e) = snd.send(String::from("Nothing is playing")) {
                                log.log(&format!("Error responding to command{}\n", e)).await;
                            }
                        }
                        AudioPromiseCommand::Shuffle(shuffle) => {
                            let shuffle = shuffle.get_val(control.settings.shuffle);
                            if control.settings.shuffle != shuffle {
                                control.settings.shuffle = shuffle;
                                if let Err(e) = snd.send(format!("Shuffle set to `{}`", control.settings.shuffle)) {
                                    log.log(&format!("Error responding to command{}\n", e)).await;
                                }
                            } else if let Err(e) = snd.send(format!("Shuffle is already `{}`", control.settings.shuffle)) {
                                log.log(&format!("Error responding to command{}\n", e)).await;
                            }
                        }
                        AudioPromiseCommand::Autoplay(autoplay) => {
                            let autoplay = autoplay.get_val(control.settings.autoplay);
                            if control.settings.autoplay != autoplay {
                                control.settings.autoplay = autoplay;
                                if let Err(e) = snd.send(format!("Autoplay set to `{}`", control.settings.autoplay)) {
                                    log.log(&format!("Error responding to command{}\n", e)).await;
                                }
                            } else if let Err(e) = snd.send(format!(
                                "Autoplay is already `{}`",
                                control.settings.autoplay
                            )) {
                                log.log(&format!("Error responding to command{}\n", e)).await;
                            }
                        }
                        AudioPromiseCommand::ReadTitles(read_titles) => {
                            let read_titles = read_titles.get_val(control.settings.read_titles);
                            if control.settings.read_titles != read_titles {
                                control.settings.read_titles = read_titles;
                                if let Err(e) = snd.send(format!(
                                    "Read titles set to `{}`",
                                    control.settings.read_titles
                                )) {
                                    log.log(&format!("Error responding to command{}\n", e)).await;
                                }
                            } else if let Err(e) = snd.send(format!(
                                "Read titles is already `{}`",
                                control.settings.read_titles
                            )) {
                                log.log(&format!("Error responding to command{}\n", e)).await;
                            }
                        }
                        AudioPromiseCommand::Loop(looped) => {
                            let looped = looped.get_val(control.settings.looped);
                            if control.settings.looped != looped {
                                control.settings.looped = looped;
                                if let Err(e) = snd.send(format!("Loop set to `{}`", control.settings.looped)) {
                                    log.log(&format!("Error responding to command{}\n", e)).await;
                                }
                            } else if let Err(e) = snd.send(format!("Loop is already `{}`", control.settings.looped)) {
                                log.log(&format!("Error responding to command{}\n", e)).await;
                            }
                        }
                        AudioPromiseCommand::Repeat(repeat) => {
                            let repeat = repeat.get_val(control.settings.repeat);
                            if control.settings.repeat != repeat {
                                control.settings.repeat = repeat;
                                if let Err(e) = snd.send(format!("Repeat set to `{}`", control.settings.repeat)) {
                                    log.log(&format!("Error responding to command{}\n", e)).await;
                                }
                            } else {
                                let r =
                                    snd.send(format!("Repeat is already `{}`", control.settings.repeat));
                                if let Err(e) = r {
                                    log.log(&format!("Error responding to command{}\n", e)).await;
                                }
                            }
                        }
                        AudioPromiseCommand::Skip => {
                            if let Some(trackhandle) = current_song.take() {
                                log.log(&format!("Skipping track on line {}", line!())).await;
                                trackhandle.stop(&log).await;
                                if let Some(handle) = current_handle.take() {
                                    if let Err(e) = handle.get_handle().stop() {
                                        log.log(&format!("Error stopping track: {}\n", e)).await;
                                    }
                                }
                            } else if let Err(e) = snd.send(String::from("Nothing is playing")) {
                                log.log(&format!("Error responding to command{}\n", e)).await;
                            }
                        }
                        AudioPromiseCommand::Volume(SpecificVolume::Current(v)) => {
                            if let Some(handle) = current_handle.as_ref() {
                                if let Err(e) = handle.get_handle().set_volume(v) {
                                    log.log(&format!("Error setting volume: {}\n", e)).await;
                                }
                                control.settings.set_song_volume(v);
                            } else {
                                if let Some(ref handle) = nothing_handle {
                                    if let Err(e) = handle.set_volume(v) {
                                        log.log(&format!("Error setting volume: {}\n", e)).await;
                                    }
                                }
                                control.settings.set_radio_volume(v);
                            }
                        }
                        AudioPromiseCommand::Volume(SpecificVolume::SongVolume(v)) => {
                            control.settings.set_song_volume(v);
                            if let Err(e) = snd.send(format!(
                                "Song volume set to `{}%`",
                                control.settings.display_song_volume() * 100.0
                            )) {
                                log.log(&format!("Error responding to command{}\n", e)).await;
                            }
                            if let Some(handle) = current_handle.as_ref() {
                                if let Err(e) = handle.get_handle().set_volume(v) {
                                    log.log(&format!("Error setting volume: {}\n", e)).await;
                                }
                            }
                        }
                        AudioPromiseCommand::Volume(SpecificVolume::RadioVolume(v)) => {
                            control.settings.set_radio_volume(v);
                            if let Err(e) = snd.send(format!(
                                "Radio volume set to `{}%`",
                                control.settings.display_radio_volume() * 100.0
                            )) {
                                log.log(&format!("Error responding to command{}\n", e)).await;
                            }
                            if let Some(ref handle) = nothing_handle {
                                if let Err(e) = handle.set_volume(control.settings.radio_volume()) {
                                    log.log(&format!("Error setting volume: {}\n", e)).await;
                                }
                            }
                        }
                        AudioPromiseCommand::Remove(i) => {
                            let index = i - 1;
                            if index < queue.len() {
                                let v = queue.remove(index);
                                if let Err(e) = snd.send(format!("Removed `{}`", v.title)) {
                                    log.log(&format!("Error responding to command{}\n", e)).await;
                                }
                            } else if let Err(e) = snd.send(format!("Index out of range, max is `{}`", queue.len())) {
                                log.log(&format!("Error responding to command{}\n", e)).await;
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
                            if let Err(e) = snd.send(format!("Bitrate set to `{}`", bitrate)) {
                                log.log(&format!("Error responding to command{}\n", e)).await;
                            }
                        }
                        AudioPromiseCommand::UserConnect(id) => {
                            pending_disconnect.end_now();
                            if let Err(e) = snd.send(format!("User `{}` connected", id)) {
                                log.log(&format!("Error responding to command{}\n", e)).await;
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
            _ = if_true(current_song.is_none() && !queue.is_empty()) => {
                let mut superhandle = if next_index < queue.len() {
                    queue.remove(next_index)
                } else {
                    continue
                };
                match superhandle.next_audio(!control.settings.read_titles).await {
                    Ok(Some(next_song)) => {
                        log::info!("Playing next audio");
                        match next_song.handle {
                            HandleType::Song(ref handle) => {
                                if let Err(e) = handle.play() {
                                    log.log(&format!("Error playing song: {}\n", e)).await;
                                } else {
                                    if let Err(e) = next_song.get_handle().set_volume(control.settings.song_volume()) {
                                        log.log(&format!("Error setting volume: {}\n", e)).await;
                                    }
                                    current_handle = Some(next_song);
                                    current_song = Some(superhandle);
                                }
                            }
                            HandleType::Tts(ref handle) => {
                                if control.settings.read_titles {
                                    if let Err(e) = handle.play() {
                                        log.log(&format!("Error playing tts: {}\n", e)).await;
                                    } else {
                                        if let Err(e) = next_song.get_handle().set_volume(control.settings.song_volume()) {
                                            log.log(&format!("Error setting volume: {}\n", e)).await;
                                        }
                                        current_handle = Some(next_song);
                                        current_song = Some(superhandle);
                                    }
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        continue
                    }
                    Err(e) => {
                        log.log(&format!("Error getting next song: {}\n", e)).await;
                    }
                }
            }
            msg = get_message(current_handle.as_mut(), current_song.as_mut()) => {
                match msg {
                    Ok((_handle, msg, current, song)) => {
                        log::trace!("Got message: {:?}", msg);
                        match msg {
                            SimpleTrackEvent::SongFinished => {
                                log.log("Track finished").await;
                                current_song = None;
                            }
                            SimpleTrackEvent::SongError(e) => {
                                log.log(&format!("Error playing track: {}\n", e)).await;
                                current_song = None;
                            }
                            SimpleTrackEvent::SongBegan => {
                                log::trace!("Track began");
                            }
                            SimpleTrackEvent::TtsFinished => {
                                log.log("TTS finished").await;
                                match song.next_audio(control.settings.read_titles).await {
                                    Ok(Some(next_song)) => {
                                        log::info!("Playing next audio");
                                        *current = next_song;
                                        match current.handle {
                                            HandleType::Song(ref handle) => {
                                                if let Err(e) = handle.play() {
                                                    log.log(&format!("Error playing song: {}\n", e)).await;
                                                    current_song = None;
                                                }
                                            }
                                            HandleType::Tts(ref handle) => {
                                                if control.settings.read_titles {
                                                    if let Err(e) = handle.play() {
                                                        log.log(&format!("Error playing tts: {}\n", e)).await;
                                                        current_song = None;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Ok(None) => {
                                        current_song = None;
                                        continue
                                    }
                                    Err(e) => {
                                        current_song = None;
                                        log.log(&format!("Error getting next song: {}\n", e)).await;
                                    }
                                }
                            }
                            SimpleTrackEvent::TtsError(e) => {
                                log.log(&format!("Error playing tts: {}\n", e)).await;
                                match song.next_audio(control.settings.read_titles).await {
                                    Ok(Some(next_song)) => {
                                        log::info!("Playing next audio");
                                        *current = next_song;
                                        match current.handle {
                                            HandleType::Song(ref handle) => {
                                                if let Err(e) = handle.play() {
                                                    log.log(&format!("Error playing song: {}\n", e)).await;
                                                    current_song = None;
                                                }
                                            }
                                            HandleType::Tts(ref handle) => {
                                                if control.settings.read_titles {
                                                    if let Err(e) = handle.play() {
                                                        log.log(&format!("Error playing tts: {}\n", e)).await;
                                                        current_song = None;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Ok(None) => {
                                        current_song = None;
                                        continue
                                    }
                                    Err(e) => {
                                        current_song = None;
                                        log.log(&format!("Error getting next song: {}\n", e)).await;
                                    }
                                }
                            }
                            SimpleTrackEvent::TtsBegan => {
                                log::trace!("TTS began");
                            }
                        }
                    }
                    Err(e) => {
                        log.log(&format!("Error getting message: {}\n", e)).await;
                    }
                }
            }
            Some(event) = connection_events.recv() => {
                match event {
                    SimpleConnectionEvent::DriverDisconnect => {
                        log.log("Connection lost, starting timeout").await;
                        pending_reconnect.begin_now();
                    }
                    SimpleConnectionEvent::DriverConnect(channel) => {
                        log.log("Connection established, cancelling timeout").await;
                        pending_reconnect.end_now();
                        match crate::global_data::voice_data::channel_count_besides(&control.msg.guild_id, &control.msg.channel_id, &this_bot_id).await {
                            Ok(counts) => {
                                if counts.bots > 0 {
                                    log.log("Too many bots, leaving immediately").await;
                                    break;
                                } else if counts.users > 0 {
                                    log.log("Users are still here, staying").await;
                                } else {
                                    log.log("Bot is alone, leaving").await;
                                    break;
                                }
                            }
                            Err(e) => {
                                log.log(&format!("Error checking if bot is alone: {}\n", e)).await;
                            }
                        }
                        if let Some(channel) = channel {
                            if current_channel != channel {
                                log.log("Channel changed, updating").await;
                                current_channel = channel;
                                if let Err(e) = change_channel.send(channel) {
                                    log.log(&format!("Error joining channel: {}\n", e)).await;
                                }
                            }
                        }
                    }
                    SimpleConnectionEvent::ClientDisconnect => {
                        log.log("Client disconnected, beginning timeout").await;
                        pending_disconnect.set_duration(crate::global_data::guild_config::GuildConfig::get(control.msg.guild_id).get_empty_channel_timeout());
                        pending_disconnect.begin_now();
                    }
                }
            }
            _ = &mut pending_reconnect => {
                log.log("Connection lost, breaking").await;
                break;
            }
            _ = &mut pending_disconnect => {
                log.log("Bot has been alone, checking if it should leave").await;
                match crate::global_data::voice_data::channel_count_besides(&control.msg.guild_id, &control.msg.channel_id, &this_bot_id).await {
                    Ok(counts) => {
                        if counts.bots > 0 {
                            log.log("Too many bots, leaving immediately").await;
                            break;
                        } else if counts.users > 0 {
                            log.log("Users are still here, staying").await;
                        } else {
                            log.log("Bot is alone, leaving").await;
                            break;
                        }
                    }
                    Err(e) => {
                        log.log(&format!("Error checking if bot is alone: {}\n", e)).await;
                    }
                }
                pending_disconnect.end_now();
            }
            _ = &mut rerun => {
                log.log("Force rerun").await;
                rerun.end_now();
            }
            Some((something, user)) = transcription.receiver.recv() => {
                if let Err(e) = manually_send.send((something, user)) {
                    log.log(&format!("Error sending transcription: {}\n", e)).await;
                }
            }
            data = never_resolve_option(azuracast_updates.as_mut()) => {
                azuracast_data = Some(data);
            }
        }
        let mut embed = EmbedData::default();
        if queue.is_empty() && current_song.is_none() {
            control.settings.pause = false;
            if let Some(handle) = nothing_handle.as_mut() {
                if let Err(e) = handle.play() {
                    log.log(&format!("Error playing nothing: {}\n", e)).await;
                }
            } else {
                let r: songbird::input::Input = if let Some(uri) = control.nothing_uri.clone() {
                    File::new(uri).into()
                } else {
                    YoutubeDl::new(crate::WEB_CLIENT.clone(), crate::Config::get().idle_url).into()
                };
                {
                    let mut clock = control.call.lock().await;
                    let handle = clock.play(
                        Track::new(r)
                            .volume(control.settings.radio_volume())
                            .loops(songbird::tracks::LoopState::Infinite),
                    );
                    nothing_handle = Some(handle);
                }
            }
            let mut possible_body = "Queue is empty, use `/add` to play something!".to_owned();
            if let Some(ref data) = azuracast_data {
                possible_body = format!(
                    "{}\nIn the meantime, enjoy these fine tunes from `{}`",
                    possible_body, data.station.name,
                );
                embed.fields.push((
                    "Now Playing".to_owned(), // i want the song title, artist, and album
                    format!(
                        "{} - {} on {}",
                        data.now_playing.song.title,
                        data.now_playing.song.artist,
                        data.now_playing.song.album
                    ),
                    false,
                ));
                embed.thumbnail = Some(data.now_playing.song.art.clone());
                embed.fields.push((
                    "Next Up:".to_string(),
                    format!(
                        "{} - {} on {}",
                        data.playing_next.song.title,
                        data.playing_next.song.artist,
                        data.playing_next.song.album
                    ),
                    true,
                ));
            };
            if !possible_body.is_empty() {
                embed.body = Some(possible_body);
            }
            embed.color = Some(Color::from_rgb(184, 29, 19));
        } else {
            if let Some(ref data) = azuracast_data {
                embed.author = Some(format!(
                    "{} - {} playing on {}",
                    data.now_playing.song.title, data.now_playing.song.artist, data.station.name
                ));
                embed.author_icon_url = Some(data.now_playing.song.art.clone());
            }
            if let Some(handle) = nothing_handle.as_mut() {
                if let Err(e) = handle.pause() {
                    log.log(&format!("Error pausing nothing: {}\n", e)).await;
                }
            }
            let mut possible_body = String::new();
            let mut total_duration: Option<f64> = try {
                let mut total = 0.0;
                if let Some(ref current) = current_song {
                    total += current.duration?;
                };
                for track in queue.iter() {
                    total += track.duration?;
                }
                total
            };
            if let Some(t) = current_song.as_ref() {
                let mut time_left = t.duration;
                embed.fields.push((
                    format!("Now Playing | {}", t.title),
                    match time_left {
                        Some(d) => friendly_duration(&Duration::from_secs(d.round() as u64)),
                        None => "live".to_owned(),
                    },
                    false,
                ));
                if let Some(handle) = current_handle.as_ref() {
                    match handle.last_state.as_ref() {
                        Some(info) => {
                            if let Some(ref mut dur) = time_left {
                                let secs_elapsed = info.position.as_secs_f64();
                                if let Some(ref mut length) = total_duration {
                                    *length -= secs_elapsed;
                                }
                                let percent_done = secs_elapsed / *dur;
                                *dur -= secs_elapsed;
                                let total_time_str =
                                    friendly_duration(&Duration::from_secs(dur.round() as u64));
                                possible_body.push_str(&format!(
                                    "\n{}\n[{} remaining]",
                                    get_bar(percent_done, 15),
                                    total_time_str
                                ));
                            }
                        }
                        None => {
                            log.log("No last state").await;
                        }
                    }
                }
                let total_length_str = match total_duration {
                    Some(d) => format!(
                        "{} remaining",
                        friendly_duration(&Duration::from_secs(d.round() as u64))
                    ),
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
                            format!("#{} | {}", i + 1, track.title),
                            match track.duration {
                                Some(d) => {
                                    friendly_duration(&Duration::from_secs(d.round() as u64))
                                }
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
                        format!("#{} | {}", i + 1, track.title),
                        match track.duration {
                            Some(d) => friendly_duration(&Duration::from_secs(d.round() as u64)),
                            None => "live".to_owned(),
                        },
                        false,
                    ));
                }
                let total_length_str = match total_duration {
                    Some(d) => format!(
                        "{} remaining",
                        friendly_duration(&Duration::from_secs(d.round() as u64))
                    ),
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
    }
    log.log("SHUTTING DOWN").await;
    let (returner, gimme) =
        tokio::sync::oneshot::channel::<tokio::sync::broadcast::Receiver<RawMessage>>();
    if killsubthread.send(returner).is_err() {
        log.log("Error sending killsubthread").await;
    }
    if let Err(e) = subthread.await {
        log.log(&format!("Error joining subthread: {}\n", e)).await;
    }
    match gimme.await {
        Ok(gimme) => {
            drop(gimme);
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
    if let Some(t) = current_handle.take() {
        if let Err(e) = t.get_handle().stop() {
            log.log(&format!("Error stopping track: {}\n", e)).await;
        }
    }
    if let Some(t) = current_song.take() {
        t.stop(&log).await;
    }
    if let Err(e) = calllock.leave().await {
        log.log(&format!("Error leaving voice channel: {}\n", e))
            .await;
    }
    if killmsg.send(()).is_err() {
        log.log("Error sending killmsg").await;
    } else if let Err(e) = msghandler.await {
        log.log(&format!("Error joining msghandler: {}\n", e)).await;
    }
    if let Err(e) = transcription.stop().await {
        log.log(&format!("Error killing transcription: {}\n", e))
            .await;
    }
    log.log("Gracefully exited").await;
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
    log: Arc<RwLock<(Vec<LogString>, Instant)>>,
}
impl Log {
    pub fn new(source: String) -> Self {
        Self {
            source,
            log: Arc::new(RwLock::new((Vec::new(), Instant::now()))),
        }
    }
    pub async fn log(&self, s: &str) {
        let mut d = self.log.write().await;
        let t = d.1.elapsed();
        log::info!("[{}] {}: {}", t.as_secs_f64(), self.source, s);
        d.0.push(LogString {
            s: s.to_owned(),
            time: t,
        });
    }
    #[allow(dead_code)]
    pub async fn get(&self) -> String {
        let d = self.log.read().await;
        d.0.iter()
            .map(|l| l.pretty())
            .collect::<Vec<String>>()
            .join("\n")
    }
    pub async fn clear_until(&self, rm: usize) {
        let mut d = self.log.write().await;
        if rm >= d.0.len() {
            d.0.clear();
            return;
        }
        d.0.drain(0..=rm);
    }
    pub async fn get_chunks_4k(&self) -> Vec<ChunkOfLog> {
        let d = self.log.read().await;
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
        let d = self.log.read().await;
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
pub fn friendly_duration(dur: &std::time::Duration) -> String {
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
    if s.trim().is_empty() {
        s.push_str("0 seconds");
    }
    s.trim().to_owned()
}
static EVENTS: &[songbird::Event] = &[
    songbird::Event::Track(songbird::TrackEvent::End),
    songbird::Event::Track(songbird::TrackEvent::Error),
    songbird::Event::Track(songbird::TrackEvent::Play),
];
#[derive(Debug, Clone)]
enum SimpleTrackEvent {
    SongFinished,
    SongError(songbird::error::PlayError),
    TtsFinished,
    TtsError(songbird::error::PlayError),
    TtsBegan,
    SongBegan,
}
struct HandleMetadata {
    handle: HandleType,
    last_state: Option<TrackState>,
    recv: tokio::sync::mpsc::Receiver<(TrackHandle, TrackState, SimpleTrackEvent)>,
}
impl HandleMetadata {
    async fn process_handle(handle: HandleType) -> Result<Self> {
        let (send, recv) = tokio::sync::mpsc::channel(3);
        match handle {
            HandleType::Song(ref handle) => {
                match handle.get_info().await?.playing {
                    songbird::tracks::PlayMode::End => {
                        return Err(anyhow::anyhow!("Track ended before we could process it"));
                    }
                    songbird::tracks::PlayMode::Stop => {
                        return Err(anyhow::anyhow!("Track stopped before we could process it"));
                    }
                    songbird::tracks::PlayMode::Errored(ref e) => {
                        return Err(anyhow::anyhow!(
                            "Track errored before we could process it: {:?}",
                            e
                        ));
                    }
                    _ => {}
                }
                let handler = TrackEventHandler { send, tts: false };
                for event in EVENTS {
                    handle.add_event(*event, handler.clone())?;
                }
            }
            HandleType::Tts(ref handle) => {
                match handle.get_info().await?.playing {
                    songbird::tracks::PlayMode::End => {
                        return Err(anyhow::anyhow!("Track ended before we could process it"));
                    }
                    songbird::tracks::PlayMode::Stop => {
                        return Err(anyhow::anyhow!("Track stopped before we could process it"));
                    }
                    songbird::tracks::PlayMode::Errored(ref e) => {
                        return Err(anyhow::anyhow!(
                            "Track errored before we could process it: {:?}",
                            e
                        ));
                    }
                    _ => {}
                }
                let handler = TrackEventHandler { send, tts: true };
                for event in EVENTS {
                    handle.add_event(*event, handler.clone())?;
                }
            }
        }
        Ok(Self {
            handle,
            recv,
            last_state: None,
        })
    }
    fn get_handle(&self) -> &TrackHandle {
        match &self.handle {
            HandleType::Song(handle) => handle,
            HandleType::Tts(handle) => handle,
        }
    }
}
enum HandleType {
    Song(TrackHandle),
    Tts(TrackHandle),
}
#[derive(Clone)]
struct TrackEventHandler {
    tts: bool,
    send: tokio::sync::mpsc::Sender<(TrackHandle, TrackState, SimpleTrackEvent)>,
}
#[async_trait]
impl songbird::EventHandler for TrackEventHandler {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<songbird::events::Event> {
        if let EventContext::Track(tracks) = ctx {
            for (state, handle) in *tracks {
                match state.playing {
                    songbird::tracks::PlayMode::End => {
                        let _ = self
                            .send
                            .send((
                                (*handle).clone(),
                                (*state).clone(),
                                if self.tts {
                                    SimpleTrackEvent::TtsFinished
                                } else {
                                    SimpleTrackEvent::SongFinished
                                },
                            ))
                            .await;
                    }
                    songbird::tracks::PlayMode::Stop => {
                        let _ = self
                            .send
                            .send((
                                (*handle).clone(),
                                (*state).clone(),
                                if self.tts {
                                    SimpleTrackEvent::TtsFinished
                                } else {
                                    SimpleTrackEvent::SongFinished
                                },
                            ))
                            .await;
                    }
                    songbird::tracks::PlayMode::Errored(ref e) => {
                        let _ = self
                            .send
                            .send((
                                (*handle).clone(),
                                (*state).clone(),
                                if self.tts {
                                    SimpleTrackEvent::TtsError(e.clone())
                                } else {
                                    SimpleTrackEvent::SongError(e.clone())
                                },
                            ))
                            .await;
                    }
                    songbird::tracks::PlayMode::Play => {
                        let _ = self
                            .send
                            .send((
                                (*handle).clone(),
                                (*state).clone(),
                                if self.tts {
                                    SimpleTrackEvent::TtsBegan
                                } else {
                                    SimpleTrackEvent::SongBegan
                                },
                            ))
                            .await;
                    }
                    _ => {}
                }
            }
        }
        None
    }
}
async fn never_resolve_option(opt: Option<&mut broadcast::Receiver<Arc<Root>>>) -> Arc<Root> {
    match opt {
        Some(rx) => match rx.recv().await {
            Ok(data) => data,
            Err(e) => {
                log::error!("Error receiving data: {}", e);
                Never::default().await
            }
        },
        None => Never::default().await,
    }
}
async fn if_true(b: bool) -> Result<()> {
    if b {
        Ok(())
    } else {
        Never::default().await
    }
}
struct Never<T> {
    _phantom: std::marker::PhantomData<T>,
}
impl<T> Default for Never<T> {
    fn default() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}
impl<T> Future for Never<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<T> {
        Poll::Pending
    }
}
async fn get_message<'a>(
    current_track: Option<&'a mut HandleMetadata>,
    current_song: Option<&'a mut SuperHandle>,
) -> Result<(
    TrackHandle,
    SimpleTrackEvent,
    &'a mut HandleMetadata,
    &'a mut SuperHandle,
)> {
    log::trace!("Getting message");
    match current_song {
        Some(song) => match current_track {
            Some(current) => {
                log::trace!("Getting message from song");
                match current.recv.recv().await {
                    Some((handle, state, event)) => {
                        current.last_state = Some(state);
                        Ok((handle, event, current, song))
                    }
                    None => Err(anyhow::anyhow!("Event handler closed")),
                }
            }
            None => {
                log::trace!("No current, waiting forever this time");
                Never::default().await
            }
        },
        None => {
            log::trace!("No song, waiting forever this time");
            Never::default().await
        }
    }
}
struct SuperHandle {
    tts: Option<Lazy<Result<Option<HandleMetadata>>>>,
    song: Lazy<Result<HandleMetadata>>,
    title: Arc<str>,
    duration: Option<f64>,
    author: Option<Author>,
}
impl SuperHandle {
    async fn stop(mut self, log: &Log) {
        if let Some(mut tts) = self.tts.take() {
            if let Err(e) = tts.resolve().await {
                log.log(&format!("Error resolving tts: {}\n", e)).await;
            }
            match tts.take() {
                Some(Ok(Some(tts))) => {
                    let _ = tts.get_handle().stop();
                }
                Some(Err(e)) => {
                    log.log(&format!("Error resolving tts: {}\n", e)).await;
                }
                _ => {}
            }
        }
        if let Err(e) = self.song.resolve().await {
            log.log(&format!("Error resolving song: {}\n", e)).await;
        }
        if let Some(Ok(song)) = self.song.take() {
            let _ = song.get_handle().stop();
        }
    }
    async fn new(call: &Arc<Mutex<Call>>, data: MetaVideo, volume: f32) -> Result<Self> {
        let (song, title, duration, author) = {
            let call = Arc::clone(call);
            let song = data.video;
            let title = song.get_title();
            let author = data.author.clone();
            let duration = song.get_duration();
            (
                Lazy::new(async move {
                    let handle = {
                        let mut clock = call.lock().await;
                        clock.play(Track::new(song.to_songbird()).pause().volume(volume))
                    };
                    HandleMetadata::process_handle(HandleType::Song(handle)).await
                })
                .await,
                title,
                duration,
                author,
            )
        };
        let tts = {
            let call = Arc::clone(call);
            let tts = data.ttsmsg;
            Lazy::new(async move {
                let tts = match tts {
                    Some(mut tts) => tts.wait_for().await?,
                    None => return Ok(None),
                };
                let handle = {
                    let mut clock = call.lock().await;
                    clock.play(Track::new(tts.to_songbird()).pause().volume(volume))
                };
                Ok(Some(
                    HandleMetadata::process_handle(HandleType::Tts(handle)).await?,
                ))
            })
            .await
        };
        Ok(Self {
            tts: Some(tts),
            song,
            title,
            duration,
            author,
        })
    }
    async fn next_audio(&mut self, read_titles: bool) -> Result<Option<HandleMetadata>> {
        if read_titles {
            if let Some(ref mut tts) = self.tts {
                tts.resolve().await?;
                if let Some(mut tts) = self.tts.take() {
                    if let Some(ttsresult) = tts.take() {
                        if let Some(tts) = ttsresult? {
                            return Ok(Some(tts));
                        }
                    }
                }
            }
        }
        self.song.resolve().await?;
        if let Some(song) = self.song.take() {
            return Ok(Some(song?));
        }
        Ok(None)
    }
}
#[derive(Default)]
enum Lazy<T> {
    #[default]
    Taken,
    Done(T),
    NotDone(tokio::task::JoinHandle<T>),
}
impl<T> Lazy<T> {
    async fn resolve(&mut self) -> Result<()> {
        match self {
            Lazy::Taken => Err(anyhow::anyhow!("Lazy is taken")),
            Lazy::Done(_) => Ok(()),
            Lazy::NotDone(handle) => {
                let t = handle.await?;
                *self = Lazy::Done(t);
                Ok(())
            }
        }
    }
    fn take(&mut self) -> Option<T> {
        match mem::take(self) {
            Lazy::Taken => {
                log::trace!("Lazy is taken");
                None
            }
            Lazy::Done(t) => Some(t),
            Lazy::NotDone(_) => None,
        }
    }
}
impl<T: Send + 'static> Lazy<T> {
    async fn new<F>(f: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
    {
        Lazy::NotDone(tokio::spawn(f))
    }
}
#[derive(Debug, Clone)]
struct DisconnectEvents {
    snd: mpsc::Sender<SimpleConnectionEvent>,
}
#[async_trait]
impl songbird::EventHandler for DisconnectEvents {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<songbird::events::Event> {
        match ctx {
            EventContext::ClientDisconnect(_) => {
                let _ = self.snd.send(SimpleConnectionEvent::ClientDisconnect).await;
            }
            EventContext::DriverDisconnect(_) => {
                let _ = self.snd.send(SimpleConnectionEvent::DriverDisconnect).await;
            }
            EventContext::DriverReconnect(data) => {
                let _ = self
                    .snd
                    .send(SimpleConnectionEvent::DriverConnect(
                        data.channel_id.map(|c| c.0.get()).map(ChannelId::new),
                    ))
                    .await;
            }
            EventContext::DriverConnect(data) => {
                let _ = self
                    .snd
                    .send(SimpleConnectionEvent::DriverConnect(
                        data.channel_id.map(|c| c.0.get()).map(ChannelId::new),
                    ))
                    .await;
            }
            _ => {}
        }
        None
    }
}
impl DisconnectEvents {
    async fn register(call: &Arc<Mutex<Call>>) -> mpsc::Receiver<SimpleConnectionEvent> {
        let (snd, recv) = mpsc::channel(5);
        let handler = DisconnectEvents { snd };
        let mut lock = call.lock().await;
        for event in &[
            songbird::Event::Core(songbird::CoreEvent::ClientDisconnect),
            songbird::Event::Core(songbird::CoreEvent::DriverDisconnect),
            songbird::Event::Core(songbird::CoreEvent::DriverReconnect),
            songbird::Event::Core(songbird::CoreEvent::DriverConnect),
        ] {
            lock.add_global_event(*event, handler.clone());
        }
        recv
    }
}
#[derive(Debug, Clone)]
enum SimpleConnectionEvent {
    ClientDisconnect,
    DriverDisconnect,
    DriverConnect(Option<ChannelId>),
}
