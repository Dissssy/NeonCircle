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

#[cfg(feature = "download")]
use crate::video::Video;
#[cfg(not(feature = "download"))]
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
    #[cfg(feature = "download")]
    Play(Vec<Video>),
    #[cfg(not(feature = "download"))]
    Play(VideoInfo),
    Stop,
    Pause,
    Resume,
    Skip,
    Volume(f32),
    // Seek((MessageReference, u64)),
    Remove(usize),
    Loop(bool),
    Shuffle(bool),
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
        // check if the message content actually needs updated
        let (orig_content, new_content) = (message.content.as_str().trim(), content.trim());
        let diff = Self::is_different_enough(new_content, orig_content, 3);
        // println!("{}\n{}\n{}", orig_content, new_content, diff);
        if new_content != orig_content && ((self.last_edit.elapsed().as_millis() > self.edit_delay) || diff) {
            // update the message
            message.edit(&self.http, |m| m.content(new_content)).await?;
            // update the last edit time
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
    // if the user is not in a voice channel, return
    if let Some(uvs) = gvs.iter().find(|vs| vs.user_id == interaction.member.as_ref().unwrap().user.id && vs.channel_id.is_some()) {
        if uvs.channel_id.is_some() {
            // if the bot is in a voice channel, check if it is the same as the user
            if let Some(bvs) = gvs.iter().find(|vs| vs.user_id == bot_id && vs.channel_id.is_some()) {
                // if the bot is in a different channel, return
                if bvs.channel_id != uvs.channel_id {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |response| response.content("You need to be in the same voice channel as the bot to use this command"))
                        .await
                        .unwrap();
                    None
                } else {
                    // if the bot is in the same channel, return the channel id
                    uvs.channel_id.map(|id| (false, id))
                }
            } else {
                // println!("Bot is not in a voice channel");
                // interaction
                //     .edit_original_interaction_response(&ctx.http, |response| response.content("You need to be in a voice channel to use this command"))
                //     .await
                //     .unwrap();
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
