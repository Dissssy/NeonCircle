use common::anyhow::Result;
use common::audio::{AudioPromiseCommand, OrAuto};
use common::serenity::all::*;
use common::{log, CommandTrait};
#[derive(Debug, Clone)]
pub struct Command;
#[async_trait]
impl CommandTrait for Command {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .contexts(vec![InteractionContext::Guild])
                .description("Set the bot's bitrate")
                .set_options(vec![CreateCommandOption::new(
                    CommandOptionType::Integer,
                    "bitrate",
                    "the bitrate to set the bot to, otherwise auto",
                )
                .max_int_value(512_000)
                .min_int_value(512)]),
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
        let options = interaction.data.options();
        let option = match options.iter().find_map(|o| match o.name {
            "bitrate" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Integer(i)) => OrAuto::Specific(*i as i32),
            None => OrAuto::Auto,
            _ => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content("This command requires an option"),
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
            next_step
                .send_command_or_respond(
                    interaction,
                    guild_id,
                    AudioPromiseCommand::SetBitrate(option),
                )
                .await;
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
        "set_bitrate"
    }
    fn modal_names(&self) -> &'static [&'static str] {
        &["bitrate"]
    }
    async fn run_modal(&self, ctx: &Context, interaction: &ModalInteraction) -> Result<()> {
        let val = match interaction
            .data
            .components
            .first()
            .and_then(|ar| ar.components.first())
        {
            Some(ActionRowComponent::InputText(bitrate)) => match bitrate.value {
                Some(ref v) => v,
                None => {
                    log::error!("No value in bitrate modal");
                    return Ok(());
                }
            },
            Some(_) => {
                log::error!("Invalid components in bitrate modal");
                return Ok(());
            }
            None => {
                log::error!("No components in bitrate modal");
                return Ok(());
            }
        };
        let val = if val.is_empty() {
            OrAuto::Auto
        } else {
            OrAuto::Specific({
                let val = match val.parse::<i64>() {
                    Ok(v) => v,
                    Err(e) => {
                        log::info!("Failed to interactionarse bitrate: {}", e);
                        if let Err(e) = interaction
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
                            log::error!("Failed to send response: {}", e);
                        }
                        return Ok(());
                    }
                };
                if !(512..=512000).contains(&val) {
                    if let Err(e) = interaction
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
                        log::error!("Failed to send response: {}", e);
                    }
                    return Ok(());
                }
                val as i32
            })
        };
        let guild_id = match interaction.guild_id {
            Some(id) => id,
            None => {
                if let Err(e) = interaction
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
                    log::error!("Failed to send response: {}", e);
                    return Ok(());
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
            if let Err(e) = interaction.defer_ephemeral(&ctx.http).await {
                log::error!("Failed to defer: {:?}", e);
            }
            next_step
                .send_command_or_respond(
                    interaction,
                    guild_id,
                    AudioPromiseCommand::SetBitrate(val),
                )
                .await;
        } else {
            log::error!("Failed to get voice data");
            if let Err(e) = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Failed to get voice data")
                            .ephemeral(true),
                    ),
                )
                .await
            {
                log::error!("Failed to send response: {}", e);
            }
        }
        Ok(())
    }
}
