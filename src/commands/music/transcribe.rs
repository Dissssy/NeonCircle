use super::{AudioPromiseCommand, RawMessage};
use crate::{video::Video, voice_events::PostSomething, youtube::TTSVoice};
use anyhow::Result;
use rand::seq::SliceRandom;
use serenity::all::*;
use songbird::{input::Input, tracks::TrackHandle, typemap::TypeMapKey, Call};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
#[derive(Debug, Clone)]
pub struct Transcribe;
#[async_trait]
impl crate::CommandTrait for Transcribe {
    fn register(&self) -> CreateCommand {
        CreateCommand::new(self.name())
            .description("Transcribe this channel")
            .set_options(vec![CreateCommandOption::new(
                CommandOptionType::Boolean,
                "value",
                "Specific value, otherwise toggle",
            )])
    }
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) {
        if let Err(e) = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Defer(
                    CreateInteractionResponseMessage::new().ephemeral(true),
                ),
            )
            .await
        {
            log::error!("Failed to create interaction response: {:?}", e);
        }
        let guild_id = match interaction.guild_id {
            Some(id) => id,
            None => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new()
                            .content("This command can only be used in a server"),
                    )
                    .await
                {
                    log::error!("Failed to edit original interaction response: {:?}", e);
                }
                return;
            }
        };
        let options = interaction.data.options();
        let option = match options.iter().find_map(|o| match o.name {
            "value" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Boolean(o)) => super::OrToggle::Specific(*o),
            None => super::OrToggle::Toggle,
            _ => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content("This command requires an option"),
                    )
                    .await
                {
                    log::error!("Failed to edit original interaction response: {:?}", e);
                }
                return;
            }
        };
        let ungus = {
            let bingus = ctx.data.read().await;
            let bungly = bingus.get::<super::VoiceData>();
            bungly.cloned()
        };
        if let (Some(v), Some(member)) = (ungus, interaction.member.as_ref()) {
            let next_step = {
                v.write()
                    .await
                    .mutual_channel(ctx, &guild_id, &member.user.id)
            };
            match next_step {
                super::VoiceAction::UserNotConnected => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content("You're not in a voice channel"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::InDifferent(_channel) => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new()
                                .content("I'm in a different voice channel"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::Join(_channel) => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content(
                                "I'm not in a channel, if you want me to join use /join or /play",
                            ),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::InSame(_channel) => {
                    let em = match super::get_transcribe_channel_handler(ctx, &guild_id).await {
                        Ok(v) => v,
                        Err(e) => {
                            log::error!("Failed to get transcribe channel handler: {:?}", e);
                            if let Err(e) = interaction
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new().content("Failed to get handler"),
                                )
                                .await
                            {
                                log::error!(
                                    "Failed to edit original interaction response: {:?}",
                                    e
                                );
                            }
                            return;
                        }
                    };
                    match option {
                        super::OrToggle::Specific(option) => {
                            if option {
                                if let Err(res) =
                                    em.write().await.register(interaction.channel_id).await
                                {
                                    if let Err(e) = interaction
                                        .edit_response(
                                            &ctx.http,
                                            EditInteractionResponse::new()
                                                .content(format!("Error registering: {:?}", res)),
                                        )
                                        .await
                                    {
                                        log::error!(
                                            "Failed to edit original interaction response: {:?}",
                                            e
                                        );
                                    }
                                } else if let Err(e) = interaction
                                    .edit_response(
                                        &ctx.http,
                                        EditInteractionResponse::new().content("Registered"),
                                    )
                                    .await
                                {
                                    log::error!(
                                        "Failed to edit original interaction response: {:?}",
                                        e
                                    );
                                }
                            } else if let Err(res) =
                                em.write().await.unregister(interaction.channel_id).await
                            {
                                if let Err(e) = interaction
                                    .edit_response(
                                        &ctx.http,
                                        EditInteractionResponse::new()
                                            .content(format!("Error unregistering: {:?}", res)),
                                    )
                                    .await
                                {
                                    log::error!(
                                        "Failed to edit original interaction response: {:?}",
                                        e
                                    );
                                }
                            } else if let Err(e) = interaction
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new().content("Unregistered"),
                                )
                                .await
                            {
                                log::error!(
                                    "Failed to edit original interaction response: {:?}",
                                    e
                                );
                            }
                        }
                        super::OrToggle::Toggle => {
                            if let Err(res) = em.write().await.toggle(interaction.channel_id).await
                            {
                                if let Err(e) = interaction
                                    .edit_response(
                                        &ctx.http,
                                        EditInteractionResponse::new()
                                            .content(format!("Error toggling: {:?}", res)),
                                    )
                                    .await
                                {
                                    log::error!(
                                        "Failed to edit original interaction response: {:?}",
                                        e
                                    );
                                }
                            } else if let Err(e) = interaction
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new().content("Toggled"),
                                )
                                .await
                            {
                                log::error!(
                                    "Failed to edit original interaction response: {:?}",
                                    e
                                );
                            }
                        }
                    }
                }
            }
        } else if let Err(e) = interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("TELL ETHAN THIS SHOULD NEVER HAPPEN :("),
            )
            .await
        {
            log::error!("Failed to edit original interaction response: {:?}", e);
        }
    }
    fn name(&self) -> &str {
        "transcribe"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &CommandInteraction) -> Result<()> {
        Ok(())
    }
}
pub struct Handler {
    call: Arc<Mutex<Call>>,
    queue: Vec<RawMessage>,
    prepared_next: Option<Deleteable>,
    current_handle: Option<(TrackHandle, Deleteable)>,
    channel_names: HashMap<String, Deleteable>,
    last_channel_name: String,
    waiting_on: Option<String>,
}
#[derive(Debug, Clone)]
pub enum Deleteable {
    Delete(Video),
    Keep(Video),
}
impl Deleteable {
    pub fn delete(&self) -> Result<()> {
        match self {
            Self::Delete(v) => v.delete(),
            Self::Keep(_) => Ok(()),
        }
    }
    pub fn force_delete(&self) -> Result<()> {
        self.get_video().delete()
    }
    pub fn get_video(&self) -> &Video {
        match self {
            Self::Delete(v) => v,
            Self::Keep(v) => v,
        }
    }
    pub fn to_songbird(&self) -> Input {
        match self {
            Self::Delete(v) => v.to_songbird(),
            Self::Keep(v) => v.to_songbird(),
        }
    }
}
impl Handler {
    pub fn new(call: Arc<Mutex<Call>>) -> Self {
        Self {
            call,
            queue: Vec::new(),
            prepared_next: None,
            current_handle: None,
            channel_names: HashMap::new(),
            waiting_on: None,
            last_channel_name: String::new(),
        }
    }
    pub async fn update(&mut self, messages: Vec<RawMessage>) -> Result<()> {
        self.queue.extend(messages);
        self.queue.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        self.shift().await?;
        Ok(())
    }
    pub async fn shift(&mut self) -> Result<()> {
        self.check_current_handle().await?;
        self.check_next_tts().await?;
        self.prepare_next_tts().await?;
        Ok(())
    }
    pub async fn check_current_handle(&mut self) -> Result<()> {
        if let Some((handle, v)) = &self.current_handle {
            match tokio::time::timeout(tokio::time::Duration::from_secs(1), handle.get_info()).await
            {
                Ok(Ok(info)) => {
                    if !info.playing.is_done() {
                        return Ok(());
                    }
                }
                Ok(Err(e)) => match e {
                    songbird::error::ControlError::Finished => {}
                    e => {
                        return Err(e.into());
                    }
                },
                Err(_) => {}
            }
            let _ = handle.stop();
            v.delete()?;
        }
        self.current_handle = None;
        Ok(())
    }
    pub async fn check_next_tts(&mut self) -> Result<()> {
        if let Some(v) = &self.prepared_next {
            if self.current_handle.is_none() {
                let handle = {
                    let mut call = self.call.lock().await;
                    call.play_input(v.to_songbird())
                };
                let _ = handle.set_volume(2.0);
                self.current_handle = Some((handle, v.clone()));
                self.prepared_next = None;
            }
        }
        Ok(())
    }
    pub async fn prepare_next_tts(&mut self) -> Result<()> {
        if self.prepared_next.is_some() {
            return Ok(());
        }
        let mut push = None;
        if let Some(m) = self.queue.first() {
            if let Some(ref mn) = &m.channel_name {
                if mn != &self.last_channel_name {
                    self.last_channel_name.clone_from(mn);
                    self.waiting_on = Some(mn.clone());
                    let content = format!("in #{}", mn);
                    push = Some(RawMessage {
                        author_id: String::new(),

                        channel_id: m.channel_id,
                        channel_name: Some(mn.clone()),

                        timestamp: m.timestamp,
                        tts_audio_handle: match self.channel_names.get(mn) {
                            Some(v) => {
                                let v = v.clone().get_video().clone();
                                Some(tokio::task::spawn(async move { Ok(v) }))
                            }
                            None => {
                                match RawMessage::audio_handle(content, TTSVoice::default()).await {
                                    Ok(Ok(v)) => {
                                        self.channel_names
                                            .insert(mn.clone(), Deleteable::Keep(v.clone()));
                                        Some(tokio::task::spawn(async move { Ok(v) }))
                                    }
                                    _ => None,
                                }
                            }
                        },
                    });
                }
            }
        }
        if let Some(m) = push {
            self.queue.insert(0, m);
        }
        if let Some(m) = self.queue.get_mut(0) {
            let deleteable = !m.author_id.is_empty();
            let v = match m.check_tts().await? {
                Some(Ok(v)) => Some(v),
                Some(Err(e)) => {
                    log::error!("Error generating audio: {:?}", e);
                    None
                }
                None => {
                    return Ok(());
                }
            };
            if let Some(v) = v {
                self.prepared_next = if deleteable {
                    Some(Deleteable::Delete(v))
                } else {
                    Some(Deleteable::Keep(v))
                };
            }
            self.queue.remove(0);
        }
        Ok(())
    }
    pub async fn stop(&mut self) {
        if let Some((handle, v)) = &self.current_handle {
            if let Err(e) = handle.stop() {
                log::error!("Error stopping audio: {:?}", e);
            }
            if let Err(e) = v.delete() {
                log::error!("Error deleting video: {:?}", e);
            }
        }
        self.current_handle = None;
        if let Some(v) = self.prepared_next.take() {
            if let Err(e) = v.delete() {
                log::error!("Error deleting video: {:?}", e);
            }
        }
        self.prepared_next = None;
        for m in self.queue.iter_mut() {
            let h = m.tts_audio_handle.take();
            if let Some(h) = h {
                match h.await {
                    Ok(Ok(v)) => {
                        if let Err(e) = v.delete() {
                            log::error!("Error deleting video: {:?}", e);
                        }
                    }
                    Ok(Err(e)) => {
                        log::error!("Error getting audio handle: {:?}", e);
                    }
                    Err(e) => {
                        log::error!("Error getting audio handle: {:?}", e);
                    }
                }
            }
        }
        for v in self.channel_names.values() {
            if let Err(e) = v.force_delete() {
                log::error!("Error deleting video: {:?}", e);
            }
        }
        self.queue.clear();
    }
}
pub struct TranscribeData;
impl TypeMapKey for TranscribeData {
    type Value = Arh<GuildId, Arc<RwLock<TranscribeChannelHandler>>>;
}
type Arh<K, V> = Arc<RwLock<HashMap<K, V>>>;
pub struct TranscribeChannelHandler {
    channels: Arh<ChannelId, mpsc::Sender<RawMessage>>,
    sender: mpsc::Sender<RawMessage>,
    receiver: Option<mpsc::Receiver<RawMessage>>,
    assigned_voice: Arh<String, crate::youtube::TTSVoice>,
    voice_cycle: Vec<crate::youtube::TTSVoice>,
}
impl TranscribeChannelHandler {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel::<RawMessage>(16);
        let mut v = crate::youtube::VOICES.clone();
        v.shuffle(&mut rand::thread_rng());
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            sender,
            receiver: Some(receiver),
            assigned_voice: Arc::new(RwLock::new(HashMap::new())),
            voice_cycle: v,
        }
    }
    pub fn lock(&mut self) -> Result<mpsc::Receiver<RawMessage>> {
        self.receiver
            .take()
            .ok_or_else(|| anyhow::anyhow!("Receiver already taken"))
    }
    pub async fn unlock(&mut self, receiver: mpsc::Receiver<RawMessage>) -> Result<()> {
        self.receiver = Some(receiver);
        self.channels.write().await.clear();
        self.assigned_voice.write().await.clear();
        self.voice_cycle.shuffle(&mut rand::thread_rng());
        Ok(())
    }
    pub async fn register(&mut self, channel: ChannelId) -> Result<()> {
        let tx = self.sender.clone();
        let mut channels = self.channels.write().await;
        channels.insert(channel, tx);
        Ok(())
    }
    pub async fn unregister(&mut self, channel: ChannelId) -> Result<()> {
        let mut channels = self.channels.write().await;
        channels.remove(&channel);
        Ok(())
    }
    pub async fn toggle(&mut self, channel: ChannelId) -> Result<()> {
        let mut channels = self.channels.write().await;
        if let std::collections::hash_map::Entry::Vacant(e) = channels.entry(channel) {
            let tx = self.sender.clone();
            e.insert(tx);
        } else {
            channels.remove(&channel);
        }
        Ok(())
    }
    pub async fn send(&mut self, msg: RawMessage) -> Result<(), RawMessage> {
        let mut channels = self.channels.write().await;
        let tx = match channels.get_mut(&msg.channel_id) {
            Some(tx) => tx,
            None => return Err(msg),
        };
        match tx.try_send(msg) {
            Ok(_) => Ok(()),
            Err(e) => Err(match e {
                mpsc::error::TrySendError::Full(m) => m,
                mpsc::error::TrySendError::Closed(m) => m,
            }),
        }
    }
    pub async fn get_tts(&mut self, ctx: &Context, msg: &Message) -> Vec<RawMessage> {
        let mut messages = Vec::new();
        let voice = {
            let mut assigned_voice = self.assigned_voice.write().await;
            match assigned_voice.get(&msg.author.name) {
                Some(v) => *v,
                None => {
                    let v = self.voice_cycle.remove(0);
                    assigned_voice.insert(msg.author.name.clone(), v);
                    self.voice_cycle.push(v);
                    messages.push(RawMessage::announcement(
                        msg,
                        format!("{} is now using this voice to speak", msg.author.name),
                        &v,
                    ));
                    v
                }
            }
        };
        if let Ok(b) = RawMessage::message(ctx, msg, &voice).await {
            messages.push(b);
        }
        messages
    }
    pub async fn send_tts(&mut self, ctx: &Context, msg: &Message) {
        let undo_voice = {
            self.assigned_voice
                .read()
                .await
                .get(&msg.author.name)
                .is_none()
        };
        let messages = self.get_tts(ctx, msg).await;
        let mut errored = false;
        for raw in messages {
            if let Err(ugh) = self.send(raw).await {
                if let Some(ughh) = ugh.tts_audio_handle {
                    ughh.abort();
                }
                errored = true;
            }
        }
        if errored && undo_voice {
            self.assigned_voice.write().await.remove(&msg.author.name);
        }
    }
}
pub struct TranscriptionThread {
    pub thread: tokio::task::JoinHandle<()>,
    pub message: mpsc::UnboundedSender<TranscriptionMessage>,
    pub receiver: mpsc::UnboundedReceiver<(PostSomething, UserId)>,
}
impl TranscriptionThread {
    pub async fn new(
        call: Arc<Mutex<Call>>,
        http: Arc<http::Http>,
        otx: mpsc::UnboundedSender<(oneshot::Sender<String>, AudioPromiseCommand)>,
    ) -> Self {
        let (message, messagerx) = mpsc::unbounded_channel();
        let (tx, receiver) = mpsc::unbounded_channel::<(PostSomething, UserId)>();
        // let transcribe =
        //     crate::voice_events::VoiceDataManager::new(Arc::clone(&call), http, otx).await;
        // let thread = tokio::task::spawn(async move {
        //     crate::voice_events::transcription_thread(transcribe, messagerx, tx, call).await
        // });
        let thread = tokio::task::spawn(crate::voice_events::transcription_thread(
            call, http, otx, messagerx, tx,
        ));
        Self {
            thread,
            message,
            receiver,
        }
    }
    // pub async fn consent(
    //     &self,
    //     user_id: UserId,
    //     consent: bool,
    //     ret: oneshot::Sender<String>,
    // ) -> Result<()> {
    //     if let Err(e) = self.message.send(TranscriptionMessage::Consent {
    //         user_id,
    //         consent,
    //         ret,
    //     }) {
    //         if let TranscriptionMessage::Consent { ret, consent, .. } = e.0 {
    //             ret.send(format!("The database was successfully updated, however we were unable to update the consent state for the voice reader.\nNeon circle may {} be able to process your audio data for this session.", if consent { "not" } else { "still" })).ok();
    //         }
    //         Err(anyhow::anyhow!("Failed to send consent message"))
    //     } else {
    //         log::trace!("Sent consent message");
    //         Ok(())
    //     }
    // }
    pub async fn stop(self) -> Result<()> {
        self.message.send(TranscriptionMessage::Stop)?;
        // await the thread to finish with a timeout of 5 seconds
        match tokio::time::timeout(tokio::time::Duration::from_secs(5), self.thread).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err(anyhow::anyhow!("Timeout")),
        }
    }
}
#[derive(Debug)]
pub enum TranscriptionMessage {
    Stop,
    // Consent {
    //     user_id: UserId,
    //     consent: bool,
    //     ret: oneshot::Sender<String>,
    // },
}
