pub mod add;
pub mod autoplay;
pub mod join;
pub mod loop_queue;
pub mod mainloop;
pub mod pause;
pub mod remove;
pub mod repeat;
pub mod resume;
pub mod setbitrate;
pub mod settingsdata;
pub mod shuffle;
pub mod skip;
pub mod stop;
pub mod transcribe;
pub mod volume;
use self::mainloop::EmbedData;
use self::settingsdata::SettingsData;
use crate::video::Video;
use crate::youtube::{TTSVoice, VideoInfo};
use anyhow::Error;
use serde_json::json;
use serenity::all::*;
use songbird::typemap::TypeMapKey;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::JoinHandle;
use tokio::time::Instant;
pub struct AudioHandler;
impl TypeMapKey for AudioHandler {
    type Value = Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>;
}
pub struct AudioCommandHandler;
impl TypeMapKey for AudioCommandHandler {
    type Value = Arc<
        Mutex<
            HashMap<String, mpsc::UnboundedSender<(oneshot::Sender<String>, AudioPromiseCommand)>>,
        >,
    >;
}
pub struct VoiceData;
impl TypeMapKey for VoiceData {
    type Value = Arc<Mutex<InnerVoiceData>>;
}
pub struct InnerVoiceData {
    pub guilds: HashMap<GuildId, GuildVc>,
    pub bot_id: UserId,
}
impl InnerVoiceData {
    pub fn new(bot_id: UserId) -> Self {
        Self {
            guilds: HashMap::new(),
            bot_id,
        }
    }
    pub fn update(&mut self, old: Option<VoiceState>, new: VoiceState) {
        if let Some(guild_id) = new.guild_id {
            let guild = self.guilds.entry(guild_id).or_insert_with(GuildVc::new);
            guild.update(old, new.clone());
            if self.bot_id == new.user_id {
                guild.bot_connected = new.channel_id.is_some();
            }
        }
    }
    pub fn mutual_channel(
        &mut self,
        ctx: &Context,
        guild: &GuildId,
        member: &UserId,
    ) -> VoiceAction {
        let bot_id = ctx.cache.current_user().id;
        if bot_id != self.bot_id {
            self.bot_id = bot_id;
        }
        let guildstate = self.guilds.entry(*guild).or_insert_with(GuildVc::new);
        let botstate = guildstate.find_user(bot_id);
        let memberstate = guildstate.find_user(*member);
        match (botstate, memberstate) {
            (Some(botstate), Some(memberstate)) => {
                if botstate == memberstate {
                    VoiceAction::InSame(botstate)
                } else {
                    VoiceAction::InDifferent(botstate)
                }
            }
            (None, Some(memberstate)) => VoiceAction::Join(memberstate),
            _ => VoiceAction::UserNotConnected,
        }
    }
    pub async fn refresh_guild(&mut self, ctx: &Context, guild_id: GuildId) -> Result<(), Error> {
        let mut new = GuildVc::new();
        for (_i, channel) in ctx
            .http
            .get_guild(guild_id)
            .await?
            .channels(&ctx.http)
            .await?
        {
            if channel.kind == ChannelType::Voice {
                let mut newchannel = HashMap::new();
                for member in match ctx.http.get_channel(channel.id).await {
                    Ok(Channel::Guild(channel)) => channel,
                    Err(_) => {
                        continue;
                    }
                    _ => return Err(anyhow::anyhow!("Expected Guild channel")),
                }
                .members(&ctx.cache)?
                {
                    newchannel.insert(
                        member.user.id,
                        UserMetadata {
                            member: member.clone(),
                        },
                    );
                }
                new.replace_channel(channel.id, newchannel);
            }
        }
        self.guilds.insert(guild_id, new);
        Ok(())
    }
    pub fn bot_alone(&mut self, guild: &GuildId) -> bool {
        let guild = self.guilds.entry(*guild).or_insert_with(GuildVc::new);
        let channel = match guild.find_user(self.bot_id) {
            Some(channel) => channel,
            None => return false,
        };
        let channel = guild.channels.entry(channel).or_default();
        for user in channel.values() {
            if !user.member.user.bot {
                return false;
            }
        }
        true
    }
}
pub enum VoiceAction {
    Join(ChannelId),
    InSame(ChannelId),
    InDifferent(ChannelId),
    UserNotConnected,
}
impl VoiceAction {
    pub async fn send_command_or_respond(
        self,
        ctx: &Context,
        interaction: &CommandInteraction,
        guild_id: GuildId,
        command: AudioPromiseCommand,
    ) {
        match self {
            Self::UserNotConnected => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content("You're not in a voice channel"),
                    )
                    .await
                {
                    eprintln!("Failed to edit original interaction response: {:?}", e);
                }
            }
            Self::InDifferent(_channel) => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content("I'm in a different voice channel"),
                    )
                    .await
                {
                    eprintln!("Failed to edit original interaction response: {:?}", e);
                }
            }
            Self::Join(_channel) => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content(
                            "I'm not in a channel, if you want me to join use /join or /play",
                        ),
                    )
                    .await
                {
                    eprintln!("Failed to edit original interaction response: {:?}", e);
                }
            }
            Self::InSame(_channel) => {
                let audio_command_handler = ctx
                    .data
                    .read()
                    .await
                    .get::<AudioCommandHandler>()
                    .expect("Expected AudioCommandHandler in TypeMap")
                    .clone();
                let mut audio_command_handler = audio_command_handler.lock().await;
                if let Some(tx) = audio_command_handler.get_mut(&guild_id.to_string()) {
                    let (rtx, rrx) = oneshot::channel::<String>();
                    if tx.send((rtx, command)).is_err() {
                        if let Err(e) = interaction
                            .edit_response(
                                &ctx.http,
                                EditInteractionResponse::new()
                                    .content("Failed to send volume change"),
                            )
                            .await
                        {
                            eprintln!("Failed to edit original interaction response: {:?}", e);
                        }
                        return;
                    }
                    let timeout = tokio::time::timeout(Duration::from_secs(10), rrx).await;
                    if let Ok(Ok(msg)) = timeout {
                        if let Err(e) = interaction
                            .edit_response(&ctx.http, EditInteractionResponse::new().content(msg))
                            .await
                        {
                            eprintln!("Failed to edit original interaction response: {:?}", e);
                        }
                    } else if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content("Failed to send inner command"),
                        )
                        .await
                    {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                } else if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new()
                            .content("Couldnt find the channel handler :( im broken."),
                    )
                    .await
                {
                    eprintln!("Failed to edit original interaction response: {:?}", e);
                }
            }
        }
    }
}
#[derive(Debug, Clone)]
pub struct GuildVc {
    pub channels: HashMap<ChannelId, HashMap<UserId, UserMetadata>>,
    pub bot_connected: bool,
}
impl GuildVc {
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
            bot_connected: false,
        }
    }
    pub fn update(&mut self, old: Option<VoiceState>, new: VoiceState) {
        if let Some(old) = old {
            if old.channel_id != new.channel_id {
                if let Some(channel) = old.channel_id {
                    let channel = self.channels.entry(channel).or_default();
                    channel.remove(&old.user_id);
                }
            }
        }
        if let (Some(channel), Some(member)) = (new.channel_id, new.member.clone()) {
            let channel = self.channels.entry(channel).or_default();
            channel.insert(new.user_id, UserMetadata::new(member, new));
        }
    }
    pub fn replace_channel(&mut self, id: ChannelId, members: HashMap<UserId, UserMetadata>) {
        self.channels.insert(id, members);
    }
    pub fn find_user(&self, user: UserId) -> Option<ChannelId> {
        for (channel, members) in self.channels.iter() {
            if members.contains_key(&user) {
                return Some(*channel);
            }
        }
        None
    }
}
#[derive(Debug, Clone)]
pub struct UserMetadata {
    pub member: Member,
}
impl UserMetadata {
    pub fn new(member: Member, _state: VoiceState) -> Self {
        Self { member }
    }
}
#[derive(Debug, Clone)]
pub enum OrToggle {
    Specific(bool),
    Toggle,
}
impl OrToggle {
    pub fn get_val(&self, current: bool) -> bool {
        match self {
            OrToggle::Specific(b) => *b,
            OrToggle::Toggle => !current,
        }
    }
}
#[derive(Debug, Clone)]
pub enum AudioPromiseCommand {
    Play(Vec<MetaVideo>),

