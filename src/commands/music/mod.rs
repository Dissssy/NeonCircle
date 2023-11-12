pub mod autoplay;
pub mod join;
pub mod loopit;
pub mod mainloop;
pub mod pause;
pub mod play;
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

use serenity::builder::CreateActionRow;
use serenity::client::Cache;
use serenity::http::Http;
// use serenity::model::prelude::interaction::application_command::ApplicationCommandInteraction;

use serenity::model::prelude::{ChannelId, GuildId, Member, Message, UserId};
use serenity::model::voice::VoiceState;
use serenity::prelude::Mutex;
use tokio::time::Instant;

use std::collections::HashMap;

use std::fmt::Display;
use std::sync::Arc;

use anyhow::Error;

use serenity::futures::channel::mpsc;

use serenity::prelude::{Context, TypeMapKey};

use crate::video::Video;
use crate::youtube::{TTSVoice, VideoInfo};

use self::mainloop::EmbedData;
use self::settingsdata::SettingsData;

// create the struct for holding the promises for audio playback
pub struct AudioHandler;

impl TypeMapKey for AudioHandler {
    type Value = Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>;
}

pub struct AudioCommandHandler;

impl TypeMapKey for AudioCommandHandler {
    type Value = Arc<
        Mutex<
            HashMap<
                String,
                mpsc::UnboundedSender<(
                    serenity::futures::channel::oneshot::Sender<String>,
                    AudioPromiseCommand,
                )>,
            >,
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
        let bot_id = ctx.cache.current_user_id();
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

