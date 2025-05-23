#![feature(if_let_guard, duration_millis_float, try_blocks, let_chains)]
#![allow(dead_code)]
mod commands;
mod gemini;
mod structs;
mod user;
use commands::{ParsedCommand, WithFeedback};
use common::{
    anyhow::{self, Result},
    audio::AudioPromiseCommand,
    get_config, log,
    serenity::all::*,
    songbird::{
        self,
        events::{
            context_data::{VoiceData, VoiceTick},
            Event,
        },
        model::payload::Speaking,
        Call, CoreEvent, EventContext,
    },
    tokio::{
        self,
        sync::{mpsc, oneshot, RwLock},
        time::{Duration, Instant},
    },
    utils::{DeleteAfterFinish, TranscriptionMessage},
    PostSomething, WEB_CLIENT,
};
use futures::{
    stream::{FuturesOrdered, FuturesUnordered},
    StreamExt as _,
};
use serde::Deserialize as _;
use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};
const SAMPLES_PER_MILLISECOND: f64 = 96.0;
const MIN_SAMPLES_FOR_TRANSCRIPTION: usize = 64 * 1024;
pub static EVENTS: &[Event] = &[
    Event::Core(CoreEvent::SpeakingStateUpdate),
    Event::Core(CoreEvent::VoiceTick),
];
pub async fn transcription_thread(
    call: Arc<common::serenity::prelude::Mutex<Call>>,
    context: Context,
    otx: mpsc::UnboundedSender<(oneshot::Sender<Arc<str>>, AudioPromiseCommand)>,
    mut commands: mpsc::UnboundedReceiver<TranscriptionMessage>,
    // tx: mpsc::UnboundedSender<(String, UserId)>,
    tx: mpsc::UnboundedSender<(PostSomething, UserId)>,
    mut packets: mpsc::UnboundedReceiver<PacketData>,
) {
    let (responses, mut rx) = mpsc::unbounded_channel();
    let mut threads: Vec<user::TranscriptionThread> = Vec::new();
    let mut pending_commands = FuturesOrdered::new();
    let mut pending_feedback = FuturesOrdered::new();
    loop {
        tokio::select! {
            Some(command) = commands.recv() => {
                match command {
                    TranscriptionMessage::Stop => {
                        let mut threads = threads.into_iter().map(|t| t.stop()).collect::<FuturesUnordered<_>>();
                        while threads.next().await.is_some() {
                            log::trace!("Stopped a user thread");
                        }
                        break;
                    }
                }
            }
            Some(packet) = packets.recv() => {
                if let Some(thread) = threads.iter().find(|t| t.user_id == packet.user_id) {
                    thread.send(packet);
                } else {
                    let thread = user::TranscriptionThread::new(packet.user_id, responses.clone(), Arc::clone(&context.http));
                    thread.send(packet);
                    threads.push(thread);
                }
            }
            Some(response) = rx.recv() => {
                let ThreadResponse { response: WithFeedback { command, feedback }, user_id } = response;
                if let Some(audio) = feedback {
                    log::trace!("Playing audio for {}", user_id);
                    let mut call = call.lock().await;
                    if let Err(e) = call.play(audio.to_songbird()).add_event(
                        songbird::events::Event::Track(songbird::events::TrackEvent::End),
                        DeleteAfterFinish::new_disk(audio),
                    ) {
                        log::error!("Failed to register deleter: {:?}", e);
                    }
                }
                pending_commands.push_back(command);
                // match action {
                //     ThreadResponseAction::UploadFile { name, data } => {
                //         if let Err(e) = tx.send((PostSomething::Attachment { name, data }, user_id)) {
                //             log::error!("Failed to send audio to main thread: {:?}", e);
                //         }
                //     }
                //     ThreadResponseAction::SendMessage { content } => {
                //         if let Err(e) = tx.send((PostSomething::Text(content), user_id)) {
                //             log::error!("Failed to send message to main thread: {:?}", e);
                //         }
                //     }
                //     ThreadResponseAction::None => {
                //         log::trace!("No action");
                //     }
                // }
            }
            v = then(&mut pending_commands) => {
                match v {
                    Ok(ParsedCommand::Command(command)) => {
                        let (tx, rx) = oneshot::channel();
                        if let Err(e) = otx.send((tx, command)) {
                            log::error!("Failed to send command: {:?}", e);
                        }
                        pending_feedback.push_back(rx);
                    }
                    Ok(ParsedCommand::MetaCommand(command)) => {
                        log::info!("MetaCommand: {:?}", command);
                    }
                    Ok(ParsedCommand::AiTTS(video)) => {
                        // play the video through the call
                        let mut call = call.lock().await;
                        if let Err(e) = call.play(video.to_songbird()).add_event(
                            songbird::events::Event::Track(songbird::events::TrackEvent::End),
                            DeleteAfterFinish::new_disk(video),
                        ) {
                            log::error!("Failed to play video: {:?}", e);
                        }
                    }
                    Ok(ParsedCommand::None) => {
                        log::trace!("No command");
                    }
                    Err(e) => {
                        log::error!("Failed to get command: {:?}", e);
                    }
                }
            }
            v = then(&mut pending_feedback) => {
                match v {
                    Ok(string) => {
                        log::trace!("Feedback: {}", string);
                        if let Err(e) = tx.send((PostSomething::Text(string), context.cache.current_user().id)) {
                            log::error!("Failed to send feedback to main thread: {:?}", e);
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to get feedback: {:?}", e);
                    }
                }
            }
        }
    }
    log::trace!("Transcription thread stopped");
    let mut call = call.lock().await;
    call.remove_all_global_events();
}

async fn then<T>(queue: &mut FuturesOrdered<T>) -> <T as Future>::Output
where
    T: Future,
{
    match queue.next().await {
        Some(t) => t,
        None => Never::default().await,
    }
}
pub struct Never<T> {
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
    fn poll(self: Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<T> {
        std::task::Poll::Pending
    }
}
enum InnerThreadCommand {
    Stop,
    Process(PacketData),
}
#[derive(Debug)]
struct ThreadResponse {
    // audio: Option<Video>,
    // action: ThreadResponseAction,
    response: WithFeedback,
    user_id: UserId,
}
#[derive(Debug)]
enum ThreadResponseAction {
    None,
    UploadFile { name: String, data: Vec<u8> },
    SendMessage { content: String },
}
struct PacketDuration {
    duration: Duration,
}
impl PacketDuration {
    fn from_dur(duration: Duration) -> Self {
        Self { duration }
    }
    fn from_count(count: usize) -> Self {
        Self {
            duration: Duration::from_millis((count as f64 / SAMPLES_PER_MILLISECOND).ceil() as u64),
        }
    }
    fn to_packet_count(&self) -> usize {
        // should always be even
        ((self.duration.as_millis_f64() * SAMPLES_PER_MILLISECOND) / 2.0).floor() as usize * 2
    }
}
impl std::ops::Deref for PacketDuration {
    type Target = Duration;
    fn deref(&self) -> &Self::Target {
        &self.duration
    }
}
impl std::ops::DerefMut for PacketDuration {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.duration
    }
}
#[derive(Debug, Clone)]
pub struct VoiceEventSender {
    ssrc_to_user_id: Arc<RwLock<HashMap<u32, UserId>>>,
    sender: mpsc::UnboundedSender<PacketData>,
}
impl VoiceEventSender {
    pub fn new(sender: mpsc::UnboundedSender<PacketData>) -> Self {
        Self {
            ssrc_to_user_id: Arc::new(RwLock::new(HashMap::new())),
            sender,
        }
    }
}
#[async_trait]
impl songbird::EventHandler for VoiceEventSender {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            EventContext::SpeakingStateUpdate(Speaking { ssrc, user_id, .. }) => {
                if let Some(user_id) = user_id {
                    self.ssrc_to_user_id
                        .write()
                        .await
                        .insert(*ssrc, UserId::new(user_id.0));
                }
            }
            EventContext::VoiceTick(VoiceTick { speaking, .. }) => {
                for (ssrc, VoiceData { decoded_voice, .. }) in speaking.iter() {
                    let user_id = try {
                        let user_id = {
                            let ssrc_to_user_id = self.ssrc_to_user_id.read().await;
                            *ssrc_to_user_id.get(ssrc)?
                        };
                        match long_term_storage::User::mic_consent(user_id) {
                            Ok(true) => Some(user_id),
                            Ok(false) => None,
                            Err(e) => {
                                log::error!("Failed to get consent: {:?}", e);
                                None
                            }
                        }?
                    };
                    if let (Some(user_id), Some(audio)) = (
                        // ssrc_to_user_id.get(ssrc).and_then(|u| {
                        //     // common::global_data::consent_data::get_consent(*u).then_some(*u)
                        // }),
                        user_id,
                        decoded_voice,
                    ) {
                        if let Err(e) = self.sender.send(PacketData {
                            user_id,
                            // audio: audio
                            //     .chunks(2)
                            //     .flat_map(|c| match (c.first(), c.get(1)) {
                            //         (Some(&l), Some(&r)) => ((l >> 1) + (r >> 1)).to_le_bytes(),
                            //         (Some(&l), None) => l.to_le_bytes(),
                            //         _ => unreachable!(),
                            //     })
                            //     .collect::<Vec<u8>>(),
                            audio: audio.clone(),
                            received: Instant::now(),
                        }) {
                            log::error!("Failed to send packet data: {:?}", e);
                        }
                    } else {
                        // log::trace!("user_id: {:?}, audio: {:?}", user_id, decoded_voice.as_ref().map(|a| a.len()));
                    }
                }
            }
            e => {
                log::error!("unhandled type: {:?}", e);
            }
        }
        None
    }
}
pub struct PacketData {
    pub user_id: UserId,
    pub audio: Vec<i16>,
    pub received: Instant,
}
// async fn pcm_s16le_to_mp3(data: &[u8]) -> Result<Vec<u8>> {
//     // log::info!("Processing {} of audio", human_readable_bytes(data.len()));
//     // spawn ffmpeg, pipe in the data, pipe in the data, pipe out the mp3 data
//     let mut ffmpeg = tokio::process::Command::new("ffmpeg")
//         .arg("-hide_banner")
//         .args(["-loglevel", "panic"])
//         .args(["-f", "s16le"])
//         .args(["-ar", "48k"])
//         .args(["-ac", "1"])
//         .args(["-i", "pipe:0"])
//         .args(["-f", "mp3"])
//         .arg("pipe:1")
//         .stdin(std::process::Stdio::piped())
//         .stdout(std::process::Stdio::piped())
//         .stderr(std::process::Stdio::null())
//         .spawn()?;
//     // log::trace!("ffmpeg started");
//     let mut stdin = ffmpeg
//         .stdin
//         .take()
//         .ok_or(anyhow::anyhow!("Failed to get stdin"))?;
//     // log::trace!("ffmpeg stdin taken");
//     let mut stdout = ffmpeg
//         .stdout
//         .take()
//         .ok_or(anyhow::anyhow!("Failed to get stdout"))?;
//     // log::trace!("ffmpeg stdout taken");
//     // write all the data to the ffmpeg process
//     let handle = tokio::task::spawn(async move {
//         let mut mp3 = Vec::new();
//         // log::trace!("reading from ffmpeg");
//         stdout.read_to_end(&mut mp3).await?;
//         // log::trace!("ffmpeg read {} of audio", human_readable_bytes(mp3.len()));
//         Ok::<_, anyhow::Error>(mp3)
//     });
//     // log::trace!("writing to ffmpeg");
//     stdin.write_all(data).await?;
//     // log::trace!("ffmpeg data written");
//     stdin.flush().await?;
//     // log::trace!("ffmpeg data flushed");
//     drop(stdin);
//     // log::trace!("ffmpeg stdin dropped");
//     let mp3 = handle.await??;
//     // log::info!("Processed {} of audio", human_readable_bytes(mp3.len()));
//     if mp3.is_empty() {
//         return Err(anyhow::anyhow!("Failed to get mp3 data"));
//     }
//     Ok(mp3)
// }
pub fn human_readable_bytes(size: usize) -> String {
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let mut size = size as f64;
    let mut i = 0;
    while size >= 1024.0 {
        size /= 1024.0;
        i += 1;
    }
    format!("{:.2} {}", size, units.get(i).unwrap_or(&"??"))
}
async fn transcribe(audio: &[i16]) -> Result<TranscriptionResult> {
    let cfg = get_config();
    let response = WEB_CLIENT
        .post(format!(
            "{}/transcribe/raw?format=s16le&sample_rate=48000&channels=2",
            cfg.transcribe_url
        ))
        .header("x-token", &cfg.transcribe_token)
        .header("Content-Type", "multipart/form-data")
        .body(
            audio
                .iter()
                .flat_map(|i| i.to_le_bytes().to_vec())
                .collect::<Vec<u8>>(),
        )
        .send()
        .await?
        .json::<RequestResponse>()
        .await?;
    let request_id = match response {
        RequestResponse::Success { request_id } => request_id,
        RequestResponse::Error { error } => {
            return Err(anyhow::anyhow!("Failed to start transcription: {}", error))
        }
    };
    let url = format!("{}/result/{}/wait", cfg.transcribe_url, request_id);
    let response = WEB_CLIENT
        .get(url)
        .header("x-token", cfg.transcribe_token)
        .send()
        .await?
        .json::<TranscriptionResponse>()
        .await?;
    match response {
        TranscriptionResponse::Pending { status } => Err(anyhow::anyhow!(
            "Transcription is pending, this should not happen: {:?}",
            status
        )),
        TranscriptionResponse::Error { error } => {
            Err(anyhow::anyhow!("Failed to get transcription: {}", error))
        }
        TranscriptionResponse::Success { result } => Ok(result),
    }
}
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum RequestResponse {
    Error { error: String },
    Success { request_id: String },
}
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
enum TranscriptionResponse {
    Error {
        error: String,
    },
    #[serde(deserialize_with = "deserialize_pending")]
    Pending {
        status: PendingStatus,
    },
    Success {
        result: TranscriptionResult,
    },
}
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TranscriptionResult {
    segments: Vec<TranscriptionSegment>,
}
impl std::fmt::Display for TranscriptionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut segments = self.segments.clone();
        segments.sort_by(|a, b| {
            a.start
                .partial_cmp(&b.start)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for segment in segments.iter() {
            write!(f, "{}", segment.text)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TranscriptionSegment {
    start: f64,
    text: String,
}
#[derive(Debug, Clone)]
enum PendingStatus {
    Pending { position: u32 },
    InProgress,
}
fn deserialize_pending<'de, D>(deserializer: D) -> Result<PendingStatus, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if let serde_json::Value::Object(map) = value {
        if let Some(serde_json::Value::String(status)) = map.get("status") {
            if status == "pending" {
                if let Some(serde_json::Value::Number(position)) = map.get("position") {
                    if let Some(position) = position.as_u64() {
                        return Ok(PendingStatus::Pending {
                            position: position as u32,
                        });
                    }
                }
            } else if status == "in-progress" {
                return Ok(PendingStatus::InProgress);
            }
        }
    }
    Err(serde::de::Error::custom("Invalid pending status"))
}

static MALE_NAMES: &[&str] = &[
    "Tom",
    "Jerry",
    "Bob",
    "John",
    "Bill",
    "Joe",
    "Jim",
    "Tim",
    "Sam",
    "Max",
    "Ben",
    "Dan",
    "Ted",
    "Don",
    "Ron",
    "Ed",
    "Roy",
    "Leo",
    "Lee",
    "Ray",
    "Rex",
    "Jay",
    "Sir Fartmire the Third",
];
