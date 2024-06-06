use anyhow::Result;
use serenity::{
    all::{
        ChannelId, CommandInteraction, EditInteractionResponse, GuildId, Http, ModalInteraction,
        UserId,
    },
    prelude::TypeMapKey,
};
use std::{collections::HashMap, fmt::Display, sync::Arc};
use tokio::sync::{mpsc, oneshot, RwLock};
use crate::video::MetaVideo;
pub struct AudioCommandHandler;
impl TypeMapKey for AudioCommandHandler {
    type Value = Arc<RwLock<HashMap<ChannelId, SenderAndGuildId>>>;
}
pub struct SenderAndGuildId {
    sender: mpsc::UnboundedSender<(oneshot::Sender<Arc<str>>, AudioPromiseCommand)>,
    pub guild_id: GuildId,
}
impl SenderAndGuildId {
    pub fn new(
        sender: mpsc::UnboundedSender<(oneshot::Sender<Arc<str>>, AudioPromiseCommand)>,
        guild_id: GuildId,
    ) -> Self {
        Self { sender, guild_id }
    }
    pub fn send(&self, command: (oneshot::Sender<Arc<str>>, AudioPromiseCommand)) -> Result<()> {
        Ok(self.sender.send(command)?)
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

    Volume(SpecificVolume),
    // SpecificVolume(SpecificVolume),
    SetBitrate(OrAuto),

    Skip,
    Remove(usize),

    MetaCommand(MetaCommand),
    // Consent { user_id: UserId, consent: bool },
}
#[derive(Debug, Clone)]
pub enum MetaCommand {
    RetrieveLog(mpsc::Sender<Vec<String>>),
    UserConnect(UserId),
    ChangeDefaultRadioVolume(f32),
    ChangeDefaultSongVolume(f32),
    ChangeReadTitles(bool),
    ChangeRadioAudioUrl(Arc<str>),
    ChangeRadioDataUrl(Arc<str>),
    ResetCustomRadioData,
}
pub enum GenericInteraction<'a> {
    Command(&'a CommandInteraction),
    Modal(&'a ModalInteraction),
}
impl<'a> GenericInteraction<'a> {
    pub async fn edit_response(
        &self,
        http: &Http,
        response: EditInteractionResponse,
    ) -> Result<()> {
        match self {
            Self::Command(interaction) => {
                interaction.edit_response(http, response).await?;
            }
            Self::Modal(interaction) => {
                interaction.edit_response(http, response).await?;
            }
        }
        Ok(())
    }
}
impl<'a> From<&'a CommandInteraction> for GenericInteraction<'a> {
    fn from(interaction: &'a CommandInteraction) -> Self {
        Self::Command(interaction)
    }
}
impl<'a> From<&'a ModalInteraction> for GenericInteraction<'a> {
    fn from(interaction: &'a ModalInteraction) -> Self {
        Self::Modal(interaction)
    }
}
#[derive(Debug, Clone, Copy)]
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
    Current(f32),
    SongVolume(f32),
    RadioVolume(f32),
}
