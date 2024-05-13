use rand::seq::SliceRandom;

use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;
use serenity::model::prelude::{ChannelId, GuildId, Message, UserId};
use songbird::tracks::TrackHandle;
use songbird::{ffmpeg, Call};
use tokio::sync::Mutex;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Error;
use serenity::builder::CreateApplicationCommand;
use serenity::futures::channel::mpsc;
use serenity::futures::channel::mpsc::{Receiver, Sender};
use serenity::model::application::interaction::InteractionResponseType;
use serenity::model::prelude::command::CommandOptionType;

use serenity::prelude::{Context, TypeMapKey};

use crate::video::Video;
use crate::youtube::TTSVoice;

use super::RawMessage;

#[derive(Debug, Clone)]
pub struct Transcribe;

#[serenity::async_trait]
impl crate::CommandTrait for Transcribe {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command.name(self.name()).description("Transcribe this channel").create_option(|option| option.name("transcribe").description("Transcribe the active voice channel").kind(CommandOptionType::Boolean).required(true));
    }
    async fn run(&self, ctx: &Context, interaction: &serenity::model::prelude::application_command::ApplicationCommandInteraction) {
        if let Err(e) = interaction.create_interaction_response(&ctx.http, |response| response.interaction_response_data(|f| f.ephemeral(true)).kind(InteractionResponseType::DeferredChannelMessageWithSource)).await {
            eprintln!("Failed to create interaction response: {:?}", e);
        };
        let guild_id = match interaction.guild_id {
            Some(id) => id,
            None => {
                if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command can only be used in a server")).await {
                    eprintln!("Failed to edit original interaction response: {:?}", e);
                }
                return;
            }
        };

        let option = match interaction.data.options.iter().find(|o| o.name == "transcribe") {
            Some(o) => match o.value.as_ref() {
                Some(v) => {
                    if let Some(v) = v.as_bool() {
                        v
                    } else {
                        if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command requires an option")).await {
                            eprintln!("Failed to edit original interaction response: {:?}", e);
                        }
                        return;
                    }
                }
                None => {
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command requires an option")).await {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
            },
            None => {
                if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command requires an option")).await {
                    eprintln!("Failed to edit original interaction response: {:?}", e);
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
                let mut v = v.lock().await;
                v.mutual_channel(ctx, &guild_id, &member.user.id)
            };

            match next_step {
                super::VoiceAction::UserNotConnected => {
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("You're not in a voice channel")).await {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::InDifferent(_channel) => {
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("I'm in a different voice channel")).await {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::Join(_channel) => {
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("I'm not in a channel, if you want me to join use /join or /play")).await {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::InSame(_channel) => {
                    let em = match ctx.data.read().await.get::<TranscribeData>().expect("Expected TranscribeData in TypeMap.").lock().await.entry(guild_id) {
                        std::collections::hash_map::Entry::Occupied(ref mut e) => e.get_mut(),
                        std::collections::hash_map::Entry::Vacant(e) => e.insert(Arc::new(Mutex::new(TranscribeChannelHandler::new()))),
                    }
                    .clone();

                    let mut e = em.lock().await;

                    if option {
                        if let Err(res) = e.register(interaction.channel_id).await {
                            if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content(format!("Error registering: {:?}", res))).await {
                                eprintln!("Failed to edit original interaction response: {:?}", e);
                            }
                        } else if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("Registered")).await {
                            eprintln!("Failed to edit original interaction response: {:?}", e);
                        }
                    } else if let Err(res) = e.unregister(interaction.channel_id).await {
                        if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content(format!("Error unregistering: {:?}", res))).await {
                            eprintln!("Failed to edit original interaction response: {:?}", e);
                        }
                    } else if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("Unregistered")).await {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                }
            }
        } else if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("TELL ETHAN THIS SHOULD NEVER HAPPEN :(")).await {
            eprintln!("Failed to edit original interaction response: {:?}", e);
        }
    }
    fn name(&self) -> &str {
        "transcribe"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &AutocompleteInteraction) -> Result<(), Error> {
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
    pub fn delete(&self) -> Result<(), Error> {
        match self {
            Self::Delete(v) => v.delete(),
            Self::Keep(_) => Ok(()),
        }
    }
    pub fn force_delete(&self) -> Result<(), Error> {
        self.get_video().delete()
    }
    pub fn get_video(&self) -> &Video {
        match self {
            Self::Delete(v) => v,
            Self::Keep(v) => v,
        }
    }
}

