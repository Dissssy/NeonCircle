#![deny(clippy::unwrap_used)]
#![feature(try_blocks)]
#![feature(duration_millis_float)]
#![feature(if_let_guard)]

mod commands;

mod radio;
mod sam;
mod video;
mod youtube;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
mod context_menu;
mod voice_events;

use anyhow::Error;

use commands::music::transcribe::{TranscribeChannelHandler, TranscribeData};
use commands::music::{OrAuto, SpecificVolume, VoiceAction, VoiceData};
use serde::{Deserialize, Serialize};
use serenity::all::*;

use songbird::SerenityInit;
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::commands::music::{AudioCommandHandler, AudioPromiseCommand, OrToggle};

struct Handler {
    commands: Vec<Box<dyn CommandTrait>>,
}

impl Handler {
    fn new(commands: Vec<Box<dyn CommandTrait>>) -> Self {
        Self { commands }
    }
}

lazy_static::lazy_static! {
    static ref WHITELIST: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(serde_json::from_reader(std::fs::File::open(Config::get().whitelist_path).expect("Failed to open whitelist path file")).expect("Failed to parse whitelist path file")));
    static ref WEB_CLIENT: reqwest::Client = reqwest::Client::new();
}

#[async_trait]
pub trait CommandTrait
where
    Self: Send + Sync,
{
    fn register(&self) -> CreateCommand;
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction);
    fn name(&self) -> &str;
    async fn autocomplete(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
    ) -> Result<(), Error>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSafe {
    pub id: String,
}

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match &interaction {
            Interaction::Command(rawcommand) => {
                let command_name = rawcommand.data.name.clone();
                let command = self.commands.iter().find(|c| c.name() == command_name);
                if let Some(command) = command {
                    command.run(&ctx, rawcommand).await;
                } else {
                    println!("Command not found: {command_name}");
                }
            }
            Interaction::Autocomplete(autocomplete) => {
                let commandn = autocomplete.data.name.clone();
                let command = self.commands.iter().find(|c| c.name() == commandn);
                if let Some(command) = command {
                    let r = command.autocomplete(&ctx, autocomplete).await;
                    if r.is_err() {}
                } else {
                    println!("Command not found: {commandn}");
                }
            }
            Interaction::Ping(p) => {
                println!("Ping: {:?}", p);
            }
            Interaction::Component(mci) => {
                let cmd = match mci.data.kind {
                    ComponentInteractionDataKind::Button => mci.data.custom_id.as_str(),
                    ComponentInteractionDataKind::StringSelect { ref values } => {
                        match values.first() {
                            Some(v) => v.as_str(),
                            None => {
                                println!("No values in select");
                                return;
                            }
                        }
                    }
                    _ => {
                        println!("Unknown component type");
                        return;
                    }
                };

                if cmd == "controls" {
                    if let Err(e) = mci
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Defer(
                                CreateInteractionResponseMessage::new().ephemeral(true),
                            ),
                        )
                        .await
                    {
                        eprintln!("Failed to send response: {}", e);
                    };
                    return;
                }

                match cmd {
                    original_command if ["pause", "skip", "stop", "looped", "shuffle", "repeat", "autoplay", "read_titles"].iter().any(|a| *a == original_command) => {
                        let guild_id = match mci.guild_id {
                            Some(id) => id,
                            None => {
                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("This can only be used in a server").ephemeral(true))).await {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            }
                        };

                        if let (Some(v), Some(member)) = (ctx.data.read().await.get::<VoiceData>().cloned(), mci.member.as_ref()) {
                            let mut v = v.lock().await;
                            let next_step = v.mutual_channel(&ctx, &guild_id, &member.user.id);

                            if let VoiceAction::InSame(_c) = next_step {
                                let audio_command_handler = ctx.data.read().await.get::<AudioCommandHandler>().expect("Expected AudioCommandHandler in TypeMap").clone();

                                let mut audio_command_handler = audio_command_handler.lock().await;

                                if let Some(tx) = audio_command_handler.get_mut(&guild_id.to_string()) {
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
                                                println!("Unknown command: {}", uh);
                                                return;
                                            }
                                        },
                                    )) {
                                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(format!("Failed to issue command for {} ERR {}", original_command, e)).ephemeral(true))).await {
                                            eprintln!("Failed to send response: {}", e);
                                        }
                                        return;
                                    }

                                    if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new().ephemeral(true))).await {
                                        eprintln!("Failed to send response: {}", e);
                                    }
                                    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), rrx).await;

                                    match timeout {
                                        Ok(Ok(_msg)) => {
                                            return;
                                        }
                                        Ok(Err(e)) => {
                                            println!("Failed to issue command for {} ERR: {}", original_command, e);
                                        }
                                        Err(e) => {
                                            println!("Failed to issue command for {} ERR: {}", original_command, e);
                                        }
                                    }

                                    if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(format!("Failed to issue command for {}", original_command)).ephemeral(true))).await {
                                        eprintln!("Failed to send response: {}", e);
                                    }
                                    return;
                                }

                                println!("{}", _c);
                            } else {
                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Get on in here, enjoy the tunes!").ephemeral(true))).await {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            }
                        }
                    }
                    raw if ["volume", "radiovolume"].iter().any(|a| *a == raw) => {
                        let guild_id = match mci.guild_id {
                            Some(id) => id,
                            None => {
                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("This can only be used in a server").ephemeral(true))).await {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            }
                        };

                        if let (Some(v), Some(member)) = (ctx.data.read().await.get::<VoiceData>().cloned(), mci.member.as_ref()) {
                            let mut v = v.lock().await;
                            let next_step = v.mutual_channel(&ctx, &guild_id, &member.user.id);

                            if let VoiceAction::InSame(_c) = next_step {
                                if let Err(e) = mci
                                    .create_response(
                                        &ctx.http,
                                        CreateInteractionResponse::Modal(
                                            CreateModal::new(
                                                raw,
                                                match raw {
                                                    "volume" => "Volume",
                                                    "radiovolume" => "Radio Volume",
                                                    _ => unreachable!(),
                                                },
                                            )
                                            .components(vec![CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Short, "%", "volume"))]),
                                        ),
                                    )
                                    .await
                                {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            } else {
                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Get on in here, enjoy the tunes!").ephemeral(true))).await {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            }
                        }
                    }
                    "bitrate" => {
                        let guild_id = match mci.guild_id {
                            Some(id) => id,
                            None => {
                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("This can only be used in a server").ephemeral(true))).await {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            }
                        };

                        if let (Some(v), Some(member)) = (ctx.data.read().await.get::<VoiceData>().cloned(), mci.member.as_ref()) {
                            let mut v = v.lock().await;
                            let next_step = v.mutual_channel(&ctx, &guild_id, &member.user.id);

                            if let VoiceAction::InSame(_c) = next_step {
                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Modal(CreateModal::new("bitrate", "Bitrate").components(vec![CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Short, "bps", "bitrate").placeholder("512 - 512000, left blank for auto").required(false))]))).await {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            } else {
                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Get on in here, enjoy the tunes!").ephemeral(true))).await {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            }
                        }
                    }
                    "log" => {
                        let guild_id = match mci.guild_id {
                            Some(id) => id,
                            None => {
                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("This can only be used in a server").ephemeral(true))).await {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            }
                        };

                        if let (Some(v), Some(member)) = (ctx.data.read().await.get::<VoiceData>().cloned(), mci.member.as_ref()) {
                            let mut v = v.lock().await;
                            let next_step = v.mutual_channel(&ctx, &guild_id, &member.user.id);

                            if let VoiceAction::InSame(_c) = next_step {
                                let audio_command_handler = ctx.data.read().await.get::<AudioCommandHandler>().expect("Expected AudioCommandHandler in TypeMap").clone();

                                let mut audio_command_handler = audio_command_handler.lock().await;

                                if let Some(tx) = audio_command_handler.get_mut(&guild_id.to_string()) {
                                    let (rtx, rrx) = oneshot::channel::<String>();
                                    let (realrtx, mut realrrx) = mpsc::channel::<Vec<String>>(1);
                                    if let Err(e) = tx.send((rtx, AudioPromiseCommand::RetrieveLog(realrtx))) {
                                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(format!("Failed to issue command for `log` ERR {}", e)).ephemeral(true))).await {
                                            eprintln!("Failed to send response: {}", e);
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
                                                        eprintln!("Failed to send response: {}", e);
                                                    }
                                                    return;
                                                }
                                                Ok(None) => {
                                                    println!("Failed to issue command for `log` ERR: None");
                                                }
                                                Err(e) => {
                                                    println!("Failed to issue command for `log` ERR: {}", e);
                                                }
                                            }
                                        }
                                        Ok(Err(e)) => {
                                            println!("Failed to issue command for `log` ERR: {}", e);
                                        }
                                        Err(e) => {
                                            println!("Failed to issue command for `log` ERR: {}", e);
                                        }
                                    }

                                    if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Failed to issue command for `log`").ephemeral(true))).await {
                                        eprintln!("Failed to send response: {}", e);
                                    }
                                    return;
                                }
                            } else {
                                if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Get on in here, enjoy the tunes!").ephemeral(true))).await {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            }
                        }
                    }
                    p => {
                        if let Err(e) = mci.create_response(&ctx.http, CreateInteractionResponse::Modal(CreateModal::new("feedback", "Feedback").components(vec![CreateActionRow::InputText(CreateInputText::new(InputTextStyle::Paragraph, format!("How should clicking `{}` work?\nRead the discord documentation and figure out what i can ACTUALLY do. I can't think of anything.", p), "feedback").required(true))]))).await {
                            eprintln!("Failed to send response: {}", e);
                        }
                    }
                }
            }
            Interaction::Modal(p) => match p.data.custom_id.as_str() {
                "feedback" => {
                    let i = match p
                        .data
                        .components
                        .first()
                        .and_then(|ar| ar.components.first())
                    {
                        Some(ActionRowComponent::InputText(feedback)) => feedback,
                        Some(_) => {
                            eprintln!("Invalid components in feedback modal");
                            return;
                        }
                        None => {
                            eprintln!("No components in feedback modal");
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
                                eprintln!("No value in feedback modal");
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
                                println!("Failed to send feedback: {}", e);
                                content = format!("{}{}\n{}\n{}\n{}", content, "Unfortunately, I failed to send your feedback to the developer.", "If you're able to, be sure to send it to him yourself!", "He's <@156533151198478336> (monkey_d._issy)\n\nHere's a copy if you need it.", feedback);
                            }
                        }
                        Err(e) => {
                            println!("Failed to get user: {}", e);
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
                        eprintln!("Failed to send response: {}", e);
                    }
                }
                raw if ["volume", "radiovolume"].iter().any(|a| *a == raw) => {
                    let val = match p
                        .data
                        .components
                        .first()
                        .and_then(|ar| ar.components.first())
                    {
                        Some(ActionRowComponent::InputText(volume)) => match volume.value {
                            Some(ref v) => v,
                            None => {
                                eprintln!("No value in volume modal");
                                return;
                            }
                        },
                        Some(_) => {
                            eprintln!("Invalid components in volume modal");
                            return;
                        }
                        None => {
                            eprintln!("No components in volume modal");
                            return;
                        }
                    };

                    let val = match val.parse::<f64>() {
                        Ok(v) => v,
                        Err(e) => {
                            println!("Failed to parse volume: {}", e);

                            if let Err(e) = p
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content(format!("`{}` is not a valid number", val))
                                            .ephemeral(true),
                                    ),
                                )
                                .await
                            {
                                eprintln!("Failed to send response: {}", e);
                            }
                            return;
                        }
                    };

                    if !(0.0..=100.0).contains(&val) {
                        if let Err(e) = p
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content(format!("`{}` is outside 0-100", val))
                                        .ephemeral(true),
                                ),
                            )
                            .await
                        {
                            eprintln!("Failed to send response: {}", e);
                        }
                        return;
                    }

                    let guild_id = match p.guild_id {
                        Some(id) => id,
                        None => {
                            if let Err(e) = p
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content("This can only be used in a server")
                                            .ephemeral(true),
                                    ),
                                )
                                .await
                            {
                                eprintln!("Failed to send response: {}", e);
                                return;
                            }
                            return;
                        }
                    };

                    if let (Some(v), Some(member)) = (
                        ctx.data.read().await.get::<VoiceData>().cloned(),
                        p.member.as_ref(),
                    ) {
                        let mut v = v.lock().await;
                        let next_step = v.mutual_channel(&ctx, &guild_id, &member.user.id);

                        if let VoiceAction::InSame(_c) = next_step {
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
                                if let Err(e) = tx.send((
                                    rtx,
                                    match raw {
                                        "volume" => AudioPromiseCommand::SpecificVolume(
                                            SpecificVolume::Volume(val / 100.0),
                                        ),
                                        "radiovolume" => AudioPromiseCommand::SpecificVolume(
                                            SpecificVolume::RadioVolume(val / 100.0),
                                        ),
                                        uh => {
                                            println!("Unknown volume to set: {}", uh);
                                            return;
                                        }
                                    },
                                )) {
                                    if let Err(e) = p
                                        .create_response(
                                            &ctx.http,
                                            CreateInteractionResponse::Message(
                                                CreateInteractionResponseMessage::new()
                                                    .content(format!(
                                                        "Failed to issue command for {} ERR {}",
                                                        raw, e
                                                    ))
                                                    .ephemeral(true),
                                            ),
                                        )
                                        .await
                                    {
                                        eprintln!("Failed to send response: {}", e);
                                    }
                                    return;
                                }

                                let timeout =
                                    tokio::time::timeout(std::time::Duration::from_secs(10), rrx)
                                        .await;

                                match timeout {
                                    Ok(Ok(_msg)) => {
                                        if let Err(e) = p
                                            .create_response(
                                                &ctx.http,
                                                CreateInteractionResponse::Defer(
                                                    CreateInteractionResponseMessage::new()
                                                        .ephemeral(true),
                                                ),
                                            )
                                            .await
                                        {
                                            eprintln!("Failed to send response: {}", e);
                                        }
                                        return;
                                    }
                                    Ok(Err(e)) => {
                                        println!("Failed to issue command for {} ERR: {}", raw, e);
                                    }
                                    Err(e) => {
                                        println!("Failed to issue command for {} ERR: {}", raw, e);
                                    }
                                }

                                if let Err(e) = p
                                    .create_response(
                                        &ctx.http,
                                        CreateInteractionResponse::Message(
                                            CreateInteractionResponseMessage::new()
                                                .content(format!(
                                                    "Failed to issue command for {}",
                                                    raw
                                                ))
                                                .ephemeral(true),
                                        ),
                                    )
                                    .await
                                {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            }

                            println!("{}", _c);
                        } else {
                            if let Err(e) = p.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Why did you leave? I was just about to change the volume!").ephemeral(true))).await {
                                eprintln!("Failed to send response: {}", e);
                            }
                            return;
                        }
                    }
                }
                "bitrate" => {
                    let val = match p
                        .data
                        .components
                        .first()
                        .and_then(|ar| ar.components.first())
                    {
                        Some(ActionRowComponent::InputText(bitrate)) => match bitrate.value {
                            Some(ref v) => v,
                            None => {
                                eprintln!("No value in bitrate modal");
                                return;
                            }
                        },
                        Some(_) => {
                            eprintln!("Invalid components in bitrate modal");
                            return;
                        }
                        None => {
                            eprintln!("No components in bitrate modal");
                            return;
                        }
                    };

                    let val = if val.is_empty() {
                        OrAuto::Auto
                    } else {
                        OrAuto::Specific({
                            let val = match val.parse::<i64>() {
                                Ok(v) => v,
                                Err(e) => {
                                    println!("Failed to parse bitrate: {}", e);

                                    if let Err(e) = p
                                        .create_response(
                                            &ctx.http,
                                            CreateInteractionResponse::Message(
                                                CreateInteractionResponseMessage::new()
                                                    .content(format!(
                                                        "`{}` is not a valid number",
                                                        val
                                                    ))
                                                    .ephemeral(true),
                                            ),
                                        )
                                        .await
                                    {
                                        eprintln!("Failed to send response: {}", e);
                                    }
                                    return;
                                }
                            };
                            if !(512..=512000).contains(&val) {
                                if let Err(e) = p
                                    .create_response(
                                        &ctx.http,
                                        CreateInteractionResponse::Message(
                                            CreateInteractionResponseMessage::new()
                                                .content(format!("`{}` is outside 512-512000", val))
                                                .ephemeral(true),
                                        ),
                                    )
                                    .await
                                {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            }

                            val
                        })
                    };

                    let guild_id = match p.guild_id {
                        Some(id) => id,
                        None => {
                            if let Err(e) = p
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Message(
                                        CreateInteractionResponseMessage::new()
                                            .content("This can only be used in a server")
                                            .ephemeral(true),
                                    ),
                                )
                                .await
                            {
                                eprintln!("Failed to send response: {}", e);
                                return;
                            }
                            return;
                        }
                    };

                    if let (Some(v), Some(member)) = (
                        ctx.data.read().await.get::<VoiceData>().cloned(),
                        p.member.as_ref(),
                    ) {
                        let mut v = v.lock().await;
                        let next_step = v.mutual_channel(&ctx, &guild_id, &member.user.id);

                        if let VoiceAction::InSame(_c) = next_step {
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
                                if let Err(e) = tx.send((rtx, AudioPromiseCommand::SetBitrate(val)))
                                {
                                    if let Err(e) = p.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(format!("Failed to issue command for bitrate ERR {}", e)).ephemeral(true))).await {
                                        eprintln!("Failed to send response: {}", e);
                                    }
                                    return;
                                }

                                let timeout =
                                    tokio::time::timeout(std::time::Duration::from_secs(10), rrx)
                                        .await;

                                match timeout {
                                    Ok(Ok(_msg)) => {
                                        if let Err(e) = p
                                            .create_response(
                                                &ctx.http,
                                                CreateInteractionResponse::Defer(
                                                    CreateInteractionResponseMessage::new()
                                                        .ephemeral(true),
                                                ),
                                            )
                                            .await
                                        {
                                            eprintln!("Failed to send response: {}", e);
                                        }
                                        return;
                                    }
                                    Ok(Err(e)) => {
                                        println!("Failed to issue command for bitrate ERR: {}", e);
                                    }
                                    Err(e) => {
                                        println!("Failed to issue command for bitrate ERR: {}", e);
                                    }
                                }

                                if let Err(e) = p
                                    .create_response(
                                        &ctx.http,
                                        CreateInteractionResponse::Message(
                                            CreateInteractionResponseMessage::new()
                                                .content("Failed to issue command for bitrate")
                                                .ephemeral(true),
                                        ),
                                    )
                                    .await
                                {
                                    eprintln!("Failed to send response: {}", e);
                                }
                                return;
                            }

                            println!("{}", _c);
                        } else {
                            if let Err(e) = p.create_response(&ctx.http, CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content("Why did you leave? I was just about to change the bitrate!").ephemeral(true))).await {
                                eprintln!("Failed to send response: {}", e);
                            }
                            return;
                        }
                    }
                }
                "log" => {
                    if let Err(e) = p
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Defer(
                                CreateInteractionResponseMessage::new(),
                            ),
                        )
                        .await
                    {
                        eprintln!("Failed to send response: {}", e);
                    }
                }
                _ => {
                    println!("You missed one, idiot: {:?}", p);
                }
            },
            _ => {
                println!("FUCK YOU");
            }
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
        let mut users = Vec::new();

        let voicedata = ctx
            .data
            .read()
            .await
            .get::<VoiceData>()
            .expect("Expected VoiceData in TypeMap.")
            .clone();

        let mut voicedata = voicedata.lock().await;

        for guild in ready.guilds {
            match ctx.http.get_guild(guild.id).await {
                Ok(guild) => {
                    for member in match guild.members(&ctx.http, None, None).await {
                        Ok(members) => members,
                        Err(e) => {
                            println!("Error getting members: {e}");
                            Vec::new()
                        }
                    } {
                        let id = member.user.id.get().to_string();
                        if !users.contains(&id) {
                            users.push(id);
                        }
                    }

                    if let Err(e) = voicedata.refresh_guild(&ctx, guild.id).await {
                        println!("Failed to refresh voice states for guild: {}", e);
                    }
                }
                Err(e) => {
                    println!("Error getting guild: {e}");
                }
            }
        }
        drop(voicedata);
        let mut finalusers = Vec::new();
        for id in users {
            finalusers.push(UserSafe { id });
        }

        let mut req = WEB_CLIENT
            .post("http://localhost:16834/api/set/user")
            .json(&finalusers);
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to send users to api {e}. Users might be out of date");
        }

        let mut req = WEB_CLIENT
            .post("http://localhost:16835/api/set/user")
            .json(&finalusers);
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to send users to api {e}. Users might be out of date");
        }

        println!("Registering commands");
        if let Err(e) = Command::set_global_commands(
            &ctx.http,
            self.commands
                .iter()
                .map(|command| command.register())
                .collect(),
        )
        .await
        {
            eprintln!("Failed to register commands: {}", e);
        }
    }

    async fn voice_state_update(&self, ctx: Context, old: Option<VoiceState>, new: VoiceState) {
        let data = {
            let uh = ctx.data.read().await;
            uh.get::<VoiceData>()
                .expect("Expected VoiceData in TypeMap.")
                .clone()
        };
        {
            let mut data = data.lock().await;
            data.update(old.clone(), new.clone());
        }

        let guild_id = match (old.and_then(|o| o.guild_id), new.guild_id) {
            (Some(g), _) => g,
            (_, Some(g)) => g,
            _ => return,
        };

        let leave = {
            let mut data = data.lock().await;
            data.bot_alone(&guild_id)
        };

        if !leave {
            return;
        }

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
            if let Err(e) = tx.send((rtx, AudioPromiseCommand::Stop(None))) {
                eprintln!("Failed to send stop command: {}", e);
            };

            let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), rrx).await;

            match timeout {
                Ok(Ok(_msg)) => {
                    return;
                }
                Ok(Err(e)) => {
                    println!("Failed to issue command for stop ERR: {}", e);
                }
                Err(e) => {
                    println!("Failed to issue command for stop ERR: {}", e);
                }
            }
        }
    }

    async fn message(&self, ctx: Context, new_message: Message) {
        if new_message.content.trim().is_empty() {
            return;
        }

        let guild_id = match new_message.guild_id {
            Some(guild) => guild,
            None => return,
        };
        let em = match ctx
            .data
            .write()
            .await
            .get_mut::<TranscribeData>()
            .expect("Expected TranscribeData in TypeMap.")
            .lock()
            .await
            .entry(guild_id)
        {
            std::collections::hash_map::Entry::Occupied(ref mut e) => e.get_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let uh = TranscribeChannelHandler::new();

                e.insert(Arc::new(Mutex::new(uh)))
            }
        }
        .clone();

        let mut e = em.lock().await;

        e.send_tts(&ctx, &new_message).await;
    }

    async fn resume(&self, ctx: Context, _: ResumedEvent) {
        let mut users = Vec::new();
        for guild in match ctx.http.get_guilds(None, None).await {
            Ok(guilds) => guilds,
            Err(e) => {
                println!("Error getting guilds: {e}");
                return;
            }
        } {
            match ctx.http.get_guild(guild.id).await {
                Ok(guild) => {
                    for member in match guild.members(&ctx.http, None, None).await {
                        Ok(members) => members,
                        Err(e) => {
                            println!("Error getting members: {e}");
                            continue;
                        }
                    } {
                        let id = member.user.id.get().to_string();
                        if !users.contains(&id) {
                            users.push(id);
                        }
                    }
                }
                Err(e) => {
                    println!("Error getting guild: {e}");
                }
            }
        }
        let mut finalusers = Vec::new();
        for id in users {
            finalusers.push(UserSafe { id });
        }

        let mut req = WEB_CLIENT
            .post("http://localhost:16834/api/set/user")
            .json(&finalusers);
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to send users to api {e}. Users might be out of date");
        }

        let mut req = WEB_CLIENT
            .post("http://localhost:16835/api/set/user")
            .json(&finalusers);
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to send users to api {e}. Users might be out of date");
        }
    }

    async fn guild_member_addition(&self, _ctx: Context, new_member: Member) {
        let id = new_member.user.id.get().to_string();

        let mut req = WEB_CLIENT
            .post("http://localhost:16834/api/add/user")
            .json(&UserSafe { id: id.clone() });
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to add user to api {e}. Users might be out of date");
        }

        let mut req = WEB_CLIENT
            .post("http://localhost:16835/api/add/user")
            .json(&UserSafe { id });
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to add user to api {e}. Users might be out of date");
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

        let mut req = WEB_CLIENT
            .post("http://localhost:16834/api/remove/user")
            .json(&UserSafe { id: id.clone() });
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to remove user from api {e}. Users might be out of date");
        }

        let mut req = WEB_CLIENT
            .post("http://localhost:16835/api/remove/user")
            .json(&UserSafe { id });
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to remove user from api {e}. Users might be out of date");
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

    let cfg = Config::get();

    let mut tmp = cfg.data_path.clone();
    tmp.push("tmp");

    let r = std::fs::remove_dir_all(&tmp);
    if r.is_err() {
        println!("Failed to remove tmp folder");
    }
    std::fs::create_dir_all(&tmp).expect("Failed to create tmp folder");

    let token = cfg.token;

    let handler = Handler::new(vec![
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
        Box::new(commands::embed::Video),
        Box::new(commands::embed::Audio),
        Box::new(commands::embed::John),
    ]);

    let config = songbird::Config::default()
        .preallocated_tracks(2)
        .decode_mode(songbird::driver::DecodeMode::Decode)
        .crypto_mode(songbird::driver::CryptoMode::Lite);

    let mut client = Client::builder(token, GatewayIntents::all())
        .register_songbird_from_config(config)
        .event_handler(handler)
        .await
        .expect("Error creating client");
    {
        let mut data = client.data.write().await;
        data.insert::<commands::music::AudioHandler>(Arc::new(Mutex::new(HashMap::new())));
        data.insert::<commands::music::AudioCommandHandler>(Arc::new(Mutex::new(HashMap::new())));
        data.insert::<commands::music::VoiceData>(Arc::new(Mutex::new(
            commands::music::InnerVoiceData::new(client.cache.current_user().id),
        )));
        data.insert::<commands::music::transcribe::TranscribeData>(Arc::new(Mutex::new(
            HashMap::new(),
        )));
    }

    let mut tick = tokio::time::interval({
        let now = chrono::Local::now();
        let mut next = chrono::Local::now()
            .date_naive()
            .and_hms_opt(8, 0, 0)
            .expect("Failed to get next 8 am, wtf? did time end?")
            .and_utc();
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
            println!("Exit code 3 {}", chrono::Local::now());

            exit_code = 3;
        }
        Err(why) = client.start() => {
            println!("Client error: {:?}", why);
            println!("Exit code 1 {}", chrono::Local::now());

            exit_code = 1;
        }
        _ = tokio::signal::ctrl_c() => {
            println!("Exit code 2 {}", chrono::Local::now());

            exit_code = 2;
        }
    }
    println!("Getting write lock on data");
    let dw = client.data.write().await;
    println!("Got write lock on data");
    if let Some(v) = dw.get::<commands::music::AudioCommandHandler>().take() {
        for (i, x) in v.lock().await.values().enumerate() {
            println!("Sending stop command {}", i);
            let (tx, rx) = oneshot::channel::<String>();

            if let Err(e) = x.send((tx, commands::music::AudioPromiseCommand::Stop(None))) {
                println!("Failed to send stop command: {}", e);
            };

            let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), rx);

            if let Ok(Ok(msg)) = timeout.await {
                println!("Stopped playing: {}", msg);
            } else {
                println!("Failed to stop playing");
            }
        }
    }
    if let Some(v) = dw.get::<commands::music::AudioHandler>().take() {
        for (i, x) in v.lock().await.values_mut().enumerate() {
            println!("Joining handle {}", i);

            let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), x);

            if let Ok(Ok(())) = timeout.await {
                println!("Joined handle");
            } else {
                println!("Failed to join handle");
            }
        }
    }

    if let Some(v) = dw
        .get::<commands::music::transcribe::TranscribeData>()
        .take()
    {
        v.lock().await.clear();
    }

    client.shard_manager.shutdown_all().await;

    std::process::exit(exit_code);
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Config {
    token: String,
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
            println!("Welcome back to my shitty Rust Music Bot!");
            println!("It appears that you have run the bot before, but the config got biffed up.");
            println!("I will take you through a short onboarding process to get you back up and running.");
            let app_name = if let Some(app_name) = rec.app_name {
                app_name
            } else {
                Self::safe_read("\nPlease enter your application name:")
            };
            let mut data_path = config_path
                .parent()
                .expect("Failed to get parent, this should never happen.")
                .to_path_buf();
            data_path.push(app_name.clone());
            Config {
                token: if let Some(token) = rec.token {
                    token
                } else {
                    Self::safe_read("\nPlease enter your bot token:")
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
            }
        } else {
            println!("Welcome to my shitty Rust Music Bot!");
            println!("It appears that this may be the first time you are running the bot.");
            println!("I will take you through a short onboarding process to get you started.");
            let app_name: String = Self::safe_read("\nPlease enter your application name:");
            let mut data_path = config_path
                .parent()
                .expect("Failed to get parent, this should never happen.")
                .to_path_buf();
            data_path.push(app_name.clone());
            Config {
                token: Self::safe_read("\nPlease enter your bot token:"),
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
            }
        };
        std::fs::write(
            config_path.clone(),
            serde_json::to_string_pretty(&config)
                .unwrap_or_else(|_| panic!("Failed to write\n{:?}", config_path)),
        )
        .expect("Failed to write config.json");
        println!("Config written to {:?}", config_path);
    }
    fn safe_read<T: std::str::FromStr>(prompt: &str) -> T {
        loop {
            println!("{}", prompt);
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .expect("Failed to read line");
            let input = input.trim();
            match input.parse::<T>() {
                Ok(input) => return input,
                Err(_) => println!("Invalid input"),
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
                println!("Failed to parse config.json, Attempting recovery");
                let recovered = serde_json::from_str(&config);
                if let Ok(recovered) = recovered {
                    Self::onboarding(&path, Some(recovered));
                } else {
                    Self::onboarding(&path, None);
                }
                Self::get()
            }
        } else {
            println!("Failed to read config.json");
            Self::onboarding(&path, None);
            Self::get_from_path(path)
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RecoverConfig {
    token: Option<String>,
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
}
