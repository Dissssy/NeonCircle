use crate::{
    commands::music::{AudioPromiseCommand, OrToggle},
    video::Video,
};
use anyhow::Result;
use serde::Deserialize as _;
use serenity::{
    all::*,
    futures::{stream::FuturesUnordered, StreamExt as _},
};
use songbird::{
    events::context_data::{VoiceData, VoiceTick},
    model::payload::Speaking,
    Call, CoreEvent, Event, EventContext,
};
use std::{collections::HashMap, pin::Pin, sync::Arc};
use tokio::sync::{mpsc, oneshot, Mutex};
const SAMPLES_PER_MILLISECOND: f64 = 1920.0 / 20.0;
struct VoiceEventSender {
    ssrc_to_user_id: Arc<Mutex<HashMap<u32, UserId>>>,
    sender: PacketSender,
}
#[async_trait]
impl songbird::EventHandler for VoiceEventSender {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        match ctx {
            EventContext::SpeakingStateUpdate(Speaking { ssrc, user_id, .. }) => {
                let mut ssrc_to_user_id = self.ssrc_to_user_id.lock().await;
                if let Some(user_id) = user_id {
                    ssrc_to_user_id.insert(*ssrc, UserId::new(user_id.0));
                }
            }

            EventContext::VoiceTick(VoiceTick { speaking, .. }) => {
                for (ssrc, VoiceData { decoded_voice, .. }) in speaking.iter() {
                    let ssrc_to_user_id = self.ssrc_to_user_id.lock().await;
                    if let Some(user_id) = ssrc_to_user_id.get(ssrc) {
                        if self
                            .sender
                            .send(PacketData {
                                user_id: *user_id,
                                audio: decoded_voice.as_ref().cloned().unwrap_or_default(),
                                received: std::time::Instant::now(),
                            })
                            .is_err()
                        {}
                    }
                }
            }

            e => {
                println!("unhandled type: {}", get_name(e));
            }
        }
        None
    }
}
fn get_name(e: &EventContext) -> &'static str {
    match e {
        EventContext::Track(_) => "Track",
        EventContext::SpeakingStateUpdate(_) => "SpeakingStateUpdate",
        EventContext::VoiceTick(_) => "VoiceTick",
        EventContext::DriverConnect(_) => "DriverConnect",
        EventContext::DriverReconnect(_) => "DriverReconnect",
        EventContext::DriverDisconnect(_) => "DriverDisconnect",
        _ => "Unknown",
    }
}
type PacketSender = mpsc::UnboundedSender<PacketData>;
pub struct PacketData {
    pub user_id: UserId,

