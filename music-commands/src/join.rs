use super::{
    mainloop::{ControlData, Log},
    settingsdata::SettingsData,
    transcribe::TranscriptionThread,
    AudioHandler,
};
use common::{
    anyhow::Result,
    audio::{AudioCommandHandler, AudioPromiseCommand, SenderAndGuildId},
    songbird, tokio,
};
use common::{global_data::voice_data::VoiceAction, serenity::all::*};
use common::{log, CommandTrait};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
#[derive(Debug, Clone)]
pub struct Command;
#[async_trait]
impl CommandTrait for Command {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .contexts(vec![InteractionContext::Guild])
                .description("Join the voice channel"),
        )
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
            let next_step =
                match common::global_data::voice_data::mutual_channel(&guild_id, &member.user.id)
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        log::error!("Failed to get mutual channel: {:?}", e);
                        if let Err(e) = interaction
                            .edit_response(
                                &ctx.http,
                                EditInteractionResponse::new()
                                    .content("Failed to get mutual channel"),
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
                                    oneshot::Sender<Arc<str>>,
                                    AudioPromiseCommand,
                                )>();
                                let transcription = TranscriptionThread::new(
                                    Arc::clone(&call),
                                    ctx.clone(),
                                    tx.clone(),
                                )
                                .await;
                                let msg = match channel
                                    .send_message(
                                        &ctx.http,
                                        CreateMessage::new()
                                            .content("<a:earloading:979852072998543443>")
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
                                    Arc::clone(&ctx.http),
                                    Arc::clone(&ctx.cache),
                                    guild_id,
                                    channel,
                                    msg,
                                );
                                let cfg = common::get_config();
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
                                let settings = match SettingsData::new(guild_id).await {
                                    Ok(v) => v,
                                    Err(e) => {
                                        log::error!("Failed to get settings: {:?}", e);
                                        if let Err(e) = interaction
                                            .edit_response(
                                                &ctx.http,
                                                EditInteractionResponse::new()
                                                    .content("Failed to get settings"),
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
                                // let em = match super::get_transcribe_channel_handler(ctx, &guild_id)
                                //     .await
                                // {
                                //     Ok(v) => v,
                                //     Err(e) => {
                                //         log::error!(
                                //             "Failed to get transcribe channel handler: {:?}",
                                //             e
                                //         );
                                //         if let Err(e) = interaction
                                //             .edit_response(
                                //                 &ctx.http,
                                //                 EditInteractionResponse::new()
                                //                     .content("Failed to get handler"),
                                //             )
                                //             .await
                                //         {
                                //             log::error!(
                                //     "Failed to edit original interaction response: {:?}",
                                //     e
                                // );
                                //         }
                                //         return Ok(());
                                //     }
                                // };
                                // if let Err(e) = em.write().await.register(channel).await {
                                //     log::error!("Error registering channel: {:?}", e);
                                // }
                                let this_bot_id = ctx.cache.current_user().id;
                                let audio_command_handler = {
                                    let read_lock = ctx.data.read().await;
                                    match read_lock.get::<AudioCommandHandler>() {
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

                                let handle = {
                                    let ctx = ctx.clone();
                                    let ach = Arc::clone(&audio_command_handler);
                                    tokio::task::spawn(async move {
                                        let control = ControlData {
                                            call,
                                            rx,
                                            msg: messageref,
                                            nothing_uri: nothing_path,
                                            settings,
                                            log: Log::new(format!("{}-{}", guild_id, channel)),
                                            // transcribe: em,
                                        };
                                        super::mainloop::the_lüüp(
                                            // cfg.looptime,
                                            transcription,
                                            control,
                                            this_bot_id,
                                            ctx,
                                            channel,
                                            ach,
                                        )
                                        .await;
                                    })
                                };
                                audio_handler.write().await.insert(channel, handle);
                                audio_command_handler
                                    .write()
                                    .await
                                    .insert(channel, SenderAndGuildId::new(tx, guild_id));
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