        // if let Some(memberstate) = memberstate {
        //     if let Some(botstate) = botstate {
        //         if memberstate == botstate {
        //             VoiceAction::InSame(memberstate)
        //         } else {
        //             VoiceAction::InDifferent(memberstate)
        //         }
        //     } else {
        //         VoiceAction::Join(memberstate)
        //     }
        // } else {
        //     VoiceAction::UserNotConnected
        // }
    }
    pub async fn refresh_guild(&mut self, ctx: &Context, guild_id: GuildId) -> Result<(), Error> {
        let mut new = GuildVc::new();

        for (_i, channel) in ctx
            .http
            .get_guild(guild_id.0)
            .await?
            .channels(&ctx.http)
            .await?
        {
            if channel.kind == serenity::model::channel::ChannelType::Voice {
                let mut newchannel = HashMap::new();
                for member in match ctx.http.get_channel(channel.id.0).await {
                    Ok(serenity::model::channel::Channel::Guild(channel)) => channel,
                    Err(_) => {
                        continue;
                    }
                    _ => return Err(anyhow::anyhow!("Expected Guild channel")),
                }
                .members(&ctx.cache)
                .await?
                {
                    newchannel.insert(
                        member.user.id,
                        UserMetadata {
                            member: member.clone(),
                            // deaf: member.deaf,
                            mute: member.mute,
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
        // if the bot is alone in a channel in the guild return true

        let guild = self.guilds.entry(*guild).or_insert_with(GuildVc::new);
        let channel = match guild.find_user(self.bot_id) {
            Some(channel) => channel,
            None => return false,
        };
        let channel = guild.channels.entry(channel).or_insert_with(HashMap::new);

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
                // we need to remove the old state
                if let Some(channel) = old.channel_id {
                    let channel = self.channels.entry(channel).or_insert_with(HashMap::new);
                    channel.remove(&old.user_id);
                }
            }
        }
        if let (Some(channel), Some(member)) = (new.channel_id, new.member.clone()) {
            let channel = self.channels.entry(channel).or_insert_with(HashMap::new);
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
    // pub deaf: bool,
    pub mute: bool,
    //     pub self_deaf: bool,
    //     pub self_mute: bool,
    //     pub suppress: bool,
}

impl UserMetadata {
    pub fn new(member: Member, state: VoiceState) -> Self {
        Self {
            member,
            // deaf: state.deaf,
            mute: state.mute,
            // self_deaf: state.self_deaf,
            // self_mute: state.self_mute,
            // suppress: state.suppress,
        }
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
    // probably not gonna do
    Play(Vec<MetaVideo>),

    // added
    Paused(OrToggle),
    Stop,
    Loop(OrToggle),
    Repeat(OrToggle),
    Shuffle(OrToggle),
    Autoplay(OrToggle),

    // needs impl??
    Volume(f64),
    SpecificVolume(SpecificVolume),
    SetBitrate(OrAuto),

    // todo
    Skip,
    Remove(usize),
    // Transcribe(bool, MessageId),

    // META
    RetrieveLog(serenity::futures::channel::mpsc::Sender<Vec<String>>),
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

#[derive(Debug, Clone)]
pub struct MetaVideo {
    pub video: VideoType,
    pub title: String,
    #[cfg(feature = "tts")]
    pub ttsmsg: Option<Video>,
}

impl MetaVideo {
    pub fn delete(&mut self) -> Result<(), Error> {
        match self.video {
            VideoType::Disk(ref mut video) => video.delete(),
            _ => Ok(()),
        }?;
        #[cfg(feature = "tts")]
        if let Some(ref mut ttsmsg) = self.ttsmsg {
            ttsmsg.delete()?;
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
    // message_queue: Arc<Mutex<Vec<RawMessage>>>,
    // voicemap: Vec<String>,
    // messages_sent_to_tts: Vec<u64>,
    // pub last_processed: Option<MessageId>,
    resend_next_time: bool,
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
            edit_delay: 1500,
            // message_queue: Arc::new(Mutex::new(Vec::new())),
            // voicemap: Vec::new(),
            // messages_sent_to_tts: Vec::new(),
            // last_processed: None,
            resend_next_time: false,
        }
    }
    async fn update(&mut self, settings: SettingsData, content: EmbedData) -> Result<(), Error> {
        // let addbackticks = content.ends_with("```");
        // let mut content = content.to_string();
        // if content.len() > 2000 {
        //     content.truncate(1990);
        //     content.push_str("...");
        //     if addbackticks {
        //         content.push_str("\n```");
        //     }
        // }

        // let Some(message) = self.message.as_mut() else {
        //     // return Err(anyhow::anyhow!("Message is None"));
        //     self.send_new().await?;

        // }

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

        // let (orig_content, new_content) = (message.content.as_str().trim(), content.trim());
        // let diff = Self::is_different_enough(new_content, orig_content, 3);

        let diff = match self.last_content {
            None => true,
            Some(ref last_content) => last_content != &content,
        };

        let forcediff = match self.last_settings {
            None => true,
            Some(ref last_settings) => last_settings != &settings,
        };

        let mut messages = match message.channel(&self.http).await? {
            serenity::model::prelude::Channel::Guild(channel) => {
                channel
                    .messages(&self.http, |retriever| {
                        // if let Some(p) = self.last_processed {
                        // retriever.after(p)
                        // } else {
                        retriever
                            .after(message.id)
                            // }
                            .limit(100)
                    })
                    .await?
            }
            serenity::model::prelude::Channel::Private(channel) => {
                channel
                    .messages(&self.http, |retriever| {
                        // if let Some(p) = self.last_processed {
                        //     retriever.after(p)
                        // } else {
                        retriever
                            .after(message.id)
                            // }
                            .limit(100)
                    })
                    .await?
            }
            serenity::model::prelude::Channel::Category(_) => {
                // this should never happen so we're just going to send a new message
                Vec::new()
            }
            _ => Vec::new(),
        };
        messages.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        // let key = crate::youtube::get_access_token().await?;
        for rawmessage in messages {
            // let is_bot = rawmessage.author.bot;
            // let this_id = rawmessage.id;
            // let is_empty = rawmessage.content.is_empty();
            // let is_silent = rawmessage
            //     .flags
            //     .map(|f| {
            //         f.contains(
            //             serenity::model::channel::MessageFlags::from_bits(1u64 << 12)
            //                 .expect("Failed to create message flags"),
            //         )
            //     })
            //     .unwrap_or(false);

            // let is_this_bot = rawmessage.author.id == self.http.get_current_user().await?.id;
            // let is_in_list = self.messages_sent_to_tts.contains(rawmessage.id.as_u64());

            if rawmessage.id > message.id {
                self.resend_next_time = true;
            }

            // if !is_this_bot {
            // if let Some(m) = self.last_processed {
            // if this_id <= m {
            // println!("Skipping message: {:?}", rawmessage.id);
            // continue;
            // } else {
            // println!("Setting last_processed to: {:?}", this_id);
            // self.last_processed = Some(this_id);
            // }
            // if (!is_bot) && (!is_empty) && (!is_silent) && transcribed {
            //     let safecontent = rawmessage.content_safe(&self.cache);

            //     let finder = linkify::LinkFinder::new();
            //     let links: Vec<_> =
            //         finder.links(&safecontent).map(|l| l.as_str()).collect();

            //     let mut filteredcontent = safecontent.to_string();

            //     for link in links {
            //         filteredcontent = filteredcontent.replace(link, "");
            //     }
            //     filteredcontent = filteredcontent.trim().to_string();
            //     let (msg, key) = (filteredcontent.clone(), key.clone());
            //     let author_id = rawmessage.author.id.to_string();
            //     let voice = {
            //         // if author_id is in voicemap then get the index of it, else push it to voicemap and get the index of it
            //         let mut index = self.voicemap.len();
            //         for (i, id) in self.voicemap.iter().enumerate() {
            //             if id == &author_id {
            //                 index = i;
            //                 break;
            //             }
            //         }
            //         if index == self.voicemap.len() {
            //             let announce = format!(
            //                 "{} is now using this voice to speak.",
            //                 rawmessage.author.name
            //             );
            //             let key = key.clone();
            //             {
            //                 let mut mq = self.message_queue.lock().await;
            //                 mq.push(RawMessage {
            //                     author: rawmessage.author.name.clone(),
            //                     author_id: rawmessage.author.id.to_string(),
            //                     channel_id: rawmessage.channel_id,
            //                     content: announce.clone(),
            //                     timestamp: rawmessage.timestamp,
            //                     tts_audio_handle: Some(tokio::task::spawn(async move {
            //                         Ok(
            //                             match crate::youtube::get_tts(
            //                                 announce,
            //                                 key,
            //                                 Some(index),
            //                             )
            //                             .await?
            //                             {
            //                                 VideoType::Disk(v) => v,
            //                                 VideoType::Url(_) => {
            //                                     return Err(anyhow::anyhow!(
            //                                         "Expected Disk video, got Url video"
            //                                     ))
            //                                 }
            //                             },
            //                         )
            //                     })),
            //                 });
            //             }
            //             self.voicemap.push(author_id.clone());
            //         }
            //         index
            //     };
            //     {
            //         let mut mq = self.message_queue.lock().await;
            //         mq.push(RawMessage {
            //             author: rawmessage.author.name,
            //             author_id,
            //             channel_id: rawmessage.channel_id,
            //             content: filteredcontent,
            //             timestamp: rawmessage.timestamp,
            //             tts_audio_handle: Some(tokio::task::spawn(async move {
            //                 Ok(
            //                     match crate::youtube::get_tts(msg, key, Some(voice)).await?
            //                     {
            //                         VideoType::Disk(v) => v,
            //                         VideoType::Url(_) => {
            //                             return Err(anyhow::anyhow!(
            //                                 "Expected Disk video, got Url video"
            //                             ))
            //                         }
            //                     },
            //                 )
            //             })),
            //         });
            //     }
            // }
            // }
            // }
        }

        if (diff && ((self.last_edit.elapsed().as_millis() > self.edit_delay) || self.first_time))
            || forcediff
        {
            self.first_time = false;
            let write_content = content.clone();
            self.last_content = Some(content);
            self.last_settings = Some(settings);
            // match message
            //     .edit(&self.http, |m| {
            //         m.content("".to_string());
            //         m.embed(move |e| {
            //             write_content.write_into(e);
            //             e
            //         })
            //     })
            //     .await
            // {
            //     Ok(_) => {}
            //     Err(e) => {
            //         println!("Error editing message: {:?}", e);
            //         self.send_new().await?;
            //     }
            // };

            // if match rawchannel {
            //     serenity::model::prelude::Channel::Guild(channel) => {
            //         channel.last_message_id == Some(message.id)
            //     }
            //     serenity::model::prelude::Channel::Private(channel) => {
            //         channel.last_message_id == Some(message.id)
            //     }
            //     serenity::model::prelude::Channel::Category(_) => {
            //         // this should never happen so we're just going to send a new message
            //         false
            //     }
            //     _ => false,
            // }
            if self.resend_next_time {
                self.resend_next_time = false;
                if let Err(e) = self.delete().await {
                    println!("Error deleting message: {:?}", e);
                }
                self.send_new().await?;
            } else {
                match message
                    .edit(&self.http, |m| {
                        m.content("".to_string());
                        m.embed(move |e| {
                            write_content.write_into(e);
                            e
                        });
                        if let Some(ars) = self.last_settings.as_ref().map(Self::get_ars) {
                            m.components(|f| {
                                for ar in ars {
                                    f.add_action_row(ar);
                                }
                                f
                            })
                        } else {
                            m
                        }
                    })
                    .await
                {
                    Ok(_) => {}
                    Err(e) => {
                        println!("Error editing message: {:?}", e);
                        self.send_new().await?;
                    }
                };
            }

            self.last_edit = Instant::now();
        }
        Ok(())
    }
    // async fn send_new(&mut self) -> Result<(), Error> {
    //     let content = if let Some(msg) = self.message.as_ref() {
    //         msg.content.clone()
    //     } else {
    //         String::from("Loading...")
    //     };
    //     let message = self
    //         .channel_id
    //         .send_message(&self.http, |m| m.content(content))
    //         .await?;
    //     self.message = Some(message);
    //     Ok(())
    // }

    // async fn get_messages(&mut self) -> Result<Vec<RawMessage>, anyhow::Error> {
    //     let mut mq = self.message_queue.lock().await;
    //     let mut messages = Vec::new();
    //     std::mem::swap(&mut messages, &mut mq);
    //     Ok(messages)
    // }

    async fn send_new(&mut self) -> Result<(), Error> {
        match self.last_content {
            None => {
                let content = String::from("<a:earloading:979852072998543443>");
                let message = self
                    .channel_id
                    .send_message(&self.http, |m| {
                        m.content(content)
                            // SUPPRESS_NOTIFICATIONS	1 << 12	this message will not trigger push and desktop notifications
                            .flags(
                                serenity::model::channel::MessageFlags::from_bits(1u64 << 12)
                                    .expect("Failed to create message flags"),
                            );
                        if let Some(ars) = self.last_settings.as_ref().map(Self::get_ars) {
                            m.components(|f| {
                                for ar in ars {
                                    f.add_action_row(ar);
                                }
                                f
                            })
                        } else {
                            m
                        }
                    })
                    .await?;
                // self.messages_sent_to_tts = Vec::new();
                self.message = Some(message);
                Ok(())
            }
            Some(ref content) => {
                let write_content = content.clone();
                let message = self
                    .channel_id
                    .send_message(&self.http, |m| {
                        m.content("".to_string());
                        m.embed(move |e| {
                            write_content.write_into(e);
                            e
                        })
                        .flags(
                            serenity::model::channel::MessageFlags::from_bits(1u64 << 12)
                                .expect("Failed to create message flags"),
                        );

                        if let Some(ars) = self.last_settings.as_ref().map(Self::get_ars) {
                            m.components(|f| {
                                for ar in ars {
                                    f.add_action_row(ar);
                                }
                                f
                            })
                        } else {
                            m
                        }
                    })
                    .await?;
                // self.messages_sent_to_tts = Vec::new();
                self.message = Some(message);
                Ok(())
            }
        }
    }
    async fn delete(&mut self) -> Result<(), Error> {
        let Some(message) = self.message.as_mut() else {
            return Err(anyhow::anyhow!("Message is None"));
        };
        message.delete(&self.http).await?;
        self.message = None;
        Ok(())
    }

    fn get_ars(settings: &SettingsData) -> Vec<CreateActionRow> {
        // if let Some(settings) = self.last_settings.as_ref() {
        // [volume] [radiovolume] [bitrate] [pause]
        // [looped] [repeat] [shuffle] [autoplay]

        // volume, radiovolume, and bitrate will be grey and non-clickable (for now, maybe a modal later?)
        // pause, looped, repeat, shuffle, autoplay will be clickable toggles, red when off green when on

        // possibly later we can do an extra button at the bottom to submit a song url/search to the queue (but will lack feedback bc discord SUCKS)

        let mut ars = Vec::new();

        // AR3 (volume, radiovolume, bitrate, ?log?)

        let mut ar = CreateActionRow::default();

        let (volumestyle, radiostyle) = if settings.something_playing {
            (
                serenity::model::application::component::ButtonStyle::Primary,
                serenity::model::application::component::ButtonStyle::Secondary,
            )
        } else {
            (
                serenity::model::application::component::ButtonStyle::Secondary,
                serenity::model::application::component::ButtonStyle::Primary,
            )
        };

        // volume
        ar.create_button(|b| {
            b.style(volumestyle)
                // .emoji(ReactionType::Unicode(
                //     match settings.volume {
                //         // 0%
                //         v if v == 0.0 => "🔇",
                //         // 0-33%
                //         v if v <= 0.33 => "🔈",
                //         // 33-66%
                //         v if v <= 0.66 => "🔉",
                //         // 66-100%
                //         _ => "🔊",
                //     }
                //     .to_owned(),
                // ))
                .custom_id("volume")
                // .disabled(true)
                .label(format!(
                    "{} {}%",
                    match settings.raw_volume() {
                        // 0%
                        v if v == 0.0 => "🔇",
                        // 0-33%
                        v if v <= 0.33 => "🔈",
                        // 33-66%
                        v if v <= 0.66 => "🔉",
                        // 66-100%
                        _ => "🔊",
                    },
                    settings.raw_volume() * 100.0
                ))
        });

        // radiovolume
        ar.create_button(|b| {
            b.style(radiostyle)
                // .emoji(ReactionType::Unicode("📻".to_owned()))
                .custom_id("radiovolume")
                // .disabled(true)
                .label(format!("📻 {}%", settings.raw_radiovolume() * 100.0))
        });

        // bitrate
        ar.create_button(|b| {
            b.style(serenity::model::application::component::ButtonStyle::Secondary)
                // .emoji(ReactionType::Unicode("📡".to_owned()))
                .custom_id("bitrate")
                // .disabled(true)
                .label(match settings.bitrate {
                    OrAuto::Specific(i) => {
                        if i >= 1000 {
                            format!("{}kbps", i / 1000)
                        } else {
                            format!("{}bps", i)
                        }
                    }
                    OrAuto::Auto => "Auto".to_owned(),
                })
        });

        // log
        ar.create_button(|b| {
            b.style(if settings.log_empty {
                serenity::model::application::component::ButtonStyle::Secondary
            } else {
                serenity::model::application::component::ButtonStyle::Danger
            })
            // .emoji(ReactionType::Unicode("📜".to_owned()))
            .custom_id("log")
            // .disabled(true)
            .label("📜")
            .disabled(settings.log_empty)
        });

        ars.push(ar);

        // AR1 (pause, skip, remove, stop)

        let mut ar = CreateActionRow::default();

        // pause
        ar.create_button(|b| {
            b.style(if settings.pause {
                serenity::model::application::component::ButtonStyle::Success
            } else {
                serenity::model::application::component::ButtonStyle::Danger
            })
            .label(if settings.pause { "▶️" } else { "⏸️" }.to_owned())
            .custom_id("pause")
            .disabled(!settings.something_playing)
        });

        // skip
        ar.create_button(|b| {
            b.style(serenity::model::application::component::ButtonStyle::Primary)
                // .emoji(ReactionType::Unicode("⏭️".to_owned()))
                .label("⏭️")
                .custom_id("skip")
                .disabled(!settings.something_playing)
        });

        // remove
        ar.create_button(|b| {
            b.style(serenity::model::application::component::ButtonStyle::Danger)
                // .emoji(ReactionType::Unicode("🗑️".to_owned()))
                .custom_id("remove")
                .label("🗑️")
            // .disabled(true)
        });

        // stop
        ar.create_button(|b| {
            b.style(serenity::model::application::component::ButtonStyle::Danger)
                // .emoji(ReactionType::Unicode("⏹️".to_owned()))
                .custom_id("stop")
                .label("⏹️")
        });

        ars.push(ar);

        // AR2 (looped, shuffle, repeat, autoplay)

        let mut ar = CreateActionRow::default();

        // loop
        ar.create_button(|b| {
            b.style(if settings.looped {
                serenity::model::application::component::ButtonStyle::Primary
            } else {
                serenity::model::application::component::ButtonStyle::Secondary
            })
            // .emoji(ReactionType::Unicode("🔄️".to_owned()))
            .label("🔄️")
            .custom_id("looped")
            // .disabled(true)
        });

        // shuffle
        ar.create_button(|b| {
            b.style(if settings.shuffle {
                serenity::model::application::component::ButtonStyle::Primary
            } else {
                serenity::model::application::component::ButtonStyle::Secondary
            })
            // .emoji(ReactionType::Unicode("🔀".to_owned()))
            .custom_id("shuffle")
            .label("🔀")
            // .disabled(true)
        });

        // repeat
        ar.create_button(|b| {
            b.style(if settings.repeat {
                serenity::model::application::component::ButtonStyle::Primary
            } else {
                serenity::model::application::component::ButtonStyle::Secondary
            })
            // .emoji(ReactionType::Unicode("🔁".to_owned()))
            .label("🔁")
            .custom_id("repeat")
            // .disabled(true)
        });

        // autoplay
        ar.create_button(|b| {
            b.style(if settings.autoplay {
                serenity::model::application::component::ButtonStyle::Primary
            } else {
                serenity::model::application::component::ButtonStyle::Secondary
            })
            // .emoji(ReactionType::Unicode("🎲".to_owned()))
            .custom_id("autoplay")
            .label("🎲")
            // .disabled(true)
        });

        ars.push(ar);

        ars
    }

    // fn is_different_enough(new: &str, old: &str, threshold: usize) -> bool {
    //     let old = Self::filter_bar_emojis(old);
    //     let new = Self::filter_bar_emojis(new);
    //     if old.len() != new.len() {
    //         return true;
    //     }
    //     let mut diff = 0;
    //     for (new_char, old_char) in new.chars().zip(old.chars()) {
    //         if new_char != old_char {
    //             diff += 1;
    //         }
    //     }
    //     diff >= threshold
    // }
    fn filter_bar_emojis(string: &str) -> String {
        // bar emojis are
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
    pub author: String,
    pub author_id: String,
    pub channel_id: ChannelId,
    pub channel_name: Option<String>,
    pub content: String,
    pub timestamp: serenity::model::timestamp::Timestamp,
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
            author: String::new(),
            author_id: String::from("Announcement"),
            channel_id: msg.channel_id,
            channel_name: None,
            timestamp: msg.timestamp,
            content: text.clone(),
            tts_audio_handle: Some(Self::audio_handle(text, voice.clone())),
        }
    }
    pub async fn message(ctx: &Context, msg: &Message, voice: &TTSVoice) -> Result<Self, Error> {
        let safecontent = msg.content_safe(&ctx.cache);
        let finder = linkify::LinkFinder::new();
        let links: Vec<_> = finder.links(&safecontent).map(|l| l.as_str()).collect();

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

        let channelname = match msg.channel(&ctx).await.unwrap() {
            serenity::model::prelude::Channel::Guild(channel) => channel.name,
            serenity::model::prelude::Channel::Private(_) => String::from("Private"),
            serenity::model::prelude::Channel::Category(_) => String::from("Category"),
            _ => String::from("Unknown"),
        };

        Ok(Self {
            author: msg.author.name.clone(),
            author_id: msg.author.id.to_string(),
            channel_name: Some(channelname),
            channel_id: msg.channel_id,
            timestamp: msg.timestamp,
            content: filteredcontent.clone(),
            tts_audio_handle: Some(Self::audio_handle(filteredcontent, voice.clone())),
        })
    }

    pub fn audio_handle(
        text: String,
        voice: TTSVoice,
    ) -> tokio::task::JoinHandle<Result<Video, anyhow::Error>> {
        tokio::task::spawn(async move {
            let key = crate::youtube::get_access_token().await.unwrap();
            Ok(
                match crate::youtube::get_tts(text, key, Some(voice)).await? {
                    VideoType::Disk(v) => v,
                    VideoType::Url(_) => {
                        return Err(anyhow::anyhow!("Expected Disk video, got Url video"))
                    }
                },
            )
        })
    }
}

// async fn get_mutual_voice_channel(
//     ctx: &Context,
//     interaction: &ApplicationCommandInteraction,
// ) -> Option<(bool, ChannelId)> {
//     // let guild_id = interaction.guild_id.unwrap();
//     // let gvs;
//     // {
//     //     let data_read = ctx.data.read().await;
//     //     let voice_states = data_read.get::<VoiceData>().unwrap().lock().await;
//     //     if let Some(this) = voice_states.get(&guild_id) {
//     //         gvs = this.clone();
//     //     } else {
//     //         interaction
//     //             .edit_original_interaction_response(&ctx.http, |response| {
//     //                 response.content("You need to be in a voice channel to use this command")
//     //             })
//     //             .await
//     //             .unwrap();
//     //         return None;
//     //     }
//     // }
//     // let bot_id = ctx.cache.current_user_id();

//     // if let Some(uvs) = gvs.iter().find(|vs| {
//     //     vs.user_id == interaction.member.as_ref().unwrap().user.id && vs.channel_id.is_some()
//     // }) {
//     //     if uvs.channel_id.is_some() {
//     //         if let Some(bvs) = gvs
//     //             .iter()
//     //             .find(|vs| vs.user_id == bot_id && vs.channel_id.is_some())
//     //         {
//     //             if bvs.channel_id != uvs.channel_id {
//     //                 interaction
//     //                     .edit_original_interaction_response(&ctx.http, |response| response.content("You need to be in the same voice channel as the bot to use this command"))
//     //                     .await
//     //                     .unwrap();
//     //                 None
//     //             } else {
//     //                 uvs.channel_id.map(|id| (false, id))
//     //             }
//     //         } else {
//     //             uvs.channel_id.map(|channel_id| (true, channel_id))
//     //         }
//     //     } else {
//     //         // println!("User is not in a voice CHANNEL");
//     //         interaction
//     //             .edit_original_interaction_response(&ctx.http, |response| {
//     //                 response.content("You need to be in a voice channel to use this command")
//     //             })
//     //             .await
//     //             .unwrap();
//     //         None
//     //     }
//     // } else {
//     //     // println!("User is not in a voice channel");
//     //     interaction
//     //         .edit_original_interaction_response(&ctx.http, |response| {
//     //             response.content("You need to be in a voice channel to use this command")
//     //         })
//     //         .await
//     //         .unwrap();
//     //     None
//     // }

//     if let Some(guild_id) = interaction.guild_id {
//         todo!()
//     } else {
//         interaction
//     }

// }