    Paused(OrToggle),

    Stop(Option<tokio::time::Duration>),
    Loop(OrToggle),
    Repeat(OrToggle),
    Shuffle(OrToggle),
    Autoplay(OrToggle),
    ReadTitles(OrToggle),

    Volume(f64),
    SpecificVolume(SpecificVolume),
    SetBitrate(OrAuto),

    Skip,
    Remove(usize),

    RetrieveLog(mpsc::Sender<Vec<String>>),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrAuto {
    Specific(i64),
    Auto,
}
impl Display for OrAuto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrAuto::Specific(i) => write!(f, "{}", i),
            OrAuto::Auto => write!(f, "Auto"),
        }
    }
}
#[derive(Debug, Clone)]
pub enum SpecificVolume {
    Volume(f64),
    RadioVolume(f64),
}
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum VideoType {
    Disk(Video),
    Url(VideoInfo),
}
impl VideoType {
    pub fn to_songbird(&self) -> songbird::input::Input {
        match self {
            VideoType::Disk(v) => v.to_songbird(),
            VideoType::Url(v) => v.to_songbird(),
        }
    }
    pub fn get_duration(&self) -> Option<u64> {
        match self {
            VideoType::Disk(v) => Some(v.duration.floor() as u64),
            VideoType::Url(v) => v.duration,
        }
    }
    #[allow(dead_code)]
    pub fn get_title(&self) -> String {
        match self {
            VideoType::Disk(v) => v.title.clone(),
            VideoType::Url(v) => v.title.clone(),
        }
    }
    pub async fn delete(&self) -> Result<(), Error> {
        if let VideoType::Disk(v) = self {
            v.delete()?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct LazyLoadedVideo {
    handle: Arc<Mutex<Option<JoinHandle<anyhow::Result<Video>>>>>,
    video: Arc<Mutex<Option<Video>>>,
}
impl LazyLoadedVideo {
    pub fn new(handle: JoinHandle<anyhow::Result<Video>>) -> Self {
        Self {
            handle: Arc::new(Mutex::new(Some(handle))),
            video: Arc::new(Mutex::new(None)),
        }
    }
    pub async fn check(&mut self) -> anyhow::Result<Option<Video>> {
        let mut lock = self.handle.lock().await;
        if let Some(handle) = lock.take() {
            if handle.is_finished() {
                let video = handle.await??;
                self.video.lock().await.replace(video.clone());
                Ok(Some(video))
            } else {
                lock.replace(handle);
                Ok(None)
            }
        } else {
            Err(anyhow::anyhow!("Handle is None"))
        }
    }
    pub async fn wait_for(&mut self) -> anyhow::Result<Video> {
        let mut lock = self.handle.lock().await;
        if let Some(handle) = lock.take() {
            let video = handle.await??;
            self.video.lock().await.replace(video.clone());
            Ok(video)
        } else {
            Err(anyhow::anyhow!("Handle is None"))
        }
    }
}
#[derive(Debug, Clone)]
pub struct Author {
    pub name: String,
    pub pfp_url: String,
}
impl Author {
    pub async fn from_user(ctx: &Context, user: &User, guild: Option<GuildId>) -> Option<Self> {
        let name = match guild {
            Some(g) => {
                let member = g.member(ctx, user.id).await.ok()?;
                member.display_name().to_string()
            }
            None => user.name.clone(),
        };
        let pfp_url = user
            .avatar_url()
            .unwrap_or_else(|| user.default_avatar_url());
        Some(Self { name, pfp_url })
    }
}
#[derive(Debug, Clone)]
pub struct MetaVideo {
    pub video: VideoType,
    pub title: String,
    pub author: Option<Author>,
    #[cfg(feature = "tts")]
    pub ttsmsg: Option<LazyLoadedVideo>,
}
impl MetaVideo {
    pub async fn delete(&mut self) -> Result<(), Error> {
        self.video.delete().await?;
        #[cfg(feature = "tts")]
        if let Some(ref mut ttsmsg) = self.ttsmsg {
            if let Ok(vid) = ttsmsg.wait_for().await {
                vid.delete()?;
            }
        };
        Ok(())
    }
}
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MessageReference {
    http: Arc<Http>,
    cache: Arc<Cache>,
    guild_id: GuildId,
    channel_id: ChannelId,
    message: Option<Message>,
    last_content: Option<EmbedData>,
    last_settings: Option<SettingsData>,
    last_edit: Instant,
    edit_delay: u128,
    first_time: bool,

    resend_next_time: bool,
    transcription_thread: OptionOrFailed<GuildChannel>,
}
#[derive(Debug, Clone)]
pub enum OptionOrFailed<T> {
    Some(T),
    None,
    Failed,
}
impl<T> OptionOrFailed<T> {
    pub fn is_failed(&self) -> bool {
        matches!(self, OptionOrFailed::Failed)
    }
    fn take(&mut self) -> Option<T> {
        let mut val = OptionOrFailed::None;
        std::mem::swap(self, &mut val);
        match val {
            OptionOrFailed::Some(t) => Some(t),
            _ => None,
        }
    }
}
#[allow(dead_code)]
impl MessageReference {
    fn new(
        http: Arc<Http>,
        cache: Arc<Cache>,
        guild_id: GuildId,
        channel_id: ChannelId,
        message: Message,
    ) -> Self {
        Self {
            http,
            cache,
            guild_id,
            channel_id,
            message: Some(message),
            last_edit: Instant::now(),
            last_content: None,
            last_settings: None,
            first_time: true,
            edit_delay: 10000,
            resend_next_time: false,
            transcription_thread: OptionOrFailed::None,
        }
    }
    async fn send_manually(&mut self, content: String, user: UserId) -> Result<(), Error> {
        if self.transcription_thread.is_failed() {
            return Ok(());
        }
        let webhook = match self.channel_id.webhooks(&self.http).await?.first() {
            Some(webhook) => webhook.clone(),
            None => {
                self.channel_id
                    .create_webhook(
                        &self.http,
                        CreateWebhook::new("Music Bot").audit_log_reason(
                            "Webhook for logging things said during a voice session",
                        ),
                    )
                    .await?
            }
        };
        let thread_id = match self.transcription_thread {
            OptionOrFailed::Some(ref thread) => thread.id,
            OptionOrFailed::Failed => {
                return Ok(());
            }
            OptionOrFailed::None => {
                let thread = self
                    .channel_id
                    .create_thread(
                        &self.http,
                        CreateThread::new(
                            chrono::Local::now()
                                .format("CLOSED CAPTIONS FOR %b %-d, %Y at %-I:%M%p")
                                .to_string(),
                        ),
                    )
                    .await;
                if thread.is_err() {
                    self.transcription_thread = OptionOrFailed::Failed;
                    return Ok(());
                }
                let thread = thread?;
                let id = thread.id;
                self.transcription_thread = OptionOrFailed::Some(thread);
                id
            }
        };
        let author = self.http.get_user(user).await?;
        let webhook_url = format!("{}?thread_id={}", webhook.url()?, thread_id);
        crate::WEB_CLIENT
            .post(&webhook_url)
            .json(&json!({
                "content": content,
                "username": author.name,
                "avatar_url": author.avatar_url().unwrap_or_else(|| author.default_avatar_url()),
                "allowed_mentions": {
                    "parse": []
                }
            }))
            .send()
            .await?;
        Ok(())
    }
    async fn update(&mut self, settings: SettingsData, content: EmbedData) -> Result<(), Error> {
        let message = match self.message.as_mut() {
            Some(message) => message,
            None => {
                self.send_new().await?;
                match self.message.as_mut() {
                    Some(message) => message,
                    None => {
                        return Err(anyhow::anyhow!("Message is None"));
                    }
                }
            }
        };
        let diff = match self.last_content {
            None => true,
            Some(ref last_content) => last_content != &content,
        };
        let forcediff = match self.last_settings {
            None => true,
            Some(ref last_settings) => last_settings != &settings,
        };
        let mut messages = match message.channel(&self.http).await? {
            Channel::Guild(channel) => {
                channel
                    .messages(&self.http, GetMessages::new().after(message.id).limit(1))
                    .await?
            }
            Channel::Private(channel) => {
                channel
                    .messages(&self.http, GetMessages::new().after(message.id).limit(1))
                    .await?
            }
            _ => Vec::new(),
        };
        messages.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        for rawmessage in messages {
            if rawmessage.id > message.id {
                self.resend_next_time = true;
            }
        }
        if (diff && ((self.last_edit.elapsed().as_millis() > self.edit_delay) || self.first_time))
            || forcediff
        {
            self.first_time = false;
            let write_content = content.clone();
            self.last_content = Some(content);
            self.last_settings = Some(settings);
            if self.resend_next_time {
                self.resend_next_time = false;
                if let Err(e) = self.delete().await {
                    println!("Error deleting message: {:?}", e);
                }
                self.send_new().await?;
            } else if let Err(e) = message
                .edit(&self.http, {
                    let mut m = EditMessage::new()
                        .embed(write_content.to_serenity())
                        .flags(MessageFlags::SUPPRESS_NOTIFICATIONS);
                    if let Some(ars) = self.last_settings.as_ref().map(Self::get_ars) {
                        m = m.components(ars);
                    }
                    m
                })
                .await
            {
                println!("Error editing message: {:?}", e);
                self.send_new().await?;
            };
            self.last_edit = Instant::now();
        }
        Ok(())
    }
    async fn send_new(&mut self) -> Result<(), Error> {
        match self.last_content {
            None => {
                let content = String::from("<a:earloading:979852072998543443>");
                let message = self
                    .channel_id
                    .send_message(&self.http, {
                        let mut m = CreateMessage::new()
                            .content(content)
                            .flags(MessageFlags::SUPPRESS_NOTIFICATIONS);
                        if let Some(ars) = self.last_settings.as_ref().map(Self::get_ars) {
                            m = m.components(ars);
                        }
                        m
                    })
                    .await?;
                self.message = Some(message);
                Ok(())
            }
            Some(ref content) => {
                let write_content = content.clone();
                let message = self
                    .channel_id
                    .send_message(&self.http, {
                        let mut m = CreateMessage::new()
                            .embed(write_content.to_serenity())
                            .flags(MessageFlags::SUPPRESS_NOTIFICATIONS);
                        if let Some(ars) = self.last_settings.as_ref().map(Self::get_ars) {
                            m = m.components(ars);
                        }
                        m
                    })
                    .await?;
                self.message = Some(message);
                Ok(())
            }
        }
    }
    async fn delete(&mut self) -> Result<(), Error> {
        if let Some(message) = self.message.take() {
            message.delete(&self.http).await?;
        };
        Ok(())
    }
    async fn final_cleanup(&mut self) -> Result<(), Error> {
        self.delete().await?;
        if let Some(thread) = self.transcription_thread.take() {
            if [ChannelType::PrivateThread, ChannelType::PublicThread].contains(&thread.kind) {
                thread.delete(&self.http).await?;
            }
        }
        Ok(())
    }
    #[cfg(not(feature = "new-controls"))]
    fn get_ars(settings: &SettingsData) -> Vec<CreateActionRow> {
        vec![
            CreateActionRow::Buttons(vec![
                CreateButton::new("volume")
                    .style(ButtonStyle::Primary)
                    .label(format!(
                        "{} {}%",
                        match settings.raw_volume() {
                            v if v == 0.0 => "🔇",

                            v if v <= 0.33 => "🔈",

                            v if v <= 0.66 => "🔉",

                            _ => "🔊",
                        },
                        settings.raw_volume() * 100.0
                    )),
                CreateButton::new("radiovolume")
                    .style(ButtonStyle::Secondary)
                    .label(format!("📻 {}%", settings.raw_radiovolume() * 100.0)),
                CreateButton::new("bitrate")
                    .style(ButtonStyle::Secondary)
                    .label(match settings.bitrate {
                        OrAuto::Specific(i) => {
                            if i >= 1000 {
                                format!("{}kbps", i / 1000)
                            } else {
                                format!("{}bps", i)
                            }
                        }
                        OrAuto::Auto => "Auto".to_owned(),
                    }),
                CreateButton::new("log")
                    .style(if settings.log_empty {
                        ButtonStyle::Secondary
                    } else {
                        ButtonStyle::Danger
                    })
                    .label("📜")
                    .disabled(settings.log_empty),
            ]),
            CreateActionRow::Buttons(vec![
                CreateButton::new("pause")
                    .style(if settings.pause {
                        ButtonStyle::Success
                    } else {
                        ButtonStyle::Danger
                    })
                    .label(if settings.pause { "▶️" } else { "⏸️" }),
                CreateButton::new("skip")
                    .style(ButtonStyle::Primary)
                    .label("⏭️"),
                CreateButton::new("stop")
                    .style(ButtonStyle::Danger)
                    .label("⏹️"),
            ]),
            CreateActionRow::Buttons(vec![
                CreateButton::new("looped")
                    .style(if settings.looped {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    })
                    .label("🔁"),
                CreateButton::new("shuffle")
                    .style(if settings.shuffle {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    })
                    .label("🔀"),
                CreateButton::new("repeat")
                    .style(if settings.repeat {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    })
                    .label("🔄️"),
            ]),
            CreateActionRow::Buttons(vec![
                CreateButton::new("autoplay")
                    .style(if settings.autoplay {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    })
                    .label("🎲"),
                CreateButton::new("remove")
                    .style(ButtonStyle::Danger)
                    .label("🗑️"),
                CreateButton::new("read_titles")
                    .style(if settings.read_titles {
                        ButtonStyle::Success
                    } else {
                        ButtonStyle::Danger
                    })
                    .label("🗣️"),
            ]),
        ]
    }
    #[cfg(feature = "new-controls")]
    fn get_ars(settings: &SettingsData) -> Vec<CreateActionRow> {
        let mut options = vec![
            CreateSelectMenuOption::new("Bot Controls", "controls")
                .description("🎛️")
                .default_selection(true),
            CreateSelectMenuOption::new("Volume", "volume").description(format!(
                "{} {:.0}%",
                match settings.raw_volume() {
                    v if v == 0.0 => "🔇",

                    v if v <= 0.33 => "🔈",

                    v if v <= 0.66 => "🔉",

                    _ => "🔊",
                },
                settings.raw_volume() * 100.0
            )),
            CreateSelectMenuOption::new("Radio Volume", "radiovolume")
                .description(format!("📻 {:.0}%", settings.raw_radiovolume() * 100.0)),
            CreateSelectMenuOption::new(
                if settings.something_playing {
                    "Playing"
                } else {
                    "Paused"
                },
                "pause",
            )
            .description(if settings.pause { "▶️" } else { "⏸️" }),
            CreateSelectMenuOption::new("Skip", "skip").description("⏭️"),
            CreateSelectMenuOption::new("Stop", "stop").description("⏹️"),
            CreateSelectMenuOption::new(
                if settings.looped {
                    "Queue Looped"
                } else {
                    "Queue Not Looped"
                },
                "looped",
            )
            .description(if settings.looped { "🔁" } else { "⛔" }),
            CreateSelectMenuOption::new(
                if settings.shuffle {
                    "Queue Shuffled"
                } else {
                    "Queue Not Shuffled"
                },
                "shuffle",
            )
            .description(if settings.shuffle { "🔀" } else { "⛔" }),
            CreateSelectMenuOption::new(
                if settings.repeat {
                    "Song Repeated"
                } else {
                    "Song Not Repeated"
                },
                "repeat",
            )
            .description(if settings.repeat { "🔄️" } else { "⛔" }),
            CreateSelectMenuOption::new(
                if settings.autoplay {
                    "Autoplay Enabled"
                } else {
                    "Autoplay Disabled"
                },
                "autoplay",
            )
            .description(if settings.autoplay { "🎲" } else { "⛔" }),
            CreateSelectMenuOption::new("Remove", "remove").description("🗑️"),
            CreateSelectMenuOption::new(
                if settings.read_titles {
                    "Will Read Titles"
                } else {
                    "Will Not Read Titles"
                },
                "read_titles",
            )
            .description("🗣️"),
            CreateSelectMenuOption::new("Bitrate", "bitrate").description(match settings.bitrate {
                OrAuto::Specific(i) => {
                    if i >= 1000 {
                        format!("{}kbps", i / 1000)
                    } else {
                        format!("{}bps", i)
                    }
                }
                OrAuto::Auto => "Auto".to_owned(),
            }),
        ];
        if !settings.log_empty {
            options.push(CreateSelectMenuOption::new("Log", "log").description("📜"));
        }
        vec![CreateActionRow::SelectMenu(
            CreateSelectMenu::new("::controls", CreateSelectMenuKind::String { options })
                .max_values(1)
                .min_values(1),
        )]
    }
    fn filter_bar_emojis(string: &str) -> String {
        let mut str = string.to_owned();
        let bar_emojis = vec![
            "<:LE:1038954704744480898>",
            "<:LC:1038954708422885386>",
            "<:CE:1038954710184497203>",
            "<:CC:1038954696980824094>",
            "<:RE:1038954703033217285>",
            "<:RC:1038954706841649192>",
        ];
        for emoji in bar_emojis {
            str = str.replace(emoji, "");
        }
        str
    }
}
#[derive(Debug)]
pub struct RawMessage {
    pub author_id: String,
    pub channel_id: ChannelId,
    pub channel_name: Option<String>,

    pub timestamp: Timestamp,
    pub tts_audio_handle: Option<tokio::task::JoinHandle<Result<Video, anyhow::Error>>>,
}
impl RawMessage {
    pub async fn check_tts(
        &mut self,
    ) -> Result<Option<Result<Video, anyhow::Error>>, anyhow::Error> {
        if let Some(handle) = self.tts_audio_handle.take() {
            if handle.is_finished() {
                Ok(Some(handle.await?))
            } else {
                self.tts_audio_handle = Some(handle);
                Ok(None)
            }
        } else {
            Err(anyhow::anyhow!("TTS audio handle is None"))
        }
    }
    pub fn announcement(msg: &Message, text: String, voice: &TTSVoice) -> Self {
        Self {
            author_id: String::from("Announcement"),
            channel_id: msg.channel_id,
            channel_name: None,
            timestamp: msg.timestamp,
            tts_audio_handle: Some(Self::audio_handle(text, *voice)),
        }
    }
    pub async fn message(ctx: &Context, msg: &Message, voice: &TTSVoice) -> Result<Self, Error> {
        let safecontent = msg.content_safe(&ctx.cache);
        let finder = linkify::LinkFinder::new();
        let links: Vec<_> = finder.links(&safecontent).map(|l| l.as_str()).collect();
        let mut safecontent = safecontent.replace("#0000", "");
        let emojis = detect_emojis(&safecontent);
        for emoji in emojis {
            safecontent = safecontent.replace(&emoji.raw_emoji_text, &emoji.name);
        }
        let mut filteredcontent = safecontent.to_string();
        for link in links {
            filteredcontent = filteredcontent.replace(link, "");
        }
        filteredcontent = filteredcontent.trim().to_lowercase().to_string();
        if filteredcontent.is_empty() {
            return Err(anyhow::anyhow!("Message is empty"));
        }
        if let Some(othermsg) = msg.referenced_message.as_ref() {
            filteredcontent = format!("Replying to {}:\n{}", othermsg.author.name, filteredcontent)
        }
        let channelname = match msg.channel(&ctx).await {
            Ok(Channel::Guild(channel)) => channel.name,
            Ok(Channel::Private(private)) => private.name(),
            Ok(_) => String::from("Unknown"),
            Err(_) => {
                return Err(anyhow::anyhow!("Failed to get channel name"));
            }
        };
        Ok(Self {
            author_id: msg.author.id.to_string(),
            channel_name: Some(channelname),
            channel_id: msg.channel_id,
            timestamp: msg.timestamp,
            tts_audio_handle: Some(Self::audio_handle(filteredcontent, *voice)),
        })
    }
    pub fn audio_handle(
        text: String,
        voice: TTSVoice,
    ) -> tokio::task::JoinHandle<Result<Video, anyhow::Error>> {
        tokio::task::spawn(async move {
            let key = match crate::youtube::get_access_token().await {
                Ok(key) => key,
                Err(e) => {
                    return Err(e);
                }
            };
            crate::youtube::get_tts(text, key, Some(voice)).await
        })
    }
}
fn detect_emojis(safecontent: &str) -> Vec<EmojiData> {
    let mut emojis: Vec<EmojiData> = Vec::new();
    let regex = regex::Regex::new(r"<a?:([^:]+):\d+>").expect("Failed to create regex");
    for cap in regex.captures_iter(safecontent) {
        let name = match cap.get(1) {
            Some(name) => name.as_str(),
            None => continue,
        };
        let raw_emoji_text = match cap.get(0) {
            Some(text) => text.as_str(),
            None => continue,
        };
        emojis.push(EmojiData {
            name: name.to_string(),
            raw_emoji_text: raw_emoji_text.to_string(),
        });
    }
    emojis.sort_by(|a, b| a.name.cmp(&b.name));
    emojis.dedup_by(|a, b| a.name == b.name);
    emojis
}
#[derive(Debug, Clone)]
pub struct EmojiData {
    pub name: String,
    pub raw_emoji_text: String,
}