    pub audio: Vec<i16>,
    pub received: std::time::Instant,
}
pub struct VoiceDataManager {
    user_streams: HashMap<UserId, (Vec<i16>, Option<std::time::Instant>)>,
    receiver: mpsc::UnboundedReceiver<PacketData>,
    disabled_for: std::collections::HashMap<UserId, bool>,
    http: Arc<Http>,
    command: mpsc::UnboundedSender<(
        oneshot::Sender<String>,
        crate::commands::music::AudioPromiseCommand,
    )>,
}
static EVENTS: &[CoreEvent] = &[CoreEvent::SpeakingStateUpdate, CoreEvent::VoiceTick];
impl VoiceDataManager {
    pub async fn new(
        call: Arc<Mutex<Call>>,
        http: Arc<Http>,
        command: mpsc::UnboundedSender<(
            oneshot::Sender<String>,
            crate::commands::music::AudioPromiseCommand,
        )>,
    ) -> Self {
        let ssrc_to_user_id = Arc::new(Mutex::new(HashMap::new()));
        let (sender, receiver) = mpsc::unbounded_channel::<PacketData>();
        for event in EVENTS {
            call.lock().await.add_global_event(
                (*event).into(),
                VoiceEventSender {
                    ssrc_to_user_id: ssrc_to_user_id.clone(),
                    sender: sender.clone(),
                },
            );
        }
        Self {
            user_streams: HashMap::new(),
            receiver,
            disabled_for: HashMap::new(),
            http,
            command,
        }
    }
    pub async fn get_streams(&mut self) -> Vec<(UserId, Vec<i16>)> {
        self.consume_packets().await;
        let mut streams = Vec::new();
        let now = std::time::Instant::now();
        for (user_id, (audio, last_received)) in self.user_streams.iter_mut() {
            if match last_received {
                Some(last_received) => now.duration_since(*last_received).as_secs_f64() > 0.2,
                None => false,
            } {
                let audio = std::mem::take(audio);
                std::mem::take(last_received);
                if audio.len() * std::mem::size_of::<i16>() < 120 * 1024 {
                    continue;
                }
                let max = audio.iter().map(|a| a.abs()).max().unwrap_or(0);
                if max < 1000 {
                    continue;
                }
                streams.push((*user_id, audio));
            }
        }
        streams
    }
    async fn consume_packets(&mut self) {
        while let Ok(packet) = self.receiver.try_recv() {
            let (user_id, audio) = (packet.user_id, packet.audio);
            let user_is_bot = match self.disabled_for.entry(user_id) {
                std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    match self.http.get_user(user_id).await {
                        Ok(user) => {
                            entry.insert(user.bot);
                            user.bot
                        }
                        Err(e) => {
                            eprintln!("Error getting user: {:?}", e);
                            true
                        }
                    }
                }
            };
            if user_is_bot {
                continue;
            }
            let (audio_buf, received) = self
                .user_streams
                .entry(user_id)
                .or_insert((Vec::new(), None));
            if let Some(received) = received {
                let bytes_to_fill = ((packet.received.duration_since(*received).as_millis_f64()
                    * SAMPLES_PER_MILLISECOND)
                    .floor() as usize)
                    .saturating_sub(audio.len());
                if bytes_to_fill > (SAMPLES_PER_MILLISECOND * 50.0) as usize {
                    audio_buf.extend(std::iter::repeat(0).take(bytes_to_fill));
                }
            }
            audio_buf.extend(audio);
            received.replace(packet.received);
        }
    }
}
pub async fn transcription_thread(
    mut transcribe: VoiceDataManager,
    mut transcribereturn: mpsc::Receiver<()>,
    recvtext: mpsc::UnboundedSender<(String, UserId)>,
    call: Arc<Mutex<Call>>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    let (url, key) = {
        let config = crate::Config::get();
        (config.transcribe_url, config.transcribe_token)
    };
    let mut pending_responses = FuturesUnordered::new();
    let mut responses_to_await = FuturesUnordered::new();
    let mut pending_parse = FuturesUnordered::new();
    loop {
        tokio::select! {
            _ = interval.tick() => {
                for (user, audio) in transcribe.get_streams().await {


                    let audio = audio.chunks(2).map(|c| ((c.get(1).map(|c1| (c[0] as f64 + *c1 as f64) / 2.)).unwrap_or(c[0] as f64)).floor() as i16).collect::<Vec<i16>>();



                    //curl -X 'POST' \





                    let response = crate::WEB_CLIENT
                        .post(format!("{}/transcribe/raw?format=s16le&sample_rate=48000&channels=1", url))
                        .header("x-token", key.clone())
                        .header("Content-Type", "multipart/form-data")
                        .body(audio.iter().flat_map(|i| i.to_le_bytes().to_vec()).collect::<Vec<u8>>())
                        .send()
                        .await;


                    match response {
                        Ok(response) => {
                            match response.text().await.map(|b| serde_json::from_str::<RequestResponse>(&b).map_err(|e| format!("{:?}\n{}", e, b))) {
                                Ok(Ok(RequestResponse::Success { request_id })) => {
                                    pending_responses.push(
                                        wait_for_transcription(
                                            crate::WEB_CLIENT.clone(),
                                            url.clone(),
                                            key.clone(),
                                            request_id,
                                            user,
                                        ),
                                    );
                                }
                                Ok(Ok(RequestResponse::Error { error })) => {
                                    eprintln!("Issue with audio: {}", error);
                                }
                                Ok(Err(e)) => {
                                    eprintln!("Error deserializing response: {:?}", e);
                                }
                                Err(e) => {
                                    eprintln!("Error getting id from transcription service: {:?}", e);
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Error sending audio to transcription service: {:?}", e);
                            break;
                        }
                    }
                }
            }
            _ = transcribereturn.recv() => {
                break;
            }
            Some(response) = responses_to_await.next() => {
                match response {
                    Ok(string) => {
                        if let Err(e) = recvtext.send((string, transcribe.http.get_current_user().await.expect("Current user is none?").id)) {
                            eprintln!("Error sending text: {:?}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("Error getting response: {:?}", e);
                    }
                }
            }
            Some(response) = pending_responses.next() => {
                match response {
                    Ok((Ok(TranscriptionResponse::Success { result }), user)) => {

                        let result = format!("{}", result).trim().to_string();

                        let http = Arc::clone(&transcribe.http);
                        pending_parse.push(tokio::spawn(async move {
                            parse_commands(result, user, http).await
                        }));
                    }
                    Ok((Ok(TranscriptionResponse::Pending { status }), _))  => {
                        match status {
                            PendingStatus::Pending { position } => {
                                println!("Transcription is pending, position: {}", position);
                            }
                            PendingStatus::InProgress => {
                                println!("Transcription is in progress");
                            }
                        }
                    }
                    Ok((Ok(TranscriptionResponse::Error { error }), _)) => {
                        eprintln!("Error getting transcription result: {}", error);
                    }
                    Ok((Err(e), _)) => {
                        eprintln!("Error deserializing transcription response: {:?}", e);
                    }
                    Err(e) => {
                        eprintln!("Error getting transcription result: {:?}", e);
                    }
                }
            }
            Some(result) = pending_parse.next() => {


                let (result, user, WithFeedback { command, feedback }) = match result {
                    Ok((result, user, with_feedback)) => (result, user, with_feedback),
                    Err(e) => {
                        eprintln!("Error parsing command: {:?}", e);
                        continue;
                    }
                };

                if let Some(feedback) = feedback {
                    let mut call = call.lock().await;

                    let handle = call.play_input(feedback.to_songbird());
                    let _ = handle.set_volume(0.8);
                    if let Err(e) = feedback.delete_when_finished(handle).await {
                        eprintln!("Error deleting feedback: {:?}", e);
                    }
                }

                match command.await {





                    Ok(ParsedCommand::None) => {
                        if ![
                            "bye.", "thank you."
                        ].contains(&result.to_lowercase().as_str()) {
                            if let Err(e) = recvtext.send((result, user)) {
                                eprintln!("Error sending text: {:?}", e);
                                break;
                            }
                        }
                    }
                    Ok(ParsedCommand::MetaCommand(command)) => {
                        match command {
                            Command::NoConsent => {
                                if let Err(e) = recvtext.send((format!("{} opted out.", user.mention()), transcribe.http.get_current_user().await.expect("Current user is none?").id)) {
                                    eprintln!("Error sending text: {:?}", e);
                                    break;
                                }
                                transcribe.disabled_for.insert(user, true);
                            }
                        }
                    }
                    Ok(ParsedCommand::Command(command)) => {
                        let (tx, rx) = oneshot::channel();
                        if let Err(e) = transcribe.command.send((tx, command.clone())) {
                            eprintln!("Error sending command: {:?}", e);
                        }
                        responses_to_await.push(rx);
                    }
                    Err(e) => {

                        if let Some(feedback) = get_speech(&format!("Error: {:?}", e)).await {
                            let mut call = call.lock().await;

                            let handle = call.play_input(feedback.to_songbird());
                            let _ = handle.set_volume(0.8);
                            if let Err(e) = feedback.delete_when_finished(handle).await {
                                eprintln!("Error deleting feedback: {:?}", e);
                            }
                        } else {
                            eprintln!("Error getting error speech for {:?}", e);
                        }
                    }
                }
            }
        }
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
async fn wait_for_transcription(
    reqwest: reqwest::Client,
    url: String,
    key: String,
    request_id: String,
    user: UserId,
) -> Result<(Result<TranscriptionResponse, String>, UserId), reqwest::Error> {
    let url = format!("{}/result/{}/wait", url, request_id);
    let response = reqwest
        .get(url)
        .header("x-token", key)
        .send()
        .await?
        .text()
        .await
        .map(|b| {
            serde_json::from_str::<TranscriptionResponse>(&b).map_err(|e| format!("{:?}\n{}", e, b))
        });
    response.map(|r| (r, user))
}
fn filter_input(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .collect::<Vec<&str>>()
        .join(" ")
}
lazy_static::lazy_static!(
    pub static ref ALERT_PHRASES: Alerts = {

        let file = crate::Config::get().alert_phrases_path;
        let text = std::fs::read_to_string(file).expect("Error reading alert phrases file");
        let mut the = serde_json::from_str::<Alerts>(&text).expect("Error deserializing alert phrases");

        for alert in &mut the.phrases {
            alert.main.push(' ');
            for alias in &mut alert.aliases {
                alias.push(' ');
            }
        }
        the
    };
);
#[derive(Debug, serde::Deserialize)]
pub struct Alerts {
    phrases: Vec<Alert>,
}
impl std::fmt::Display for Alerts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for alert in &self.phrases {
            writeln!(f, "{}", alert)?;
        }
        Ok(())
    }
}
impl Alerts {
    fn filter(&self, s: String) -> String {
        let mut s = s;
        for alert in &self.phrases {
            for alias in &alert.aliases {
                s = s.replace(alias, &alert.main);
            }
        }
        s
    }
    fn get_alert(&'static self, s: &str) -> Option<&'static Alert> {
        self.phrases.iter().find(|a| s.contains(&a.main))
    }
}
#[derive(Debug, serde::Deserialize)]
pub struct Alert {
    main: String,
    aliases: Vec<String>,
}
impl std::fmt::Display for Alert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: [{}]", self.main, self.aliases.join(", "))
    }
}
async fn parse_commands(s: String, u: UserId, http: Arc<Http>) -> (String, UserId, WithFeedback) {
    if s.is_empty() {
        return (
            s,
            u,
            WithFeedback::new_without_feedback(Box::pin(async move { Ok(ParsedCommand::None) }))
                .await,
        );
    }
    let filtered = filter_input(&s);
    if filtered.is_empty() {
        return (
            s,
            u,
            WithFeedback::new_without_feedback(Box::pin(async move { Ok(ParsedCommand::None) }))
                .await,
        );
    }
    if filtered.contains("i do not consent to being recorded") {
        return (
            s,
            u,
            WithFeedback::new_without_feedback(Box::pin(async move {
                Ok(ParsedCommand::MetaCommand(Command::NoConsent))
            }))
            .await,
        );
    }
    let with_aliases = ALERT_PHRASES.filter(filtered);
    let (command, args): (&str, Vec<&str>) = {
        if let Some(alert) = ALERT_PHRASES.get_alert(&with_aliases) {
            let mut split = with_aliases.split(&alert.main);
            split.next();
            let rest = split.next().unwrap_or("");
            let mut split = rest.split_whitespace();
            let command = match split.next() {
                Some(command) => command,

                None => {
                    return (
                        s,
                        u,
                        WithFeedback::new_with_feedback(
                            Box::pin(async move { Ok(ParsedCommand::None) }),
                            "You need to say a command",
                        )
                        .await,
                    )
                }
            };
            let args = split.collect();
            (command, args)
        } else {
            return (
                s,
                u,
                WithFeedback::new_without_feedback(Box::pin(
                    async move { Ok(ParsedCommand::None) },
                ))
                .await,
            );
        }
    };
    match command {
        t if ["play", "add", "queue", "played"].contains(&t) => {
            let query = args.join(" ");
            let http = Arc::clone(&http);
            if query.replace(' ', "").contains("wonderwall") {
                (
                    s,
                    u,
                    WithFeedback::new_with_feedback(
                        Box::pin(async move {
                            Ok(ParsedCommand::Command(AudioPromiseCommand::Play(
                                get_videos(query, http, u).await?,
                            )))
                        }),
                        "Anyway, here's wonderwall",
                    )
                    .await,
                )
            } else {
                let response = format!("Adding {} to the queue", query);
                (
                    s,
                    u,
                    WithFeedback::new_with_feedback(
                        Box::pin(async move {
                            Ok(ParsedCommand::Command(AudioPromiseCommand::Play(
                                get_videos(query, http, u).await?,
                            )))
                        }),
                        &response,
                    )
                    .await,
                )
            }
        }
        t if ["stop", "leave", "disconnect"].contains(&t) => (
            s,
            u,
            WithFeedback::new_with_feedback(
                Box::pin(async move {
                    Ok(ParsedCommand::Command(AudioPromiseCommand::Stop(Some(
                        tokio::time::Duration::from_millis(2500),
                    ))))
                }),
                "Goodbuy, my friend",
            )
            .await,
        ),
        t if ["skip", "next"].contains(&t) => (
            s,
            u,
            WithFeedback::new_with_feedback(
                Box::pin(async move { Ok(ParsedCommand::Command(AudioPromiseCommand::Skip)) }),
                "Skipping",
            )
            .await,
        ),
        t if ["pause"].contains(&t) => (
            s,
            u,
            WithFeedback::new_with_feedback(
                Box::pin(async move {
                    Ok(ParsedCommand::Command(AudioPromiseCommand::Paused(
                        OrToggle::Specific(true),
                    )))
                }),
                "Pausing",
            )
            .await,
        ),
        t if ["resume", "unpause"].contains(&t) => (
            s,
            u,
            WithFeedback::new_with_feedback(
                Box::pin(async move {
                    Ok(ParsedCommand::Command(AudioPromiseCommand::Paused(
                        OrToggle::Specific(false),
                    )))
                }),
                "Resuming",
            )
            .await,
        ),
        t if ["volume", "vol"].contains(&t) => {
            if let Some(vol) = attempt_to_parse_number(&args) {
                if vol <= 100 {
                    (
                        s,
                        u,
                        WithFeedback::new_with_feedback(
                            Box::pin(async move {
                                Ok(ParsedCommand::Command(AudioPromiseCommand::Volume(
                                    vol.clamp(0, 100) as f64 / 100.0,
                                )))
                            }),
                            &format!("Setting volyume to {}%", humanize_number(vol)),
                        )
                        .await,
                    )
                } else {
                    (
                        s,
                        u,
                        WithFeedback::new_with_feedback(
                            Box::pin(async move { Ok(ParsedCommand::None) }),
                            "Volyume must be between zero and one hundred",
                        )
                        .await,
                    )
                }
            } else {
                (
                    s,
                    u,
                    WithFeedback::new_with_feedback(
                        Box::pin(async move { Ok(ParsedCommand::None) }),
                        "You need to say a number for the volyume",
                    )
                    .await,
                )
            }
        }
        t if ["remove", "delete"].contains(&t) => {
            if let Some(index) = attempt_to_parse_number(&args) {
                (
                    s,
                    u,
                    WithFeedback::new_with_feedback(
                        Box::pin(async move {
                            Ok(ParsedCommand::Command(AudioPromiseCommand::Remove(index)))
                        }),
                        &format!("Removing song {} from queue", index),
                    )
                    .await,
                )
            } else {
                (
                    s,
                    u,
                    WithFeedback::new_with_feedback(
                        Box::pin(async move { Ok(ParsedCommand::None) }),
                        "You need to say a number for the index",
                    )
                    .await,
                )
            }
        }
        t if ["say", "echo"].contains(&t) => (
            s,
            u,
            WithFeedback::new_with_feedback(
                Box::pin(async move { Ok(ParsedCommand::None) }),
                &args.join(" "),
            )
            .await,
        ),
        unknown => (
            s,
            u,
            WithFeedback::new_with_feedback(
                Box::pin(async move { Ok(ParsedCommand::None) }),
                &format!("Unknown command. {}", unknown),
            )
            .await,
        ),
    }
}
#[derive(Debug)]
enum ParsedCommand {
    None,
    MetaCommand(Command),
    Command(AudioPromiseCommand),
}
struct WithFeedback {
    command: Pin<Box<dyn std::future::Future<Output = Result<ParsedCommand>> + Send>>,
    feedback: Option<Video>,
}
impl std::fmt::Debug for WithFeedback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WithFeedback")
            .field("command", &"Future")
            .field("feedback", &self.feedback)
            .finish()
    }
}
impl WithFeedback {
    async fn new_with_feedback(
        command: Pin<
            Box<
                dyn std::future::Future<
                        Output = std::prelude::v1::Result<ParsedCommand, anyhow::Error>,
                    > + Send,
            >,
        >,
        feedback: &str,
    ) -> Self {
        Self {
            command,
            feedback: get_speech(feedback).await,
        }
    }
    async fn new_without_feedback(
        command: Pin<
            Box<
                dyn std::future::Future<
                        Output = std::prelude::v1::Result<ParsedCommand, anyhow::Error>,
                    > + Send,
            >,
        >,
    ) -> Self {
        Self {
            command,
            feedback: None,
        }
    }
}
#[derive(Debug)]
enum Command {
    NoConsent,
}
fn attempt_to_parse_number(args: &[&str]) -> Option<usize> {
    let mut num = 0;
    for word in args {
        match *word {
            "zero" => num += 0,
            "one" => num += 1,
            "two" => num += 2,
            "three" => num += 3,
            "four" => num += 4,
            "five" => num += 5,
            "six" => num += 6,
            "seven" => num += 7,
            "eight" => num += 8,
            "nine" => num += 9,
            "ten" => num += 10,
            "eleven" => num += 11,
            "twelve" => num += 12,
            "thirteen" => num += 13,
            "fourteen" => num += 14,
            "fifteen" => num += 15,
            "sixteen" => num += 16,
            "seventeen" => num += 17,
            "eighteen" => num += 18,
            "nineteen" => num += 19,
            "twenty" => num += 20,
            "thirty" => num += 30,
            "forty" => num += 40,
            "fifty" => num += 50,
            "sixty" => num += 60,
            "seventy" => num += 70,
            "eighty" => num += 80,
            "ninety" => num += 90,
            "hundred" => num *= 100,
            "thousand" => num *= 1000,
            "million" => num *= 1000000,
            "billion" => num *= 1000000000,
            "trillion" => num *= 1000000000000,
            "quadrillion" => num *= 1000000000000000,
            "quintillion" => num *= 1000000000000000000,

            n if let Ok(n) = n.parse::<usize>() => num += n,
            _ => {
                eprintln!("Error parsing number: {:?} from {}", word, args.join(" "));
                return None;
            }
        }
    }
    Some(num)
}
pub fn humanize_number(n: usize) -> String {
    if n == 0 {
        return "zero".to_owned();
    }
    let mut n = n;
    let mut words = Vec::new();
    if n >= 1000 {
        words.push(humanize_number(n / 1000));
        words.push("thousand".to_owned());
        n %= 1000;
    }
    if n >= 100 {
        words.push(humanize_number(n / 100));
        words.push("hundred".to_owned());
        n %= 100;
    }
    if !words.is_empty() && n > 0 {
        words.push("and".to_owned());
    }
    if n >= 20 {
        match n / 10 {
            2 => words.push("twenty".to_owned()),
            3 => words.push("thirty".to_owned()),
            4 => words.push("forty".to_owned()),
            5 => words.push("fifty".to_owned()),
            6 => words.push("sixty".to_owned()),
            7 => words.push("seventy".to_owned()),
            8 => words.push("eighty".to_owned()),
            9 => words.push("ninety".to_owned()),
            _ => unreachable!(),
        }
        n %= 10;
    }
    if n >= 10 {
        match n {
            10 => words.push("ten".to_owned()),
            11 => words.push("eleven".to_owned()),
            12 => words.push("twelve".to_owned()),
            13 => words.push("thirteen".to_owned()),
            14 => words.push("fourteen".to_owned()),
            15 => words.push("fifteen".to_owned()),
            16 => words.push("sixteen".to_owned()),
            17 => words.push("seventeen".to_owned()),
            18 => words.push("eighteen".to_owned()),
            19 => words.push("nineteen".to_owned()),
            _ => unreachable!(),
        }
        n = 0;
    }
    if n > 0 {
        match n {
            1 => words.push("one".to_owned()),
            2 => words.push("two".to_owned()),
            3 => words.push("three".to_owned()),
            4 => words.push("four".to_owned()),
            5 => words.push("five".to_owned()),
            6 => words.push("six".to_owned()),
            7 => words.push("seven".to_owned()),
            8 => words.push("eight".to_owned()),
            9 => words.push("nine".to_owned()),
            _ => unreachable!(),
        }
    }
    words.join(" ")
}
async fn get_speech(text: &str) -> Option<Video> {
    let text = if text.ends_with('.') {
        text.to_owned()
    } else {
        format!("{}.", text)
    };
    match crate::sam::get_speech(&text) {
        Ok(vid) => Some(vid),
        Err(e) => {
            eprintln!("Error getting speech: {:?}", e);
            None
        }
    }
}
async fn get_videos(
    query: String,
    http: Arc<Http>,
    u: UserId,
) -> Result<Vec<crate::commands::music::MetaVideo>> {
    let vids = crate::video::Video::get_video(&query, true, true).await;
    match vids {
        Ok(vids) => {
            let mut truevideos = Vec::new();
            #[cfg(feature = "tts")]
            let key = crate::youtube::get_access_token().await;
            for v in vids {
                let title = match v.clone() {
                    crate::commands::music::VideoType::Disk(v) => v.title,
                    crate::commands::music::VideoType::Url(v) => v.title,
                };
                #[cfg(feature = "tts")]
                if let Ok(key) = key.as_ref() {
                    truevideos.push(crate::commands::music::MetaVideo {
                        video: v,
                        ttsmsg: Some(crate::commands::music::LazyLoadedVideo::new(tokio::spawn(
                            crate::youtube::get_tts(title.clone(), key.clone(), None),
                        ))),
                        title,
                        author: http.get_user(u).await.ok().map(|u| {
                            crate::commands::music::Author {
                                name: u.name.clone(),
                                pfp_url: u
                                    .avatar_url()
                                    .clone()
                                    .unwrap_or(u.default_avatar_url().clone()),
                            }
                        }),
                    })
                } else {
                    truevideos.push(crate::commands::music::MetaVideo {
                        video: v,
                        ttsmsg: None,
                        title,
                        author: http.get_user(u).await.ok().map(|u| {
                            crate::commands::music::Author {
                                name: u.name.clone(),
                                pfp_url: u.avatar_url().unwrap_or(u.default_avatar_url().clone()),
                            }
                        }),
                    });
                }
                #[cfg(not(feature = "tts"))]
                truevideos.push(MetaVideo { video: v, title });
            }
            Ok(truevideos)
        }
        Err(e) => {
            eprintln!("Error getting video: {:?}", e);
            Err(anyhow::anyhow!("Error getting audio."))
        }
    }
}
