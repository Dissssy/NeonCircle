#![feature(if_let_guard, try_blocks, duration_millis_float)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    // clippy::arithmetic_side_effects
)]
mod commands;
mod global_data;
mod radio;
mod sam;
mod video;
mod youtube;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
mod context_menu;
mod voice_events;
use crate::commands::music::{AudioCommandHandler, AudioPromiseCommand, OrToggle};
use anyhow::Result;
use global_data::VoiceAction;
use serde::{Deserialize, Serialize};
use serenity::{
    all::*,
    futures::{stream::FuturesUnordered, StreamExt},
};
use songbird::SerenityInit;
use tokio::sync::{mpsc, oneshot, RwLock};
struct PlanetHandler {
    commands: Vec<Box<dyn CommandTrait>>,
    playing: String,
}
impl PlanetHandler {
    fn new(commands: Vec<Box<dyn CommandTrait>>, activity: String) -> Self {
        Self {
            commands,
            playing: activity,
        }
    }
}
lazy_static::lazy_static! {
    static ref WHITELIST: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new({
        let file = match std::fs::File::open(Config::get().whitelist_path) {
            Ok(f) => f,
            Err(e) => panic!("Failed to open whitelist file: {}", e)
        };
        match serde_json::from_reader(file) {
            Ok(r) => r,
            Err(e) => panic!("Failed to read whitelist file: {}", e)
        }
    }));
    static ref WEB_CLIENT: reqwest::Client = reqwest::Client::new();
    static ref BOTS: BotsConfig = {
        let file = match std::fs::File::open(Config::get().bots_config_path) {
            Ok(f) => f,
            Err(e) => panic!("Failed to open bot config file: {}", e)
        };
        match serde_json::from_reader(file) {
            Ok(r) => r,
            Err(e) => panic!("Failed to read bot config file: {}", e)
        }
    };
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotsConfig {
    pub planet: BotConfig,
    pub satellites: Vec<BotConfig>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub token: String,
    pub playing: String,
}
#[async_trait]
pub trait CommandTrait
where
    Self: Send + Sync,
{
    fn register_command(&self) -> Option<CreateCommand> {
        None
    }
    fn command_name(&self) -> &str {
        ""
    }
    #[allow(unused_variables)]
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        log::error!("Run not implemented for {}", self.command_name());
        Ok(())
    }
    fn modal_names(&self) -> &'static [&'static str] {
        &[]
    }
    #[allow(unused_variables)]
    async fn run_modal(&self, ctx: &Context, interaction: &ModalInteraction) -> Result<()> {
        log::error!(
            "Modal not implemented for {}",
            std::any::type_name::<Self>()
        );
        Ok(())
    }
    #[allow(unused_variables)]
    async fn autocomplete(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        log::error!(
            "Autocomplete not implemented for {}",
            std::any::type_name::<Self>()
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserSafe {
    pub id: String,
}
#[async_trait]
impl EventHandler for PlanetHandler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match &interaction {
            Interaction::Command(rawcommand) => {
                let command_name = rawcommand.data.name.clone();
                let command = self
                    .commands
                    .iter()
                    .find(|c| c.command_name() == command_name);
                if let Some(command) = command {
                    if let Err(e) = command.run(&ctx, rawcommand).await {
                        log::error!("Failed to run command: {}", e);
                    }
                } else {
                    log::warn!("Command not found: {}", command_name);
                }
            }
            Interaction::Autocomplete(autocomplete) => {
                let commandn = autocomplete.data.name.clone();
                let command = self.commands.iter().find(|c| c.command_name() == commandn);
                if let Some(command) = command {
                    let r = command.autocomplete(&ctx, autocomplete).await;
                    if r.is_err() {}
                } else {
                    log::warn!("Command not found: {}", commandn);
                }
            }
            Interaction::Ping(p) => {
                log::info!("Ping received: {:?}", p);
            }
            Interaction::Component(mci) => {
                let guild_id = match mci.guild_id {
                    Some(id) => id,
                    None => {
                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("This can only be used in a server").ephemeral(true))).await {
                            log::error!("Failed to send response: {:?}", e);
                        }
                        return;
                    }
                };
                let next_step = match global_data::mutual_channel(&guild_id, &mci.user.id).await {
                    Ok(n) => n,
                    Err(e) => {
                        log::error!("Failed to get voice data: {:?}", e);
                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to get voice data").ephemeral(true))).await {
                            log::error!("Failed to send response: {:?}", e);
                        }
                        return;
                    }
                };
                match next_step.action {
                    VoiceAction::SatelliteInVcWithUser(channel, _ctx) => {
                        if channel != mci.channel_id {
                            if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Wrong control panel, silly!").ephemeral(true))).await {
                                log::error!("Failed to send response: {:?}", e);
                            }
                            return;
                        }
                        let cmd = match mci.data.kind {
                            ComponentInteractionDataKind::Button => mci.data.custom_id.as_str(),
                            ComponentInteractionDataKind::StringSelect { ref values } => {
                                match values.first() {
                                    Some(v) => v.as_str(),
                                    None => {
                                        log::error!("No values in string select");
                                        return;
                                    }
                                }
                            }
                            _ => {
                                log::error!("Unknown component interaction kind");
                                return;
                            }
                        };
                        if cmd == "controls" {
                            if let Err(e) = mci
                                .create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
                                .await
                            {
                                log::error!("Failed to send response: {:?}", e);
                            };
                            return;
                        }
                        match cmd {
                            original_command if ["pause", "skip", "stop", "looped", "shuffle", "repeat", "autoplay", "read_titles"].iter().any(|a| *a == original_command) => {
                                let guild_id = match mci.guild_id {
                                    Some(id) => id,
                                    None => {
                                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("This can only be used in a server").ephemeral(true))).await {
                                            log::error!("Failed to send response: {:?}", e);
                                        }
                                        return;
                                    }
                                };
        
                                if let Some(member) = mci.member.as_ref() {
                                    let next_step = match global_data::mutual_channel(&guild_id, &member.user.id).await {
                                        Ok(n) => n,
                                        Err(e) => {
                                            log::error!("Failed to get voice data: {:?}", e);
                                            if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to get voice data").ephemeral(true))).await {
                                                log::error!("Failed to send response: {:?}", e);
                                            }
                                            return;
                                        }
                                    };
        
                                    if let VoiceAction::SatelliteInVcWithUser(_channel, _ctx) = next_step.action {
                                        let audio_command_handler = match ctx.data.read().await.get::<AudioCommandHandler>() {
                                            Some(a) => Arc::clone(a),
                                            None => {
                                                log::error!("Expected AudioCommandHandler in TypeMap");
                                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to get audio command handler").ephemeral(true))).await {
                                                    log::error!("Failed to send response: {:?}", e);
                                                }
                                                return;
                                            }
                                        };
        
                                        let mut audio_command_handler = audio_command_handler.write().await;
        
                                        if let Some(tx) = audio_command_handler.get_mut(&channel) {
                                            let (rtx, rrx) = oneshot::channel::<String>();
                                            if let Err(e) = tx.send((
                                                rtx,
                                                match original_command {
                                                    "pause" => AudioPromiseCommand::Paused(OrToggle::Toggle),
                                                    "skip" => AudioPromiseCommand::Skip,
                                                    "stop" => AudioPromiseCommand::Stop(None),
                                                    "looped" => AudioPromiseCommand::Loop(OrToggle::Toggle),
                                                    "shuffle" => AudioPromiseCommand::Shuffle(OrToggle::Toggle),
                                                    "repeat" => AudioPromiseCommand::Repeat(OrToggle::Toggle),
                                                    "autoplay" => AudioPromiseCommand::Autoplay(OrToggle::Toggle),
                                                    "read_titles" => AudioPromiseCommand::ReadTitles(OrToggle::Toggle),
                                                    uh => {
                                                        log::error!("Unknown command: {}", uh);
                                                        return;
                                                    }
                                                },
                                            )) {
                                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(format!("Failed to issue command for {} ERR {}", original_command, e)).ephemeral(true))).await {
                                                    log::error!("Failed to send response: {}", e);
                                                }
                                                return;
                                            }
        
                                            if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Acknowledge).await {
                                                log::error!("Failed to send response: {}", e);
                                            }
                                            let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), rrx).await;
        
                                            match timeout {
                                                Ok(Ok(_msg)) => {
                                                    return;
                                                }
                                                Ok(Err(e)) => {
                                                    log::error!("Failed to issue command for {} ERR: {}", original_command, e);
                                                }
                                                Err(e) => {
                                                    log::error!("Failed to issue command for {} ERR: {}", original_command, e);
                                                }
                                            }
        
                                            if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(format!("Failed to issue command for {}", original_command)).ephemeral(true))).await {
                                                log::error!("Failed to send response: {}", e);
                                            }
                                            return;
                                        }
        
                                        log::trace!("{}", _channel);
                                    } else {
                                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Get on in here, enjoy the tunes!").ephemeral(true))).await {
                                            log::error!("Failed to send response: {}", e);
                                        }
                                        return;
                                    }
                                }
                                else {
                                    log::error!("Failed to get voice data");
                                    if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to get voice data").ephemeral(true))).await {
                                        log::error!("Failed to send response: {}", e);
                                    }
                                }
                            }
                            raw if ["volume", "radiovolume"].iter().any(|a| *a == raw) => {
                                let guild_id = match mci.guild_id {
                                    Some(id) => id,
                                    None => {
                                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("This can only be used in a server").ephemeral(true))).await {
                                            log::error!("Failed to send response: {}", e);
                                        }
                                        return;
                                    }
                                };
        
                                if let Some(member) = mci.member.as_ref() {
                                    let next_step = match global_data::mutual_channel(&guild_id, &member.user.id).await {
                                        Ok(n) => n,
                                        Err(e) => {
                                            log::error!("Failed to get voice data: {:?}", e);
                                            if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to get voice data").ephemeral(true))).await {
                                                log::error!("Failed to send response: {:?}", e);
                                            }
                                            return;
                                        }
                                    };
        
                                    if let VoiceAction::SatelliteInVcWithUser(_channel, _ctx) = next_step.action {
                                        if let Err(e) = mci
                                            .create_response(
                                                &ctx.http,
                                                CreateInteractionResponse::Modal(
                                                    CreateModal::new(
                                                        raw,
                                                        match raw {
                                                            "volume" => "Volume",
                                                            "radiovolume" => "Radio Volume",
                                                            _ => {
                                                                log::error!("Unknown command: {}", raw);
                                                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(format!("Unknown command: {}", raw)).ephemeral(true))).await {
                                                                    log::error!("Failed to send response: {}", e);
                                                                }
                                                                return;
                                                            }
                                                        },
                                                    )
                                                    .components(vec![CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Short, "%", "volume").value("").placeholder("0 - 100").required(true))]),
                                                ),
                                            )
                                            .await
                                        {
                                            log::error!("Failed to send response: {}", e);
                                        }
                                        return;
                                    } else {
                                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Get on in here, enjoy the tunes!").ephemeral(true))).await {
                                            log::error!("Failed to send response: {}", e);
                                        }
                                        return;
                                    }
                                } else {
                                    log::error!("Failed to get voice data");
                                    if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to get voice data").ephemeral(true))).await {
                                        log::error!("Failed to send response: {}", e);
                                    }
                                }
                            }
                            "bitrate" => {
                                let guild_id = match mci.guild_id {
                                    Some(id) => id,
                                    None => {
                                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("This can only be used in a server").ephemeral(true))).await {
                                            log::error!("Failed to send response: {}", e);
                                        }
                                        return;
                                    }
                                };
        
                                if let Some(member) = mci.member.as_ref() {
                                    let next_step = match global_data::mutual_channel(&guild_id, &member.user.id).await {
                                        Ok(n) => n,
                                        Err(e) => {
                                            log::error!("Failed to get voice data: {:?}", e);
                                            if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to get voice data").ephemeral(true))).await {
                                                log::error!("Failed to send response: {:?}", e);
                                            }
                                            return;
                                        }
                                    };
        
                                    if let VoiceAction::SatelliteInVcWithUser(_channel, _ctx) = next_step.action {
                                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Modal(CreateModal::new("bitrate", "Bitrate").components(vec![CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Short, "bps", "bitrate").placeholder("512 - 512000, left blank for auto").required(false))]))).await {
                                            log::error!("Failed to send response: {}", e);
                                        }
                                        return;
                                    } else {
                                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Get on in here, enjoy the tunes!").ephemeral(true))).await {
                                            log::error!("Failed to send response: {}", e);
                                        }
                                        return;
                                    }
                                } else {
                                    log::error!("Failed to get voice data");
                                    if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to get voice data").ephemeral(true))).await {
                                        log::error!("Failed to send response: {}", e);
                                    }
                                }
                            }
                            "log" => {
                                let guild_id = match mci.guild_id {
                                    Some(id) => id,
                                    None => {
                                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("This can only be used in a server").ephemeral(true))).await {
                                            log::error!("Failed to send response: {}", e);
                                        }
                                        return;
                                    }
                                };
        
                                if let Some(member) = mci.member.as_ref() {
                                    let next_step = match global_data::mutual_channel(&guild_id, &member.user.id).await {
                                        Ok(n) => n,
                                        Err(e) => {
                                            log::error!("Failed to get voice data: {:?}", e);
                                            if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to get voice data").ephemeral(true))).await {
                                                log::error!("Failed to send response: {:?}", e);
                                            }
                                            return;
                                        }
                                    };
        
                                    if let VoiceAction::SatelliteInVcWithUser(_channel, _ctx) = next_step.action {
                                        let audio_command_handler = match ctx.data.read().await.get::<AudioCommandHandler>() {
                                            Some(a) => Arc::clone(a),
                                            None => {
                                                log::error!("Expected AudioCommandHandler in TypeMap");
                                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to get audio command handler").ephemeral(true))).await {
                                                    log::error!("Failed to send response: {}", e);
                                                }
                                                return;
                                            }
                                        };
        
                                        let mut audio_command_handler = audio_command_handler.write().await;
        
                                        if let Some(tx) = audio_command_handler.get_mut(&channel) {
                                            let (rtx, rrx) = oneshot::channel::<String>();
                                            let (realrtx, mut realrrx) = mpsc::channel::<Vec<String>>(1);
                                            if let Err(e) = tx.send((rtx, AudioPromiseCommand::RetrieveLog(realrtx))) {
                                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(format!("Failed to issue command for `log` ERR {}", e)).ephemeral(true))).await {
                                                    log::error!("Failed to send response: {}", e);
                                                }
                                                return;
                                            }
        
                                            let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), rrx).await;
        
                                            match timeout {
                                                Ok(Ok(_)) => {
                                                    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), realrrx.recv()).await;
        
                                                    match timeout {
                                                        Ok(Some(log)) => {
                                                            if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Modal(CreateModal::new("log", "Log (Submitting this does nothing)").components(log.iter().enumerate().map(|(i, log)| CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Paragraph, "Log", format!("log{}", i)).value(log))).collect()))).await {
                                                                log::error!("Failed to send response: {}", e);
                                                            }
                                                            return;
                                                        }
                                                        Ok(None) => {
                                                            log::error!("Failed to get log");
                                                        }
                                                        Err(e) => {
                                                            log::error!("Failed to get log: {}", e);
                                                        }
                                                    }
                                                }
                                                Ok(Err(e)) => {
                                                    log::error!("Failed to issue command for `log` ERR: {}", e);
                                                }
                                                Err(e) => {
                                                    log::error!("Failed to issue command for `log` ERR: {}", e);
                                                }
                                            }
        
                                            if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to issue command for `log`").ephemeral(true))).await {
                                                log::error!("Failed to send response: {}", e);
                                            }
                                            return;
                                        }
                                    } else {
                                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Get on in here, enjoy the tunes!").ephemeral(true))).await {
                                            log::error!("Failed to send response: {}", e);
                                        }
                                        return;
                                    }
                                } else {
                                    log::error!("Failed to get voice data");
                                    if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to get voice data").ephemeral(true))).await {
                                        log::error!("Failed to send response: {}", e);
                                    }
                                }
                            }
                            p => {
                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Modal(CreateModal::new("missing_feature_feedback", "Feedback").components(vec![CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Paragraph, format!("How should clicking `{}` work?", p), "feedback").placeholder("Read the discord documentation and figure out what i can ACTUALLY do. I can't think of anything.").required(true))]))).await {
                                    log::error!("Failed to send response: {}", e);
                                }
                            }
                        }
                    }
                    VoiceAction::UserNotConnected => {
                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Get on in here, enjoy the tunes!").ephemeral(true))).await {
                            log::error!("Failed to send response: {}", e);
                        }
                    }
                    _ => {
                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(
                            "https://zip.p51.nl/u/2784372e-21fa-4402-ad2f-0ee1cb719d0a.png"
                        ).ephemeral(true))).await {
                            log::error!("Failed to send response: {}", e);
                        }
                    }
                }
            }
            Interaction::Modal(p) => {
                let command = self
                    .commands
                    .iter()
                    .find(|c| c.modal_names().contains(&p.data.custom_id.as_str()));
                if let Some(command) = command {
                    if let Err(e) = command.run_modal(&ctx, p).await {
                        log::error!("Failed to run modal: {}", e);
                    }
                } else {
                    match p.data.custom_id.as_str() {
                        "missing_button_feedback" => {
                            let i = match p
                                .data
                                .components
                                .first()
                                .and_then(|ar| ar.components.first())
                            {
                                Some(ActionRowComponent::InputText(feedback)) => feedback,
                                Some(_) => {
                                    log::error!("Invalid components in feedback modal");
                                    return;
                                }
                                None => {
                                    log::error!("No components in feedback modal");
                                    return;
                                }
                            };
                            let mut content = "Thanks for the feedback!".to_owned();
                            let feedback = format!(
                                "User thinks `{}` should\n```\n{}```",
                                i.custom_id,
                                match i.value {
                                    Some(ref v) => v,
                                    None => {
                                        log::error!("No value in feedback modal");
                                        return;
                                    }
                                }
                            );
                            match ctx.http.get_user(UserId::new(156533151198478336)).await {
                                Ok(user) => {
                                    if let Err(e) = user
                                        .dm(&ctx.http, CreateMessage::default().content(&feedback))
                                        .await
                                    {
                                        log::error!("Failed to send feedback to developer: {}", e);
                                        content = format!("{}{}\n{}\n{}\n{}", content, "Unfortunately, I failed to send your feedback to the developer.", "If you're able to, be sure to send it to him yourself!", "He's <@156533151198478336> (monkey_d._issy)\n\nHere's a copy if you need it.", feedback);
                                    }
                                }
                                Err(e) => {
                                    log::error!("Failed to get user: {}", e);
                                    content = format!("{}{}\n{}\n{}\n{}", content, "Unfortunately, I failed to send your feedback to the developer.", "If you're able to, be sure to send it to him yourself!", "He's <@156533151198478336> (monkey_d._issy)\n\nHere's a copy if you need it.", feedback);
                                }
                            }
                            if let Err(e) = p
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content(content)
                                            .ephemeral(true),
                                    ),
                                )
                                .await
                            {
                                log::error!("Failed to send response: {}", e);
                            }
                        }
                        "log" => {
                            if let Err(e) = p
                                .create_response(&ctx.http, CreateInteractionResponse::Acknowledge)
                                .await
                            {
                                log::error!("Failed to send response: {}", e);
                            }
                        }
                        _ => {
                            log::error!("Unknown modal: {}", p.data.custom_id);
                        }
                    }
                }
            }
            _ => {
                log::info!("Unhandled interaction: {:?}", interaction);
            }
        }
    }
    async fn ready(&self, ctx: Context, ready: Ready) {
        log::info!("Initializing Voice Data");
        if let Err(e) = global_data::initialize_planet(ctx.clone()).await {
            log::error!("Failed to initialize planet: {}", e);
        }
        log::info!("Refreshing users");
        // let voicedata = match ctx.data.read().await.get::<VoiceData>() {
        //     Some(v) => Arc::clone(v),
        //     None => {
        //         log::error!("Expected VoiceData in TypeMap");
        //         return;
        //     }
        // };
        let mut lazy_users = FuturesUnordered::new();
        let mut lazy_voicedata = FuturesUnordered::new();
        for guild in ready.guilds {
            match ctx.http.get_guild(guild.id).await {
                Ok(guild) => {
                    lazy_users.push({
                        let guild = guild.clone();
                        let ctx = ctx.clone();
                        async move {
                            let mut users = Vec::new();
                            let mut after = None;
                            loop {
                                for member in match guild.members(&ctx.http, None, after).await {
                                    Ok(members) => {
                                        if let Some(last) = members.last() {
                                            after = Some(last.user.id);
                                        } else {
                                            break;
                                        }
                                        members
                                    }
                                    Err(e) => {
                                        log::error!("Error getting members: {e}");
                                        Vec::new()
                                    }
                                } {
                                    let id = member.user.id.get().to_string();
                                    if !users.contains(&id) {
                                        users.push(id);
                                    }
                                }
                            }
                            (users, format!("{} ({})", guild.name, guild.id))
                        }
                    });
                    lazy_voicedata.push({
                        async move {
                            global_data::lazy_refresh_guild(guild.id)
                                .await
                                .map(|d| (d, format!("{} ({})", guild.name, guild.id)))
                        }
                    });
                }
                Err(e) => {
                    log::error!("Error getting guild: {e}");
                }
            }
        }
        let mut finalusers = Vec::new();
        while let Some((users, guildinfo)) = lazy_users.next().await {
            log::info!("Retrieved {} users from {}", users.len(), guildinfo);
            for user in users {
                let user = UserSafe { id: user };
                if !finalusers.contains(&user) {
                    finalusers.push(user);
                }
            }
        }
        let mut final_voice_data = Vec::new();
        while let Some(result) = lazy_voicedata.next().await {
            match result {
                Ok((data, logstr)) => {
                    final_voice_data.push(data);
                    log::info!("Refreshed voice data for {}", logstr);
                }
                Err(e) => {
                    log::error!("Failed to refresh voice data: {e}");
                }
            }
        }
        {
            for (guild_id, voice_data) in final_voice_data {
                if let Err(e) = global_data::insert_guild(guild_id, voice_data).await {
                    log::error!("Failed to insert guild: {}", e);
                }
            }
        }
        let mut req = WEB_CLIENT
            .post("http://localhost:16835/api/set/user")
            .json(&finalusers);
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            log::error!("Failed to send users to api {e}. Users might be out of date");
        }
        log::info!("Registering commands");
        if let Err(e) = Command::set_global_commands(
            &ctx.http,
            self.commands
                .iter()
                .flat_map(|command| command.register_command())
                .collect(),
        )
        .await
        {
            log::error!("Failed to register commands: {}", e);
        }
        ctx.set_activity(Some(ActivityData::playing(&self.playing)));
        if let Err(e) = global_data::add_satellite(ctx, 0).await {
            log::error!("Failed to add satellite: {}", e);
        }
        log::info!("Connected as {}", ready.user.name);
    }
    async fn voice_state_update(&self, _ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        // let data = {
        //     let uh = ctx.data.read().await;
        //     match uh.get::<VoiceData>() {
        //         Some(v) => Arc::clone(v),
        //         None => {
        //             log::error!("Expected VoiceData in TypeMap");
        //             return;
        //         }
        //     }
        // };
        {
            // let mut data = data.write().await;
            // data.update(old.clone(), new.clone());
            if let Err(e) = global_data::update_voice(old, new).await {
                log::error!("Failed to update voice data: {}", e);
            }
        }
        // let guild_id = match (old.and_then(|o| o.guild_id), new.guild_id) {
        //     (Some(g), _) => g,
        //     (_, Some(g)) => g,
        //     _ => return,
        // };
        // let leave = {
        //     let mut data = data.write().await;
        //     data.bot_alone(&guild_id)
        // };
        // if !leave {
        //     return;
        // }
        // let audio_command_handler = match ctx.data.read().await.get::<AudioCommandHandler>() {
        //     Some(a) => Arc::clone(a),
        //     None => {
        //         log::error!("Expected AudioCommandHandler in TypeMap");
        //         return;
        //     }
        // };
        // let mut audio_command_handler = audio_command_handler.write().await;
        // if let Some(tx) = audio_command_handler.get_mut(&guild_id.to_string()) {
        //     let (rtx, rrx) = oneshot::channel::<String>();
        //     if let Err(e) = tx.send((rtx, AudioPromiseCommand::Stop(None))) {
        //         log::error!("Failed to send stop command: {}", e);
        //     };
        //     let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), rrx).await;
        //     match timeout {
        //         Ok(Ok(_msg)) => {
        //             return;
        //         }
        //         Ok(Err(e)) => {
        //             log::error!("Failed to issue command for stop ERR: {}", e);
        //         }
        //         Err(e) => {
        //             log::error!("Failed to issue command for stop ERR: {}", e);
        //         }
        //     }
        // }
    }
    async fn message(&self, ctx: Context, new_message: Message) {
        if new_message.author.bot || new_message.content.trim().is_empty() {
            return;
        }
        let guild_id = match new_message.guild_id {
            Some(guild) => guild,
            None => return,
        };
        let em = match commands::music::get_transcribe_channel_handler(&ctx, &guild_id).await {
            Ok(e) => e,
            Err(e) => {
                log::error!("Failed to get transcribe channel handler: {}", e);
                return;
            }
        };
        em.write().await.send_tts(&ctx, &new_message).await;
    }
    async fn resume(&self, ctx: Context, _: ResumedEvent) {
        log::info!("Refreshing users");
        let mut guilds = Vec::new();
        let mut after = None;
        loop {
            for guild in match ctx.http.get_guilds(after.take(), None).await {
                Ok(g) => {
                    if let Some(last) = g.last() {
                        after = Some(GuildPagination::After(last.id));
                    } else {
                        break;
                    }
                    g
                }
                Err(e) => {
                    log::error!("Error getting guilds: {e}");
                    Vec::new()
                }
            } {
                // because of the guild pagination not being copy, we might take it and start over, so ensure there are no duplicate guilds before pushing, if there are, break
                if guilds.iter().any(|g: &GuildInfo| g.id == guild.id) {
                    break;
                }
                guilds.push(guild);
            }
        }
        // let voicedata = match ctx.data.read().await.get::<VoiceData>() {
        //     Some(v) => Arc::clone(v),
        //     None => {
        //         log::error!("Expected VoiceData in TypeMap");
        //         return;
        //     }
        // };
        let mut lazy_users = FuturesUnordered::new();
        let mut lazy_voicedata = FuturesUnordered::new();
        for guild in guilds {
            match ctx.http.get_guild(guild.id).await {
                Ok(guild) => {
                    lazy_users.push({
                        let guild = guild.clone();
                        let ctx = ctx.clone();
                        async move {
                            let mut users = Vec::new();
                            let mut after = None;
                            loop {
                                for member in match guild.members(&ctx.http, None, after).await {
                                    Ok(members) => {
                                        if let Some(last) = members.last() {
                                            after = Some(last.user.id);
                                        } else {
                                            break;
                                        }
                                        members
                                    }
                                    Err(e) => {
                                        log::error!("Error getting members: {e}");
                                        Vec::new()
                                    }
                                } {
                                    let id = member.user.id.get().to_string();
                                    if !users.contains(&id) {
                                        users.push(id);
                                    }
                                }
                            }
                            (users, format!("{} ({})", guild.name, guild.id))
                        }
                    });
                    lazy_voicedata.push({
                        // let voicedata = Arc::clone(&voicedata);
                        // let ctx = ctx.clone();
                        async move {
                            // let mut voicedata = voicedata.write().await;
                            if let Err(e) = crate::global_data::refresh_guild(guild.id).await {
                                log::error!("Failed to refresh guild: {}", e);
                            }
                            format!("{} ({})", guild.name, guild.id)
                        }
                    });
                }
                Err(e) => {
                    log::error!("Error getting guild: {e}");
                }
            }
        }
        let mut finalusers = Vec::new();
        while let Some((users, guildinfo)) = lazy_users.next().await {
            log::info!("Retrieved {} users from {}", users.len(), guildinfo);
            for user in users {
                let user = UserSafe { id: user };
                if !finalusers.contains(&user) {
                    finalusers.push(user);
                }
            }
        }
        while let Some(guildinfo) = lazy_voicedata.next().await {
            log::info!("Refreshed voice data for {}", guildinfo);
        }
        let mut req = WEB_CLIENT
            .post("http://localhost:16835/api/set/user")
            .json(&finalusers);
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            log::error!("Failed to send users to api {e}. Users might be out of date");
        }
    }
    async fn guild_member_addition(&self, _ctx: Context, new_member: Member) {
        let id = new_member.user.id.get().to_string();
        // let mut req = WEB_CLIENT
        //     .post("http://localhost:16834/api/add/user")
        //     .json(&UserSafe { id: id.clone() });
        // if let Some(token) = Config::get().string_api_token {
        //     req = req.bearer_auth(token);
        // }
        // if let Err(e) = req.send().await {
        //     log::error!("Failed to add user to api {e}. Users might be out of date");
        // }
        let mut req = WEB_CLIENT
            .post("http://localhost:16835/api/add/user")
            .json(&UserSafe { id });
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            log::error!("Failed to add user to api {e}. Users might be out of date");
        }
    }
    async fn guild_member_removal(
        &self,
        _ctx: Context,
        _guild_id: GuildId,
        user: User,
        _member_data_if_available: Option<Member>,
    ) {
        let id = user.id.get().to_string();
        // let mut req = WEB_CLIENT
        //     .post("http://localhost:16834/api/remove/user")
        //     .json(&UserSafe { id: id.clone() });
        // if let Some(token) = Config::get().string_api_token {
        //     req = req.bearer_auth(token);
        // }
        // if let Err(e) = req.send().await {
        //     log::error!("Failed to remove user from api {e}. Users might be out of date");
        // }
        let mut req = WEB_CLIENT
            .post("http://localhost:16835/api/remove/user")
            .json(&UserSafe { id });
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            log::error!("Failed to remove user from api {e}. Users might be out of date");
        }
    }
    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: Option<bool>) {
        if is_new.unwrap_or(true) {
            // resync the data for this guild
            // let voicedata = match ctx.data.read().await.get::<VoiceData>() {
            //     Some(v) => Arc::clone(v),
            //     None => {
            //         log::error!("Expected VoiceData in TypeMap");
            //         return;
            //     }
            // };
            // let mut voicedata = voicedata.write().await;
            // if let Err(e) = voicedata.refresh_guild(&ctx, guild.id).await {
            //     log::error!("Failed to refresh guild: {}", e);
            // }
            if let Err(e) = global_data::refresh_guild(guild.id).await {
                log::error!("Failed to refresh guild: {}", e);
            }
            // resync the users for this guild
            let mut users = Vec::new();
            let mut after = None;
            loop {
                for member in match guild.members(&ctx.http, None, after).await {
                    Ok(members) => {
                        if let Some(last) = members.last() {
                            after = Some(last.user.id);
                        } else {
                            break;
                        }
                        members
                    }
                    Err(e) => {
                        log::error!("Error getting members: {e}");
                        Vec::new()
                    }
                } {
                    let id = member.user.id.get().to_string();
                    if !users.contains(&id) {
                        users.push(id);
                    }
                    log::info!("Retrieved {} users from {}", users.len(), guild.name);
                }
            }
            let mut req = WEB_CLIENT
                .post("http://localhost:16835/api/set/user")
                .json(&users);
            if let Some(token) = Config::get().string_api_token {
                req = req.bearer_auth(token);
            }
            if let Err(e) = req.send().await {
                log::error!("Failed to send users to api {e}. Users might be out of date");
            }
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Timed<T> {
    thing: T,
    time: u64,
}
#[tokio::main]
async fn main() {
    env_logger::init();
    global_data::init();
    let cfg = Config::get();
    let mut tmp = cfg.data_path.clone();
    tmp.push("tmp");
    if let Err(e) = std::fs::remove_dir_all(&tmp) {
        log::error!("Failed to remove tmp folder: {:?}", e);
    }
    match std::fs::create_dir_all(&tmp) {
        Ok(_) => {}
        Err(e) => {
            log::error!("Failed to create tmp folder: {:?}", e);
            return;
        }
    }
    let handler = PlanetHandler::new(
        vec![
            Box::new(commands::music::transcribe::Transcribe),
            Box::new(commands::music::repeat::Repeat),
            Box::new(commands::music::loop_queue::Loop),
            Box::new(commands::music::pause::Pause),
            Box::new(commands::music::add::Add),
            Box::new(commands::music::join::Join),
            Box::new(commands::music::setbitrate::SetBitrate),
            Box::new(commands::music::remove::Remove),
            Box::new(commands::music::resume::Resume),
            Box::new(commands::music::shuffle::Shuffle),
            Box::new(commands::music::skip::Skip),
            Box::new(commands::music::stop::Stop),
            Box::new(commands::music::volume::Volume),
            Box::new(commands::music::autoplay::Autoplay),
            Box::new(commands::music::consent::Consent),
            Box::new(commands::embed::Video),
            Box::new(commands::embed::Audio),
            Box::new(commands::john::John),
            Box::new(commands::feedback::Feedback),
        ],
        BOTS.planet.playing.clone(),
    );
    let config = songbird::Config::default()
        .preallocated_tracks(2)
        .decode_mode(songbird::driver::DecodeMode::Decode)
        .crypto_mode(songbird::driver::CryptoMode::Lite);
    let mut client = match Client::builder(&BOTS.planet.token, GatewayIntents::all())
        .register_songbird_from_config(config.clone())
        .event_handler(handler)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to create client: {:?}", e);
            return;
        }
    };
    {
        let mut data = client.data.write().await;
        data.insert::<commands::music::AudioHandler>(Arc::new(RwLock::new(HashMap::new())));
        data.insert::<commands::music::AudioCommandHandler>(Arc::new(RwLock::new(HashMap::new())));
        // data.insert::<commands::music::VoiceData>(Arc::new(RwLock::new(
        //     commands::music::InnerVoiceData::new(client.cache.current_user().id),
        // )));
        data.insert::<commands::music::transcribe::TranscribeData>(Arc::new(RwLock::new(
            HashMap::new(),
        )));
    }
    let (kill_tx, kill_rx) = tokio::sync::broadcast::channel::<()>(1);
    let mut clients = FuturesUnordered::new();
    for (index, bot) in BOTS.satellites.iter().enumerate() {
        let mut client = match Client::builder(&bot.token, GatewayIntents::non_privileged())
            .register_songbird_from_config(config.clone())
            .event_handler(SatelliteHandler::new(bot.playing.clone(), index + 1))
            .await
        {
            Ok(c) => c,
            Err(e) => {
                log::error!("Failed to create client: {:?}", e);
                return;
            }
        };
        let mut kill_rx = kill_rx.resubscribe();
        clients.push(tokio::spawn(async move {
            tokio::select! {
                Err(e) = client.start() => {
                    log::error!("Client error: {:?}", e);
                    Err(e)
                }
                _ = kill_rx.recv() => {
                    log::info!("Killing client");
                    client.shard_manager.shutdown_all().await;
                    Ok(())
                }
            }
        }));
    }
    let mut tick = tokio::time::interval({
        let now = chrono::Local::now();
        let mut next = match chrono::Local::now().date_naive().and_hms_opt(8, 0, 0) {
            Some(v) => v.and_utc(),
            None => {
                log::error!("Failed to get next 8am, did time stop?");
                return;
            }
        };
        if next < now {
            next += chrono::Duration::days(1);
        }
        let next = next - now.naive_utc().and_utc();
        tokio::time::Duration::from_secs(next.num_seconds() as u64)
    });
    tick.tick().await;
    let exit_code;
    tokio::select! {
        _ = tick.tick() => {
            log::info!("Exit code 3 {}", chrono::Local::now());

            exit_code = 3;
        }
        t = client.start() => {
            match t {
                Ok(()) => {
                    log::error!("Client exited normally");
                    exit_code = 0;
                }
                Err(why) => {
                    log::error!("Client error: {:?}", why);
                    log::info!("Exit code 1 {}", chrono::Local::now());

                    exit_code = 1;
                }
            }
        }
        t = clients.select_next_some() => {
            match t {
                Ok(Ok(())) => {
                    log::error!("Client exited normally");
                    exit_code = 1;
                }
                Ok(Err(why)) => {
                    log::error!("Client returned: {:?}", why);
                    log::info!("Exit code 1 {}", chrono::Local::now());

                    exit_code = 1;
                }
                Err(e) => {
                    log::error!("Client error: {:?}", e);
                    log::info!("Exit code 1 {}", chrono::Local::now());

                    exit_code = 1;
                }
            }
        }
        _ = tokio::signal::ctrl_c() => {
            log::info!("Exit code 2 {}", chrono::Local::now());

            exit_code = 2;
        }
    }
    if let Err(e) = kill_tx.send(()) {
        log::error!("Failed to send kill signal: {:?}", e);
    };
    log::info!("Getting write lock on data");
    let dw = client.data.read().await;
    log::info!("Got write lock on data");
    if let Some(v) = dw.get::<commands::music::AudioCommandHandler>().take() {
        for (i, x) in v.read().await.values().enumerate() {
            log::info!("Sending stop command {}", i);
            let (tx, rx) = oneshot::channel::<String>();
            if let Err(e) = x.send((tx, commands::music::AudioPromiseCommand::Stop(None))) {
                log::error!("Failed to send stop command: {}", e);
            };
            let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), rx);
            if let Ok(Ok(msg)) = timeout.await {
                log::info!("Stopped playing: {}", msg);
            } else {
                log::error!("Failed to stop playing");
            }
        }
    }
    if let Some(v) = dw.get::<commands::music::AudioHandler>().take() {
        for (i, x) in v.write().await.values_mut().enumerate() {
            log::info!("Joining handle {}", i);
            let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), x);
            if let Ok(Ok(())) = timeout.await {
                log::info!("Joined handle");
            } else {
                log::error!("Failed to join handle");
            }
        }
    }
    if let Some(v) = dw
        .get::<commands::music::transcribe::TranscribeData>()
        .take()
    {
        v.write().await.clear();
    }
    client.shard_manager.shutdown_all().await;
    // write the consent data to disk
    global_data::save();
    // {
    //     match std::fs::File::create(&cfg.consent_path) {
    //         Ok(f) => {
    //             if let Err(e) = serde_json::to_writer(&f, &*CONSENT_DATABASE) {
    //                 log::error!("Failed to write consent data: {:?}", e);
    //             }
    //         }
    //         Err(e) => {
    //             log::error!("Failed to create consent file: {:?}", e);
    //         }
    //     };
    // }
    std::process::exit(exit_code);
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Config {
    bots_config_path: PathBuf,
    guild_id: String,
    app_name: String,
    looptime: u64,
    data_path: PathBuf,
    shitgpt_path: PathBuf,
    whitelist_path: PathBuf,
    string_api_token: Option<String>,
    idle_url: String,
    api_url: Option<String>,
    #[cfg(feature = "tts")]
    gcloud_script: String,
    #[cfg(feature = "youtube-search")]
    youtube_api_key: String,
    #[cfg(feature = "youtube-search")]
    autocomplete_limit: u64,
    #[cfg(feature = "spotify")]
    spotify_api_key: String,
    bumper_url: String,
    #[cfg(feature = "transcribe")]
    transcribe_url: String,
    #[cfg(feature = "transcribe")]
    transcribe_token: String,
    #[cfg(feature = "transcribe")]
    alert_phrases_path: PathBuf,
    #[cfg(feature = "transcribe")]
    sam_path: PathBuf,
    #[cfg(feature = "transcribe")]
    consent_path: PathBuf,
}
impl Config {
    pub fn get() -> Self {
        let path = dirs::data_dir();
        let mut path = if let Some(path) = path {
            path
        } else {
            PathBuf::from(".")
        };
        path.push("RmbConfig.json");
        Self::get_from_path(path)
    }
    fn onboarding(config_path: &PathBuf, recovered_config: Option<RecoverConfig>) {
        let config = if let Some(rec) = recovered_config {
            log::error!("Welcome back to my shitty Rust Music Bot!");
            log::error!(
                "It appears that you have run the bot before, but the config got biffed up."
            );
            log::error!("I will take you through a short onboarding process to get you back up and running.");
            let app_name = if let Some(app_name) = rec.app_name {
                app_name
            } else {
                Self::safe_read("\nPlease enter your application name:")
            };
            let mut data_path = match config_path.parent() {
                Some(p) => p.to_path_buf(),
                None => {
                    log::error!("Failed to get parent, this should never happen.");
                    return;
                }
            };
            data_path.push(app_name.clone());
            Config {
                bots_config_path: if let Some(bots_config_path) = rec.bots_config_path {
                    bots_config_path
                } else {
                    Self::safe_read("\nPlease enter your bots config path:")
                },
                guild_id: if let Some(guild_id) = rec.guild_id {
                    guild_id
                } else {
                    Self::safe_read("\nPlease enter your guild id:")
                },
                app_name,
                looptime: if let Some(looptime) = rec.looptime {
                    looptime
                } else {
                    Self::safe_read("\nPlease enter your loop time in ms\nlower time means faster response but potentially higher cpu utilization (50 is a good compromise):")
                },
                #[cfg(feature = "tts")]
                gcloud_script: if let Some(gcloud_script) = rec.gcloud_script {
                    gcloud_script
                } else {
                    Self::safe_read("\nPlease enter your gcloud script location:")
                },
                #[cfg(feature = "youtube-search")]
                youtube_api_key: if let Some(youtube_api_key) = rec.youtube_api_key {
                    youtube_api_key
                } else {
                    Self::safe_read("\nPlease enter your youtube api key:")
                },
                #[cfg(feature = "youtube-search")]
                autocomplete_limit: if let Some(autocomplete_limit) = rec.autocomplete_limit {
                    autocomplete_limit
                } else {
                    Self::safe_read("\nPlease enter your youtube autocomplete limit:")
                },
                #[cfg(feature = "spotify")]
                spotify_api_key: if let Some(spotify_api_key) = rec.spotify_api_key {
                    spotify_api_key
                } else {
                    Self::safe_read("\nPlease enter your spotify api key:")
                },
                idle_url: if let Some(idle_audio) = rec.idle_url {
                    idle_audio
                } else {
                    Self::safe_read("\nPlease enter your idle audio URL (NOT A FILE PATH)\nif you wish to use a file on disk, set this to something as a fallback, and name the file override.mp3 inside the bot directory)\n(appdata/local/ for windows users and ~/.local/share/ for linux users):")
                },
                api_url: rec.api_url,
                bumper_url: if let Some(bumper_url) = rec.bumper_url {
                    bumper_url
                } else {
                    Self::safe_read("\nPlease enter your bumper audio URL (NOT A FILE PATH) (for silence put \"https://www.youtube.com/watch?v=Vbks4abvLEw\"):")
                },
                data_path: if let Some(data_path) = rec.data_path {
                    data_path
                } else {
                    data_path
                },
                shitgpt_path: if let Some(shitgpt_path) = rec.shitgpt_path {
                    shitgpt_path
                } else {
                    Self::safe_read("\nPlease enter your shitgpt path:")
                },
                whitelist_path: if let Some(whitelist_path) = rec.whitelist_path {
                    whitelist_path
                } else {
                    Self::safe_read("\nPlease enter your whitelist path:")
                },
                string_api_token: if let Some(string_api_token) = rec.string_api_token {
                    Some(string_api_token)
                } else {
                    Some(Self::safe_read("\nPlease enter your string api token:"))
                },
                #[cfg(feature = "transcribe")]
                transcribe_url: if let Some(transcribe_url) = rec.transcribe_url {
                    transcribe_url
                } else {
                    Self::safe_read("\nPlease enter your transcribe url:")
                },
                #[cfg(feature = "transcribe")]
                transcribe_token: if let Some(transcribe_token) = rec.transcribe_token {
                    transcribe_token
                } else {
                    Self::safe_read("\nPlease enter your transcribe token:")
                },
                #[cfg(feature = "transcribe")]
                alert_phrases_path: if let Some(alert_phrase_path) = rec.alert_phrase_path {
                    alert_phrase_path
                } else {
                    Self::safe_read("\nPlease enter your alert phrase path:")
                },
                #[cfg(feature = "transcribe")]
                sam_path: if let Some(sam_path) = rec.sam_path {
                    sam_path
                } else {
                    Self::safe_read("\nPlease enter your sam path:")
                },
                consent_path: if let Some(consent_path) = rec.consent_path {
                    consent_path
                } else {
                    Self::safe_read("\nPlease enter your consent path:")
                },
            }
        } else {
            log::error!("Welcome to my shitty Rust Music Bot!");
            log::error!("It appears that this may be the first time you are running the bot.");
            log::error!("I will take you through a short onboarding process to get you started.");
            let app_name: String = Self::safe_read("\nPlease enter your application name:");
            let mut data_path = match config_path.parent() {
                Some(p) => p.to_path_buf(),
                None => {
                    log::error!("Failed to get parent, this should never happen.");
                    return;
                }
            };
            data_path.push(app_name.clone());
            Config {
                bots_config_path: Self::safe_read("\nPlease enter your bots config path:"),
                guild_id: Self::safe_read("\nPlease enter your guild id:"),
                app_name,
                looptime: Self::safe_read("\nPlease enter your loop time in ms\nlower time means faster response but higher utilization:"),
                #[cfg(feature = "tts")]
                gcloud_script: Self::safe_read("\nPlease enter your gcloud script location:"),
                data_path,
                #[cfg(feature = "youtube-search")]
                youtube_api_key: Self::safe_read("\nPlease enter your youtube api key:"),
                #[cfg(feature = "youtube-search")]
                autocomplete_limit: Self::safe_read("\nPlease enter your youtube autocomplete limit:"),
                #[cfg(feature = "spotify")]
                spotify_api_key: Self::safe_read("\nPlease enter your spotify api key:"),
                idle_url: Self::safe_read("\nPlease enter your idle audio URL (NOT A FILE PATH):"),
                api_url: None,
                bumper_url: Self::safe_read("\nPlease enter your bumper audio URL (NOT A FILE PATH) (for silence put \"https://www.youtube.com/watch?v=Vbks4abvLEw\"):"),
                shitgpt_path: Self::safe_read("\nPlease enter your shitgpt path:"),
                whitelist_path: Self::safe_read("\nPlease enter your whitelist path:"),
                string_api_token: Some(Self::safe_read("\nPlease enter your string api token:")),
                transcribe_token: Self::safe_read("\nPlease enter your transcribe token:"),
                transcribe_url: Self::safe_read("\nPlease enter your transcribe url:"),
                alert_phrases_path: Self::safe_read("\nPlease enter your alert phrase path:"),
                sam_path: Self::safe_read("\nPlease enter your sam path:"),
                consent_path: Self::safe_read("\nPlease enter your consent path:"),
            }
        };
        match std::fs::write(
            config_path.clone(),
            match serde_json::to_string_pretty(&config) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("Failed to serialize config: {}", e);
                    return;
                }
            },
        ) {
            Ok(_) => {
                log::info!("Config written to {:?}", config_path);
            }
            Err(e) => {
                log::error!("Failed to write config to {:?}: {}", config_path, e);
            }
        }
    }
    fn safe_read<T: std::str::FromStr>(prompt: &str) -> T {
        loop {
            log::error!("{}", prompt);
            let mut input = String::new();
            if let Err(e) = std::io::stdin().read_line(&mut input) {
                log::error!("Failed to read input: {}", e);
                continue;
            }
            let input = input.trim();
            match input.parse::<T>() {
                Ok(input) => return input,
                Err(_) => log::error!("Invalid input"),
            }
        }
    }
    fn get_from_path(path: std::path::PathBuf) -> Self {
        if !path.exists() {
            Self::onboarding(&path, None);
        }
        let config = std::fs::read_to_string(&path);
        if let Ok(config) = config {
            let x: Result<Config, serde_json::Error> = serde_json::from_str(&config);
            if let Ok(x) = x {
                x
            } else {
                log::error!("Failed to parse config.json, Attempting recovery");
                let recovered = serde_json::from_str(&config);
                if let Ok(recovered) = recovered {
                    Self::onboarding(&path, Some(recovered));
                } else {
                    Self::onboarding(&path, None);
                }
                Self::get()
            }
        } else {
            log::error!("Failed to read config.json");
            Self::onboarding(&path, None);
            Self::get_from_path(path)
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RecoverConfig {
    bots_config_path: Option<PathBuf>,
    guild_id: Option<String>,
    app_name: Option<String>,
    looptime: Option<u64>,
    data_path: Option<PathBuf>,
    #[cfg(feature = "tts")]
    gcloud_script: Option<String>,
    #[cfg(feature = "youtube-search")]
    youtube_api_key: Option<String>,
    #[cfg(feature = "youtube-search")]
    autocomplete_limit: Option<u64>,
    #[cfg(feature = "spotify")]
    spotify_api_key: Option<String>,
    idle_url: Option<String>,
    api_url: Option<String>,
    shitgpt_path: Option<PathBuf>,
    whitelist_path: Option<PathBuf>,
    string_api_token: Option<String>,
    bumper_url: Option<String>,
    #[cfg(feature = "transcribe")]
    transcribe_url: Option<String>,
    #[cfg(feature = "transcribe")]
    transcribe_token: Option<String>,
    #[cfg(feature = "transcribe")]
    alert_phrase_path: Option<PathBuf>,
    #[cfg(feature = "transcribe")]
    sam_path: Option<PathBuf>,
    #[cfg(feature = "transcribe")]
    consent_path: Option<PathBuf>,
}
struct SatelliteHandler {
    playing: String,
    position: usize,
}
impl SatelliteHandler {
    fn new(playing: String, position: usize) -> Self {
        Self { playing, position }
    }
}
#[async_trait]
impl EventHandler for SatelliteHandler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        log::info!("Connected as {}", ready.user.name);
        ctx.set_activity(Some(ActivityData::playing(&self.playing)));
        global_data::add_satellite_wait(ctx, self.position).await;
    }
}
