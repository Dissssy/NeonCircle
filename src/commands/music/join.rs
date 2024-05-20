use super::{
    mainloop::{ControlData, Log},
    settingsdata::SettingsData,
    transcribe::TranscriptionThread,
    AudioHandler, AudioPromiseCommand,
};
use anyhow::Result;
use serenity::all::*;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
#[derive(Debug, Clone)]
pub struct Join;
#[async_trait]
impl crate::CommandTrait for Join {
    fn register(&self) -> CreateCommand {
        CreateCommand::new(self.name()).description("Join the voice channel")
    }
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) {
        if let Err(e) = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Defer(
                    CreateInteractionResponseMessage::new().ephemeral(true),
                ),
            )
            .await
        {
            log::error!("Failed to create interaction response: {:?}", e);
        }
        let guild_id = match interaction.guild_id {
            Some(id) => id,
            None => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new()
                            .content("This command can only be used in a server"),
                    )
                    .await
                {
                    log::error!("Failed to edit original interaction response: {:?}", e);
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
                v.write()
                    .await
                    .mutual_channel(ctx, &guild_id, &member.user.id)
            };
            match next_step {
                super::VoiceAction::UserNotConnected => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content("You're not in a voice channel"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::InDifferent(_channel) => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new()
                                .content("I'm in a different voice channel"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::InSame(_channel) => {
                    if let Err(e) = interaction.edit_response(&ctx.http, EditInteractionResponse::new().content("I'm already in the same voice channel as you, what do you want from me?")).await {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::Join(channel) => {
                    let manager = match songbird::get(ctx).await {
                        Some(v) => v,
                        None => {
                            if let Err(e) = interaction
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new()
                                        .content("Failed to get songbird manager"),
                                )
                                .await
                            {
                                log::error!(
                                    "Failed to edit original interaction response: {:?}",
                                    e
                                );
                            }
                            return;
                        }
                    };
                    {
                        let audio_handler = {
                            match ctx.data.read().await.get::<AudioHandler>() {
                                Some(v) => Arc::clone(v),
                                None => {
                                    if let Err(e) = interaction
                                        .edit_response(
                                            &ctx.http,
                                            EditInteractionResponse::new()
                                                .content("Failed to get audio handler"),
                                        )
                                        .await
                                    {
                                        log::error!(
                                            "Failed to edit original interaction response: {:?}",
                                            e
                                        );
                                    }
                                    return;
                                }
                            }
                        };
                        match manager.join(guild_id, channel).await {
                            Ok(call) => {
                                let (tx, rx) = mpsc::unbounded_channel::<(
                                    oneshot::Sender<String>,
                                    AudioPromiseCommand,
                                )>();
                                let transcription = TranscriptionThread::new(
                                    Arc::clone(&call),
                                    Arc::clone(&ctx.http),
                                    tx.clone(),
                                )
                                .await;
                                let msg = match interaction
                                    .channel_id
                                    .send_message(
                                        &ctx.http,
                                        CreateMessage::new()
                                            .content("Joining voice channel")
                                            .flags(MessageFlags::SUPPRESS_NOTIFICATIONS),
                                    )
                                    .await
                                {
                                    Ok(msg) => msg,
                                    Err(e) => {
                                        log::error!("Failed to send message: {:?}", e);
                                        if let Err(e) = interaction
                                            .edit_response(
                                                &ctx.http,
                                                EditInteractionResponse::new()
                                                    .content("Failed to send message"),
                                            )
                                            .await
                                        {
                                            log::error!("Failed to edit original interaction response: {:?}", e);
                                        }
                                        return;
                                    }
                                };
                                let messageref = super::MessageReference::new(
                                    ctx.http.clone(),
                                    ctx.cache.clone(),
                                    guild_id,
                                    msg.channel_id,
                                    msg,
                                );
                                let cfg = crate::Config::get();
                                let mut nothing_path = cfg.data_path.clone();
                                nothing_path.push("override.mp3");
                                let nothing_path = if nothing_path.exists() {
                                    Some(nothing_path)
                                } else {
                                    None
                                };
                                let guild_id = match interaction.guild_id {
                                    Some(guild) => guild,
                                    None => return,
                                };
                                let em = match super::get_transcribe_channel_handler(ctx, &guild_id)
                                    .await
                                {
                                    Ok(v) => v,
                                    Err(e) => {
                                        log::error!(
                                            "Failed to get transcribe channel handler: {:?}",
                                            e
                                        );
                                        if let Err(e) = interaction
                                            .edit_response(
                                                &ctx.http,
                                                EditInteractionResponse::new()
                                                    .content("Failed to get handler"),
                                            )
                                            .await
                                        {
                                            log::error!(
                                    "Failed to edit original interaction response: {:?}",
                                    e
                                );
                                        }
                                        return;
                                    }
                                };
                                if let Err(e) = em.write().await.register(channel).await {
                                    log::error!("Error registering channel: {:?}", e);
                                }
                                let handle = tokio::task::spawn(async move {
                                    let control = ControlData {
                                        call,
                                        rx,
                                        msg: messageref,
                                        nothing_uri: nothing_path,
                                        settings: SettingsData::default(),
                                        brk: false,
                                        log: Log::new(format!("{}-{}", guild_id, channel)),
                                        transcribe: em,
                                    };
                                    super::mainloop::the_lüüp(
                                        cfg.looptime,
                                        transcription,
                                        control,
                                    )
                                    .await;
                                });
                                audio_handler
                                    .write()
                                    .await
                                    .insert(guild_id.to_string(), handle);
                                let audio_command_handler = {
                                    let read_lock = ctx.data.read().await;
                                    match read_lock.get::<super::AudioCommandHandler>() {
                                        Some(v) => Arc::clone(v),
                                        None => {
                                            if let Err(e) = interaction
                                                .edit_response(
                                                    &ctx.http,
                                                    EditInteractionResponse::new().content(
                                                        "Failed to get audio command handler",
                                                    ),
                                                )
                                                .await
                                            {
                                                log::error!(
                                                    "Failed to edit original interaction response: {:?}",
                                                    e
                                                );
                                            }
                                            return;
                                        }
                                    }
                                };
                                audio_command_handler
                                    .write()
                                    .await
                                    .insert(guild_id.to_string(), tx);
                                if let Err(e) = interaction.delete_response(&ctx.http).await {
                                    log::error!("Error deleting interaction: {:?}", e);
                                }
                            }
                            Err(e) => {
                                log::error!("Failed to join channel: {:?}", e);
                                if let Err(e) = interaction
                                    .edit_response(
                                        &ctx.http,
                                        EditInteractionResponse::new()
                                            .content("Failed to join voice channel"),
                                    )
                                    .await
                                {
                                    log::error!(
                                        "Failed to edit original interaction response: {:?}",
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
            }
        } else if let Err(e) = interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("TELL ETHAN THIS SHOULD NEVER HAPPEN :("),
            )
            .await
        {
            log::error!("Failed to edit original interaction response: {:?}", e);
        }
    }
    fn name(&self) -> &str {
        "join"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &CommandInteraction) -> Result<()> {
        Ok(())
    }
}
