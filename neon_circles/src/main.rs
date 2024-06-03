#![feature(if_let_guard, try_blocks, duration_millis_float)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::implicit_clone,
    clippy::clone_on_ref_ptr,
)]
mod commands;
mod global_data;
mod utils;
mod radio;
mod sam;
mod video;
mod youtube;
use std::sync::atomic::AtomicBool;
use std::{collections::HashMap, time::Duration};
use std::sync::Arc;
mod context_menu;
#[cfg(feature = "transcribe")]
mod voice_events;
use crate::commands::music::{AudioCommandHandler, AudioPromiseCommand, OrToggle};
use commands::music::MetaCommand;
use global_data::voice_data::VoiceAction;
use serde::{Deserialize, Serialize};
mod traits {
    pub use common::{CommandTrait, SubCommandTrait};
}
mod config {
    pub use common::get_config;
}
use common::log;
use common::serenity::{
    all::*,
    futures::{stream::FuturesUnordered, StreamExt},
};
use songbird::SerenityInit;
use tokio::sync::{mpsc, oneshot, RwLock};
struct PlanetHandler {
    commands: Vec<Box<dyn traits::CommandTrait>>,
    initialized: AtomicBool,
    playing: String,
}
impl PlanetHandler {
    fn new(commands: Vec<Box<dyn traits::CommandTrait>>, activity: String) -> Self {
        Self {
            commands,
            initialized: AtomicBool::new(false),
            playing: activity,
        }
    }
}
lazy_static::lazy_static! {
    static ref WHITELIST: Arc<RwLock<Vec<String>>> = Arc::new(RwLock::new({
        let file = match std::fs::File::open(config::get_config().whitelist_path) {
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
        let file = match std::fs::File::open(config::get_config().bots_config_path) {
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
                    if let Err(e) = command.autocomplete(&ctx, autocomplete).await {
                        log::error!("Failed to run autocomplete for {} ERR: {}", commandn, e);
                    }
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
                let next_step = match global_data::voice_data::mutual_channel(&guild_id, &mci.user.id).await {
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
                                    let next_step = match global_data::voice_data::mutual_channel(&guild_id, &member.user.id).await {
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
                                            let (rtx, rrx) = oneshot::channel::<Arc<str>>();
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
                                    let next_step = match global_data::voice_data::mutual_channel(&guild_id, &member.user.id).await {
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
                                    let next_step = match global_data::voice_data::mutual_channel(&guild_id, &member.user.id).await {
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
                                    let next_step = match global_data::voice_data::mutual_channel(&guild_id, &member.user.id).await {
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
                                            let (rtx, rrx) = oneshot::channel::<Arc<str>>();
                                            let (realrtx, mut realrrx) = mpsc::channel::<Vec<String>>(1);
                                            if let Err(e) = tx.send((rtx, AudioPromiseCommand::MetaCommand(MetaCommand::RetrieveLog(realrtx)))) {
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
        log::info!("Ready");
        if self.initialized.load(std::sync::atomic::Ordering::Relaxed) {
            log::info!("Already initialized");
            return;
        }
        log::info!("Initializing Voice Data");
        if let Err(e) = global_data::voice_data::initialize_planet(ctx.clone()).await {
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
                            global_data::voice_data::lazy_refresh_guild(guild.id)
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
                if let Err(e) = global_data::voice_data::insert_guild(guild_id, voice_data).await {
                    log::error!("Failed to insert guild: {}", e);
                }
            }
        }
        if let Err(e) = WEB_CLIENT
            .post("http://localhost:16835/api/set/user")
            .json(&finalusers)
            .bearer_auth(config::get_config().string_api_token)
            .send()
            .await
        {
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
        if let Err(e) = global_data::voice_data::add_satellite(ctx, 0).await {
            log::error!("Failed to add satellite: {}", e);
        }
        self.initialized.store(true, std::sync::atomic::Ordering::Relaxed);
        log::info!("Connected as {}", ready.user.name);
    }
    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
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

        if let Some(channel_id) = new.channel_id {
            // get the command handler for the channel if it exists, and fire off a UserConnect command
            if let Some(audio_command_handler) = ctx
                .data
                .read()
                .await
                .get::<AudioCommandHandler>().map(Arc::clone) {
                let mut audio_command_handler = audio_command_handler.write().await;
                if let Some(tx) = audio_command_handler.get_mut(&channel_id) {
                    let (rtx, rrx) = oneshot::channel::<Arc<str>>();
                    if let Err(e) = tx.send((rtx, AudioPromiseCommand::MetaCommand(MetaCommand::UserConnect(new.user_id)))) {
                        log::error!("Failed to send UserConnect command: {}", e);
                    }
                    let timeout = tokio::time::timeout(Duration::from_secs(10), rrx).await;
                    if let Ok(Ok(msg)) = timeout {
                        log::trace!("UserConnect: {}", msg);
                    } else {
                        log::error!("Failed to get UserConnect response");
                    }
                } else {
                    log::trace!("No command handler for channel");
                }
            }
        }
        {
            // let mut data = data.write().await;
            // data.update(old.clone(), new.clone());
            if let Err(e) = global_data::voice_data::update_voice(old, new).await {
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
    async fn message(&self, _ctx: Context, new_message: Message) {
        if new_message.author.bot || new_message.content.trim().is_empty() {
            return;
        }
        // if let Some(guild_id) = global_data::transcribe::get_transcribe(new_message.channel_id) {
        //     let em = match commands::music::get_transcribe_channel_handler(&ctx, &guild_id).await {
        //         Ok(e) => e,
        //         Err(e) => {
        //             log::error!("Failed to get transcribe channel handler: {}", e);
        //             return;
        //         }
        //     };
        //     em.write().await.send_tts(&ctx, &new_message).await;
        // }
        global_data::transcribe::send_message(new_message).await;
    }
    async fn resume(&self, ctx: Context, _: ResumedEvent) {
        log::info!("Resumed");
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
                            if let Err(e) = global_data::voice_data::refresh_guild(guild.id).await {
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
        if let Err(e) = WEB_CLIENT
            .post("http://localhost:16835/api/set/user")
            .json(&finalusers)
            .bearer_auth(config::get_config().string_api_token)
            .send()
            .await
        {
            log::error!("Failed to send users to api {e}. Users might be out of date");
        }
    }
    async fn guild_member_addition(&self, _ctx: Context, new_member: Member) {
        let id = new_member.user.id.get().to_string();
        // let mut req = WEB_CLIENT
        //     .post("http://localhost:16834/api/add/user")
        //     .json(&UserSafe { id: id.clone() });
        // if let Some(token) = config::get_config().string_api_token {
        //     req = req.bearer_auth(token);
        // }
        // if let Err(e) = req.send().await {
        //     log::error!("Failed to add user to api {e}. Users might be out of date");
        // }
        if let Err(e) = WEB_CLIENT
            .post("http://localhost:16835/api/add/user")
            .json(&UserSafe { id })
            .bearer_auth(config::get_config().string_api_token)
            .send()
            .await
        {
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
        // if let Some(token) = config::get_config().string_api_token {
        //     req = req.bearer_auth(token);
        // }
        // if let Err(e) = req.send().await {
        //     log::error!("Failed to remove user from api {e}. Users might be out of date");
        // }
        if let Err(e) = WEB_CLIENT
            .post("http://localhost:16835/api/remove/user")
            .json(&UserSafe { id })
            .bearer_auth(config::get_config().string_api_token)
            .send()
            .await
        {
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
            if let Err(e) = global_data::voice_data::refresh_guild(guild.id).await {
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
            if let Err(e) = WEB_CLIENT
                .post("http://localhost:16835/api/set/user")
                .json(&users)
                .bearer_auth(config::get_config().string_api_token)
                .send().await
            {
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
    global_data::init().await;
    #[cfg(feature = "debug")]
    console_subscriber::init();
    // let cfg = config::get_config();
    // let mut tmp = cfg.data_path.clone();
    // tmp.push("tmp");
    // if let Err(e) = std::fs::remove_dir_all(&tmp) {
    //     log::error!("Failed to remove tmp folder: {:?}", e);
    // }
    // match std::fs::create_dir_all(&tmp) {
    //     Ok(_) => {}
    //     Err(e) => {
    //         log::error!("Failed to create tmp folder: {:?}", e);
    //         return;
    //     }
    // }
    let handler = PlanetHandler::new(
        vec![
            // Box::new(commands::music::transcribe::Command),
            Box::new(commands::music::repeat::Command),
            Box::new(commands::music::loop_queue::Command),
            Box::new(commands::music::pause::Command),
            Box::new(commands::music::add::Command),
            Box::new(commands::music::join::Command),
            Box::new(commands::music::setbitrate::Command),
            Box::new(commands::music::remove::Command),
            Box::new(commands::music::resume::Command),
            Box::new(commands::music::shuffle::Command),
            Box::new(commands::music::skip::Command),
            Box::new(commands::music::stop::Command),
            Box::new(commands::music::volume::Command),
            Box::new(commands::music::autoplay::Command),
            Box::new(commands::music::consent::Command),
            Box::new(commands::embed::Video),
            Box::new(commands::embed::Audio),
            Box::new(commands::john::Command),
            Box::new(commands::feedback::Feedback),
            Box::new(commands::config::Command::new()),
            Box::new(commands::remind::Command::new())
        ],
        BOTS.planet.playing.clone(),
    );
    let config = songbird::Config::default()
        .preallocated_tracks(4)
        .decode_mode(songbird::driver::DecodeMode::Decode)
        .crypto_mode(songbird::driver::CryptoMode::Normal);
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
        // data.insert::<commands::music::transcribe::TranscribeData>(Arc::new(RwLock::new(
        //     HashMap::new(),
        // )));
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
    // let mut tick = tokio::time::interval({
    //     let now = chrono::Local::now();
    //     let mut next = match chrono::Local::now().date_naive().and_hms_opt(8, 0, 0) {
    //         Some(v) => v.and_utc(),
    //         None => {
    //             log::error!("Failed to get next 8am, did time stop?");
    //             return;
    //         }
    //     };
    //     if next < now {
    //         next += chrono::Duration::days(1);
    //     }
    //     let next = next - now.naive_utc().and_utc();
    //     tokio::time::Duration::from_secs(next.num_seconds() as u64)
    // });
    // tick.tick().await;
    let exit_code;
    tokio::select! {
        // _ = tick.tick() => {
        //     log::info!("Exit code 3 {}", chrono::Local::now());

        //     exit_code = 3;
        // }
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
            let (tx, rx) = oneshot::channel::<Arc<str>>();
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
    // if let Some(v) = dw
    //     .get::<commands::music::transcribe::TranscribeData>()
    //     .take()
    // {
    //     v.write().await.clear();
    // }
    client.shard_manager.shutdown_all().await;
    for client in clients {
        let timeout = tokio::time::timeout(std::time::Duration::from_secs(3), client);
        if let Ok(Ok(Ok(()))) = timeout.await {
            log::info!("Client exited normally");
        } else {
            log::error!("Client failed to exit");
        }
    }
    // write the consent data to disk
    global_data::save().await;
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
        if let Err(e) = Command::set_global_commands(
            &ctx.http,
            vec![]
        ).await {
            log::error!("Failed to register commands: {}", e);
        }
        global_data::voice_data::add_satellite_wait(ctx, self.position).await;
    }
}
