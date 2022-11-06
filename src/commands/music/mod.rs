pub mod loopit;
pub mod mainloop;
pub mod pause;
pub mod play;
pub mod remove;
pub mod resume;
pub mod shuffle;
pub mod skip;
pub mod stop;
pub mod volume;

use serenity::http::Http;
use serenity::model::prelude::interaction::application_command::ApplicationCommandInteraction;

use serenity::model::prelude::{ChannelId, GuildId, Message};
use serenity::model::voice::VoiceState;
use serenity::prelude::Mutex;
use tokio::time::Instant;

use std::collections::HashMap;

use std::sync::Arc;

use anyhow::Error;

use serenity::futures::channel::mpsc;

use serenity::prelude::{Context, TypeMapKey};

use crate::video::Video;
use crate::youtube::VideoInfo;

// create the struct for holding the promises for audio playback
pub struct AudioHandler;

impl TypeMapKey for AudioHandler {
    type Value = Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>;
}

pub struct AudioCommandHandler;

impl TypeMapKey for AudioCommandHandler {
    type Value = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<(mpsc::UnboundedSender<String>, AudioPromiseCommand)>>>>;
}

pub struct VoiceData;

impl TypeMapKey for VoiceData {
    type Value = Arc<Mutex<HashMap<GuildId, Vec<VoiceState>>>>;
}

#[derive(Debug, Clone)]
pub enum AudioPromiseCommand {
    Play(Vec<MetaVideo>),
    Stop,
    Pause,
    Resume,
    Skip,
    Volume(f32),
    Remove(usize),
    Loop(bool),
    Shuffle(bool),
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
    pub ttsmsg: Option<Video>,
}

impl MetaVideo {
    pub fn delete(&mut self) -> Result<(), Error> {
        match self.video {
            VideoType::Disk(ref mut video) => video.delete(),
            _ => Ok(()),
        }?;
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
    guild_id: GuildId,
    channel_id: ChannelId,
    message: Option<Message>,
    last_edit: Instant,
    edit_delay: u128,
}
#[allow(dead_code)]
impl MessageReference {
    fn new(http: Arc<Http>, guild_id: GuildId, channel_id: ChannelId, message: Message) -> Self {
        Self {
            http,
            guild_id,
            channel_id,
            message: Some(message),
            last_edit: Instant::now(),
            edit_delay: 1500,
        }
    }
    async fn update(&mut self, content: &str) -> Result<(), Error> {
        let Some(message) = self.message.as_mut() else {
            return Err(anyhow::anyhow!("Message is None"));
        };

        let (orig_content, new_content) = (message.content.as_str().trim(), content.trim());
        let diff = Self::is_different_enough(new_content, orig_content, 3);

        if new_content != orig_content && ((self.last_edit.elapsed().as_millis() > self.edit_delay) || diff) {
            message.edit(&self.http, |m| m.content(new_content)).await?;

            self.last_edit = Instant::now();
        }
        Ok(())
    }
    async fn send_new(&mut self) -> Result<(), Error> {
        let content = if let Some(msg) = self.message.as_ref() { msg.content.clone() } else { String::from("Loading...") };
        let message = self.channel_id.send_message(&self.http, |m| m.content(content)).await?;
        self.message = Some(message);
        Ok(())
    }
    async fn delete(&mut self) -> Result<(), Error> {
        let Some(message) = self.message.as_mut() else {
            return Err(anyhow::anyhow!("Message is None"));
        };
        message.delete(&self.http).await?;
        self.message = None;
        Ok(())
    }
    fn is_different_enough(new: &str, old: &str, threshold: usize) -> bool {
        if old.len() != new.len() {
            return true;
        }
        let mut diff = 0;
        for (new_char, old_char) in new.chars().zip(old.chars()) {
            if new_char != old_char {
                diff += 1;
            }
        }
        diff >= threshold
    }
}

async fn get_mutual_voice_channel(ctx: &Context, interaction: &ApplicationCommandInteraction) -> Option<(bool, ChannelId)> {
    let guild_id = interaction.guild_id.unwrap();
    let gvs;
    {
        let data_read = ctx.data.read().await;
        let voice_states = data_read.get::<VoiceData>().unwrap().lock().await;
        if let Some(this) = voice_states.get(&guild_id) {
            gvs = this.clone();
        } else {
            interaction
                .edit_original_interaction_response(&ctx.http, |response| response.content("You need to be in a voice channel to use this command"))
                .await
                .unwrap();
            return None;
        }
    }
    let bot_id = ctx.cache.current_user_id();

    if let Some(uvs) = gvs.iter().find(|vs| vs.user_id == interaction.member.as_ref().unwrap().user.id && vs.channel_id.is_some()) {
        if uvs.channel_id.is_some() {
            if let Some(bvs) = gvs.iter().find(|vs| vs.user_id == bot_id && vs.channel_id.is_some()) {
                if bvs.channel_id != uvs.channel_id {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |response| response.content("You need to be in the same voice channel as the bot to use this command"))
                        .await
                        .unwrap();
                    None
                } else {
                    uvs.channel_id.map(|id| (false, id))
                }
            } else {
                uvs.channel_id.map(|channel_id| (true, channel_id))
            }
        } else {
            println!("User is not in a voice CHANNEL");
            interaction
                .edit_original_interaction_response(&ctx.http, |response| response.content("You need to be in a voice channel to use this command"))
                .await
                .unwrap();
            None
        }
    } else {
        println!("User is not in a voice channel");
        interaction
            .edit_original_interaction_response(&ctx.http, |response| response.content("You need to be in a voice channel to use this command"))
            .await
            .unwrap();
        None
    }
}
