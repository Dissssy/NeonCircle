#![feature(try_blocks)]
pub mod add;
pub mod autoplay;
pub mod consent;
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
pub mod voice_preference;
pub mod volume;
use self::mainloop::EmbedData;
use self::settingsdata::SettingsData;
use common::anyhow::{self, Result};
#[cfg(not(feature = "new-controls"))]
use common::serenity::all::{ButtonStyle, CreateButton};
use common::serenity::all::{
    Cache, Channel, ChannelId, ChannelType, Context, CreateActionRow, CreateMessage, EditMessage,
    GetMessages, GuildChannel, GuildId, Http, Message, MessageFlags, UserId,
};
#[cfg(feature = "new-controls")]
use common::serenity::all::{CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption};
#[cfg(feature = "transcribe")]
use common::serenity::all::{CreateThread, CreateWebhook};
use common::video::Video;
use common::youtube::TTSVoice;
use common::{log, songbird, tokio};
#[cfg(feature = "transcribe")]
use serde_json::json;
use songbird::typemap::TypeMapKey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Instant;
pub struct AudioHandler;
impl TypeMapKey for AudioHandler {
    type Value = Arc<RwLock<HashMap<ChannelId, tokio::task::JoinHandle<()>>>>;
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
    async fn change_channel(&mut self, channel_id: ChannelId) -> Result<()> {
        self.channel_id = channel_id;
        // delete and resend
        self.delete().await?;
        self.send_new().await?;
        Ok(())
    }
    #[cfg(feature = "transcribe")]
    async fn send_manually(&mut self, content: common::PostSomething, user: UserId) -> Result<()> {
        use common::{chrono, reqwest, PostSomething, WEB_CLIENT};
        if cfg!(feature = "send_to_thread") {
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
                                // chrono::Local::now()
                                //     .format("CLOSED CAPTIONS FOR %b %-d, %Y at %-I:%M%p")
                                //     .to_string(),
                                format!(
                                    "CLOSED CAPTIONS FOR {}",
                                    common::utils::full_datetime_format(
                                        &chrono::Utc::now()
                                            .with_timezone(&common::chrono_tz::Tz::EST5EDT),
                                        true
                                    )
                                ),
                            ),
                        )
                        .await
                        .inspect_err(|e| {
                            log::error!("Error creating thread: {:?}", e);
                            self.transcription_thread = OptionOrFailed::Failed;
                        })?;
                    let id = thread.id;
                    self.transcription_thread = OptionOrFailed::Some(thread);
                    id
                }
            };
            let author = self.http.get_user(user).await?;
            let webhook_url = format!("{}?thread_id={}", webhook.url()?, thread_id);
            match content {
                PostSomething::Text(text) => {
                    WEB_CLIENT
                        .post(&webhook_url)
                        .json(&json!({
                            "content": text,
                            "username": author.name,
                            "avatar_url": author.avatar_url().unwrap_or_else(|| author.default_avatar_url()),
                            "allowed_mentions": {
                                "parse": []
                            }
                        })).send().await?;
                }
                PostSomething::Attachment { name, data } => {
                    WEB_CLIENT
                        .post(&webhook_url)
                        .multipart({
                            let mut builder = reqwest::multipart::Form::new();
                            let mut payload = json!({
                                "username": author.name,
                                "avatar_url": author.avatar_url().unwrap_or_else(|| author.default_avatar_url()),
                                "allowed_mentions": {
                                    "parse": []
                                }
                            });
                            builder = builder.part("files[0]", reqwest::multipart::Part::bytes(data).file_name(name.to_string()));
                            if let Some(attachments) = payload.get_mut("attachments") {
                                let mut new = json!([
                                    {
                                        "id": 0,
                                        "description": "Transcription",
                                        "filename": "transcription.mp3",
                                    }
                                ]);
                                std::mem::swap(attachments, &mut new);
                            }
                            builder.text("payload_json", serde_json::to_string(&payload)?)
                        })
                        .send()
                        .await?;
                }
            }
        } else {
            match content {
                PostSomething::Text(text) => {
                    log::trace!("Would have sent: {}", text);
                }
                PostSomething::Attachment { name, data } => {
                    log::trace!("Would have sent: a {} byte file named {}", data.len(), name);
                }
            }
        }
        Ok(())
    }
    async fn update(&mut self, settings: SettingsData, content: EmbedData) -> Result<()> {
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
        // let diff = match self.last_content {
        //     None => true,
        //     Some(ref last_content) => last_content != &content,
        // };
        // let forcediff = match self.last_settings {
        //     None => true,
        //     Some(ref last_settings) => last_settings != &settings,
        // };
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
        // if (diff && ((self.last_edit.elapsed().as_millis() > self.edit_delay) || self.first_time))
        //     || forcediff
        {
            self.first_time = false;
            let write_content = content.clone();
            self.last_content = Some(content);
            self.last_settings = Some(settings);
            if self.resend_next_time {
                self.resend_next_time = false;
                if let Err(e) = self.delete().await {
                    log::error!("Error deleting message: {:?}", e);
                }
                self.send_new().await?;
            } else if let Err(e) = message
                .edit(&self.http, {
                    let mut m = EditMessage::new()
                        .content("")
                        .embed(write_content.to_serenity())
                        .flags(MessageFlags::SUPPRESS_NOTIFICATIONS);
                    if let Some(ars) = self.last_settings.as_ref().map(Self::get_ars) {
                        m = m.components(ars);
                    }
                    m
                })
                .await
            {
                log::error!("Error editing message: {:?}", e);
                self.send_new().await?;
            };
            self.last_edit = Instant::now();
        }
        Ok(())
    }
    async fn send_new(&mut self) -> Result<()> {
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
                            .content("")
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
    async fn delete(&mut self) -> Result<()> {
        if let Some(message) = self.message.take() {
            message.delete(&self.http).await?;
        };
        Ok(())
    }
    async fn final_cleanup(&mut self) -> Result<()> {
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
                        match settings.raw_song_volume() {
                            v if v == 0.0 => "🔇",

                            v if v <= 0.33 => "🔈",

                            v if v <= 0.66 => "🔉",

                            _ => "🔊",
                        },
                        settings.raw_song_volume() * 100.0
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
        use common::audio::OrAuto;
        let mut options = vec![
            // CreateSelectMenuOption::new("Bot Controls", "controls")
            //     .description("🎛️")
            //     .default_selection(true),
            CreateSelectMenuOption::new("Volume", "volume").description(format!(
                "{} {:.0}%",
                match settings.display_song_volume() {
                    v if v == 0.0 => "🔇",

                    v if v <= 0.33 => "🔈",

                    v if v <= 0.66 => "🔉",

                    _ => "🔊",
                },
                settings.display_song_volume() * 100.0
            )),
            CreateSelectMenuOption::new("Radio Volume", "radiovolume").description(format!(
                "📻 {:.0}%",
                settings.display_radio_volume() * 100.0
            )),
            CreateSelectMenuOption::new(
                // if settings.something_playing {
                //     "Playing"
                // } else {
                //     "Paused"
                // },
                "Pause", "pause",
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
                .placeholder("Bot Controls")
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
#[derive(Debug, Clone)]
pub struct RawMessage {
    // pub author_id: String,
    // pub channel_id: ChannelId,
    // pub channel_name: Option<String>,

    // pub timestamp: Timestamp,
    // pub tts: Arc<RwLock<TTSStatus>>,
    // pub tts_audio_handle: Option<tokio::task::JoinHandle<Result<()>>>,
}
// #[derive(Debug, Clone)]
// pub enum TTSStatus {
//     Pending,
//     Errored,
//     Finished(Video),
// }
// impl Clone for RawMessage {
//     fn clone(&self) -> Self {
//         Self {
//             author_id: self.author_id.clone(),
//             channel_id: self.channel_id,
//             channel_name: self.channel_name.clone(),
//             timestamp: self.timestamp,
//             tts: Arc::clone(&self.tts),
//             tts_audio_handle: None,
//         }
//     }
// }
impl RawMessage {
    // pub async fn check_tts(&mut self) -> TTSStatus {
    //     if let Some(handle) = self.tts_audio_handle.take() {
    //         if handle.is_finished() {
    //             match handle.await {
    //                 Ok(Ok(())) => {}
    //                 Ok(Err(e)) => {
    //                     log::error!("Error in TTS: {:?}", e);
    //                     let mut lock = self.tts.write().await;
    //                     *lock = TTSStatus::Errored;
    //                 }
    //                 Err(e) => {
    //                     log::error!("Error in thread: {:?}", e);
    //                     let mut lock = self.tts.write().await;
    //                     *lock = TTSStatus::Errored;
    //                 }
    //             }
    //         } else {
    //             self.tts_audio_handle = Some(handle);
    //         }
    //     }
    //     let lock = self.tts.read().await;
    //     lock.clone()
    // }
    // pub async fn consume_handle(&mut self) -> Option<Video> {
    //     if let Some(handle) = self.tts_audio_handle.take() {
    //         handle.await.ok()?.ok()?;
    //         let lock = self.tts.read().await;
    //         match lock.clone() {
    //             TTSStatus::Finished(video) => Some(video),
    //             _ => None,
    //         }
    //     } else {
    //         None
    //     }
    // }
    // pub fn announcement(msg: &Message, text: String, voice: &TTSVoice) -> Self {
    //     let tts = Arc::new(RwLock::new(TTSStatus::Pending));
    //     Self {
    //         author_id: String::from("Announcement"),
    //         channel_id: msg.channel_id,
    //         channel_name: None,
    //         timestamp: msg.timestamp,
    //         tts_audio_handle: Some(Self::audio_handle(text, *voice, Arc::clone(&tts))),
    //         tts,
    //     }
    // }
    async fn announcement(
        // msg: &Message,
        text: String,
        voice: &TTSVoice,
    ) -> Result<Video> {
        Self::audio_handle(text, *voice).await
    }
    // pub async fn message(ctx: &Context, msg: &Message, voice: &TTSVoice) -> Result<Self> {
    //     let safecontent = msg.content_safe(&ctx.cache);
    //     let finder = linkify::LinkFinder::new();
    //     let links: Vec<_> = finder.links(&safecontent).map(|l| l.as_str()).collect();
    //     let mut safecontent = safecontent.replace("#0000", "");
    //     let emojis = detect_emojis(&safecontent);
    //     for emoji in emojis {
    //         safecontent = safecontent.replace(&emoji.raw_emoji_text, &emoji.name);
    //     }
    //     let mut filteredcontent = safecontent.to_string();
    //     for link in links {
    //         filteredcontent = filteredcontent.replace(link, "");
    //     }
    //     filteredcontent = filteredcontent.trim().to_lowercase().to_string();
    //     if filteredcontent.is_empty() {
    //         return Err(anyhow::anyhow!("Message is empty"));
    //     }
    //     if let Some(othermsg) = msg.referenced_message.as_ref() {
    //         filteredcontent = format!("Replying to {}:\n{}", othermsg.author.name, filteredcontent)
    //     }
    //     let channelname = match msg.channel(&ctx).await {
    //         Ok(Channel::Guild(channel)) => channel.name,
    //         Ok(Channel::Private(private)) => private.name(),
    //         Ok(_) => String::from("Unknown"),
    //         Err(_) => {
    //             return Err(anyhow::anyhow!("Failed to get channel name"));
    //         }
    //     };
    //     let tts = Arc::new(RwLock::new(TTSStatus::Pending));
    //     Ok(Self {
    //         author_id: msg.author.id.to_string(),
    //         channel_name: Some(channelname),
    //         channel_id: msg.channel_id,
    //         timestamp: msg.timestamp,
    //         tts_audio_handle: Some(Self::audio_handle(
    //             filteredcontent,
    //             *voice,
    //             Arc::clone(&tts),
    //         )),
    //         tts,
    //     })
    // }
    async fn message(ctx: &Context, msg: impl AsRef<Message>, voice: &TTSVoice) -> Result<Video> {
        let safecontent = msg.as_ref().content_safe(&ctx.cache);
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
        if let Some(othermsg) = msg.as_ref().referenced_message.as_ref() {
            filteredcontent = format!("Replying to {}:\n{}", othermsg.author.name, filteredcontent)
        }
        // let channelname = match msg.channel(&ctx).await {
        //     Ok(Channel::Guild(channel)) => channel.name,
        //     Ok(Channel::Private(private)) => private.name(),
        //     Ok(_) => String::from("Unknown"),
        //     Err(_) => {
        //         return Err(anyhow::anyhow!("Failed to get channel name"));
        //     }
        // };
        Self::audio_handle(filteredcontent, *voice).await
    }
    // pub fn audio_handle(
    //     text: String,
    //     voice: TTSVoice,
    //     tts: Arc<RwLock<TTSStatus>>,
    // ) -> tokio::task::JoinHandle<Result<()>> {
    //     tokio::task::spawn(async move {
    //         let key = crate::youtube::get_access_token().await?;
    //         let res = crate::youtube::get_tts(text, key, Some(voice)).await?;
    //         let mut lock = tts.write().await;
    //         *lock = TTSStatus::Finished(res);
    //         Ok(())
    //     })
    // }
    async fn audio_handle(
        text: String,
        voice: TTSVoice,
        // call: &Arc<Mutex<Call>>,
    ) -> Result<Video> {
        common::youtube::get_tts(text, Some(voice)).await
        // let res = crate::youtube::get_tts(text, key, Some(voice)).await?;
        // let handle = {
        //     let mut clock = call.lock().await;
        //     clock.play(Track::new(res.to_songbird()).pause())
        // };
        // Ok(HandleMetadata::process_handle(HandleType::Tts(handle)).await?)
    }
}
fn detect_emojis(safecontent: &str) -> Vec<EmojiData> {
    let mut emojis: Vec<EmojiData> = Vec::new();
    let regex = match regex::Regex::new(r"<a?:([^:]+):\d+>") {
        Ok(regex) => regex,
        Err(e) => {
            log::error!("Failed to create regex: {:?}", e);
            return emojis;
        }
    };
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
// pub async fn get_transcribe_channel_handler(
//     ctx: &Context,
//     guild_id: &GuildId,
// ) -> Result<Arc<RwLock<TranscribeChannelHandler>>> {
//     let transcribe = {
//         let data = ctx.data.read().await;
//         match data.get::<TranscribeData>() {
//             Some(v) => Arc::clone(v),
//             None => {
//                 return Err(anyhow::anyhow!("Failed to get transcribe data"));
//             }
//         }
//     };
//     let mut data = transcribe.write().await;
//     Ok(match data.entry(*guild_id) {
//         std::collections::hash_map::Entry::Occupied(e) => Arc::clone(e.get()),
//         std::collections::hash_map::Entry::Vacant(e) => {
//             let v = Arc::new(RwLock::new(TranscribeChannelHandler::new()));
//             e.insert(Arc::clone(&v));
//             v
//         }
//     })
// }