impl Handler {
    pub fn new(call: Arc<Mutex<Call>>) -> Self {
        Self { call, queue: Vec::new(), prepared_next: None, current_handle: None, channel_names: HashMap::new(), waiting_on: None, last_channel_name: String::new() }
    }
    pub async fn update(&mut self, messages: Vec<RawMessage>) -> Result<(), Error> {
        self.queue.extend(messages);
        // sort by timestamp, ensuring the first element is always the oldest message
        self.queue.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        self.shift().await?;

        Ok(())
    }

    pub async fn shift(&mut self) -> Result<(), Error> {
        self.check_current_handle().await?;
        self.check_next_tts().await?;
        self.prepare_next_tts().await?;
        Ok(())
    }

    pub async fn check_current_handle(&mut self) -> Result<(), Error> {
        if let Some((handle, v)) = &self.current_handle {
            match tokio::time::timeout(tokio::time::Duration::from_secs(1), handle.get_info()).await {
                Ok(Ok(info)) => {
                    if !info.playing.is_done() {
                        return Ok(());
                    }
                }
                Ok(Err(e)) => {
                    match e {
                        songbird::error::TrackError::Finished => {
                            // track is done, we can stop and delete :D
                        }
                        e => {
                            return Err(e.into());
                        }
                    }
                }
                Err(_) => {
                    // timeout
                }
            }
            let _ = handle.stop();
            v.delete()?;
        }
        // println!("Current handle is done");
        self.current_handle = None;
        Ok(())
    }

    pub async fn check_next_tts(&mut self) -> Result<(), Error> {
        if let Some(v) = &self.prepared_next {
            if self.current_handle.is_none() {
                let mut call = self.call.lock().await;
                let handle = call.play_source(ffmpeg(v.get_video().path.clone()).await.expect("Error creating ffmpeg source"));
                let _ = handle.set_volume(1.5);
                self.current_handle = Some((handle, v.clone()));
                // println!("Prepared next is done");
                self.prepared_next = None;
            }
        }
        Ok(())
    }

    pub async fn prepare_next_tts(&mut self) -> Result<(), Error> {
        if self.prepared_next.is_some() {
            return Ok(());
        }
        let mut push = None;
        if let Some(m) = self.queue.first() {
            if let Some(ref mn) = &m.channel_name {
                if mn != &self.last_channel_name {
                    self.last_channel_name.clone_from(mn);
                    // println!("Waiting on {:?}", mn);
                    self.waiting_on = Some(mn.clone());
                    // we want to make the next item in the queue a tts for channel change announcement
                    let content = format!("in #{}", mn);
                    push = Some(RawMessage {
                        author_id: String::new(),
                        // author: String::new(),
                        channel_id: m.channel_id,
                        channel_name: Some(mn.clone()),
                        // content: content.clone(),
                        timestamp: m.timestamp,
                        tts_audio_handle: match self.channel_names.get(mn) {
                            Some(v) => {
                                let v = v.clone().get_video().clone();
                                Some(tokio::task::spawn(async move { Ok(v) }))
                            }
                            None => match RawMessage::audio_handle(content, TTSVoice::default()).await {
                                Ok(Ok(v)) => {
                                    self.channel_names.insert(mn.clone(), Deleteable::Keep(v.clone()));
                                    Some(tokio::task::spawn(async move { Ok(v) }))
                                }
                                _ => None,
                            },
                        },
                    });
                }
            }
        }
        // we need push to be the next element in the queue
        match push {
            Some(m) => {
                self.queue.insert(0, m);
            }
            None => {
                // println!("No tts needed");
            }
        }

        if let Some(m) = self.queue.get_mut(0) {
            let deleteable = !m.author_id.is_empty();

            let v = match m.check_tts().await? {
                Some(Ok(v)) => Some(v),
                Some(Err(e)) => {
                    // audio failed to generate, skip
                    println!("Error generating audio: {:?}", e);
                    None
                }
                None => {
                    // println!("tts not prepared yet");
                    return Ok(());
                }
            };
            if let Some(v) = v {
                self.prepared_next = if deleteable { Some(Deleteable::Delete(v)) } else { Some(Deleteable::Keep(v)) };
            }
            // println!("Next tts is prepared");
            self.queue.remove(0);
        }
        Ok(())
    }

