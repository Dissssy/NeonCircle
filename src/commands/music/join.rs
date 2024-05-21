use super::{
    mainloop::{ControlData, Log},
    settingsdata::SettingsData,
    transcribe::TranscriptionThread,
    AudioHandler, AudioPromiseCommand,
};
use crate::global_data::VoiceAction;
use anyhow::Result;
use serenity::all::*;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
#[derive(Debug, Clone)]
pub struct Join;
#[async_trait]
impl crate::CommandTrait for Join {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(CreateCommand::new(self.command_name()).description("Join the voice channel"))
    }
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
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
                return Ok(());
            }
        };
        if let Some(member) = interaction.member.as_ref() {
            let next_step = match crate::global_data::mutual_channel(&guild_id, &member.user.id)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    log::error!("Failed to get mutual channel: {:?}", e);
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content("Failed to get mutual channel"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return Ok(());
                }
            };
            match next_step.action {
                VoiceAction::NoRemaining => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content("No satellites available to join, use /feedback to request more (and dont forget to donate if you can! :D)"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return Ok(());
                }
                VoiceAction::InviteSatellite(invite) => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content(format!(
                                "There are no satellites available, [use this link to invite one]({})\nPlease ensure that all satellites have permission to view the voice channel you're in.",
                                invite
                            )),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return Ok(());
                }
                VoiceAction::UserNotConnected => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content("You're not in a voice channel"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return Ok(());
                }
                VoiceAction::SatelliteInVcWithUser(_channel, _ctx) => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new()
                                .content("A satellite is already in a voice channel with you"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return Ok(());
                }
                VoiceAction::SatelliteShouldJoin(channel, satellite_ctx) => {
                    let manager = match songbird::get(&satellite_ctx).await {
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
                            return Ok(());
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
                                    return Ok(());
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
                                let msg = match channel
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
                                        return Ok(());
                                    }
                                };
                                let messageref = super::MessageReference::new(
                                    ctx.http.clone(),
                                    ctx.cache.clone(),
                                    guild_id,
                                    channel,
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
                                    None => return Ok(()),
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
                                        return Ok(());
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
                                            return Ok(());
                                        }
                                    }
                                };
                                audio_command_handler.write().await.insert(channel, tx);
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
        Ok(())
    }
    fn command_name(&self) -> &str {
        "join"
    }
}