    pub async fn stop(&mut self) {
        // prepare for shutdown, close all handles and delete all files
        if let Some((handle, v)) = &self.current_handle {
            if let Err(e) = handle.stop() {
                println!("Error stopping track: {:?}", e);
            }
            if let Err(e) = v.delete() {
                println!("Error deleting video: {:?}", e);
            }
        }
        self.current_handle = None;
        if let Some(v) = self.prepared_next.take() {
            if let Err(e) = v.delete() {
                println!("Error deleting video: {:?}", e);
            }
        }
        self.prepared_next = None;
        for m in self.queue.iter_mut() {
            let h = m.tts_audio_handle.take();
            if let Some(h) = h {
                match h.await {
                    Ok(Ok(v)) => {
                        if let Err(e) = v.delete() {
                            println!("Error deleting video: {:?}", e);
                        }
                    }
                    Ok(Err(e)) => {
                        println!("Error getting audio handle: {:?}", e);
                    }
                    Err(e) => {
                        println!("Error getting audio handle: {:?}", e);
                    }
                }
            }
        }
        for v in self.channel_names.values() {
            if let Err(e) = v.force_delete() {
                println!("Error deleting video: {:?}", e);
            }
        }
        self.queue.clear();
    }
}

pub struct TranscribeData;

impl TypeMapKey for TranscribeData {
    type Value = Amh<GuildId, Arc<Mutex<TranscribeChannelHandler>>>;
}

type Amh<K, V> = Arc<Mutex<HashMap<K, V>>>;

pub struct TranscribeChannelHandler {
    channels: Amh<ChannelId, Sender<RawMessage>>,
    sender: Sender<RawMessage>,
    receiver: Option<Receiver<RawMessage>>,
    assigned_voice: Amh<UserId, crate::youtube::TTSVoice>,
    voice_cycle: Vec<crate::youtube::TTSVoice>,
}

impl TranscribeChannelHandler {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel::<RawMessage>(16);
        let mut v = crate::youtube::VOICES.clone();
        v.shuffle(&mut rand::thread_rng());
        Self { channels: Arc::new(Mutex::new(HashMap::new())), sender, receiver: Some(receiver), assigned_voice: Arc::new(Mutex::new(HashMap::new())), voice_cycle: v }
    }
    // receiver side
    pub fn lock(&mut self) -> Result<Receiver<RawMessage>, Error> {
        self.receiver.take().ok_or_else(|| anyhow::anyhow!("Receiver already taken"))
    }
    pub async fn unlock(&mut self, receiver: Receiver<RawMessage>) -> Result<(), Error> {
        self.receiver = Some(receiver);
        self.channels.lock().await.clear();
        self.assigned_voice.lock().await.clear();
        self.voice_cycle.shuffle(&mut rand::thread_rng());
        Ok(())
    }
    // sender side
    pub async fn register(&mut self, channel: ChannelId) -> Result<(), Error> {
        let tx = self.sender.clone();
        let mut channels = self.channels.lock().await;
        channels.insert(channel, tx);
        Ok(())
    }
    pub async fn unregister(&mut self, channel: ChannelId) -> Result<(), Error> {
        let mut channels = self.channels.lock().await;
        channels.remove(&channel);
        Ok(())
    }
    // when message is sent, attempt to find a sender, if not found, return error
    pub async fn send(&mut self, msg: RawMessage) -> Result<(), RawMessage> {
        let mut channels = self.channels.lock().await;
        let tx = match channels.get_mut(&msg.channel_id) {
            Some(tx) => tx,
            None => return Err(msg),
        };
        match tx.try_send(msg) {
            Ok(_) => Ok(()),
            Err(e) => Err(e.into_inner()),
        }
    }
    pub async fn get_tts(&mut self, ctx: &Context, msg: &Message) -> Vec<RawMessage> {
        let mut messages = Vec::new();

        // attempt to get voice
        let voice = {
            let mut assigned_voice = self.assigned_voice.lock().await;
            match assigned_voice.get(&msg.author.id) {
                Some(v) => *v,
                None => {
                    let v = self.voice_cycle.remove(0);
                    assigned_voice.insert(msg.author.id, v);
                    self.voice_cycle.push(v);
                    messages.push(RawMessage::announcement(msg, format!("{} is now using this voice to speak", msg.author.name), &v));
                    v
                }
            }
        };

        match RawMessage::message(ctx, msg, &voice).await {
            Ok(b) => {
                messages.push(b);
            }
            Err(_) => {
                // dont actually process message
            }
        }

        messages
    }
    pub async fn send_tts(&mut self, ctx: &Context, msg: &Message) {
        let undo_voice = { self.assigned_voice.lock().await.get(&msg.author.id).is_none() };
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
            self.assigned_voice.lock().await.remove(&msg.author.id);
        }
    }
}
