use common::anyhow::Result;
use common::audio::{AudioPromiseCommand, SpecificVolume};
use common::serenity::all::*;
use common::{log, CommandTrait};
#[derive(Debug, Clone)]
pub struct Command;
#[async_trait]
impl CommandTrait for Command {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .description("Change the volume of the bot for this session")
                .set_options(vec![CreateCommandOption::new(
                    CommandOptionType::Number,
                    "volume",
                    "Volume",
                )
                .max_number_value(100.0)
                .min_number_value(0.0)
                .required(true)]),
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
            "volume" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Number(f)) => *f,
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
        } as f32
            / 100.0;
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
                    AudioPromiseCommand::Volume(SpecificVolume::Current(option)),
                )
                .await;
        } else if let Err(e) = interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("This can only be used in a server"),
            )
            .await
        {
            log::error!("Failed to edit original interaction response: {:?}", e);
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "volume"
    }
    fn modal_names(&self) -> &'static [&'static str] {
        &["volume", "radiovolume"]
    }
    async fn run_modal(&self, ctx: &Context, interaction: &ModalInteraction) -> Result<()> {
        let raw = interaction.data.custom_id.as_str();
        let val = match interaction
            .data
            .components
            .first()
            .and_then(|ar| ar.components.first())
        {
            Some(ActionRowComponent::InputText(volume)) => match volume.value {
                Some(ref v) => v,
                None => {
                    log::error!("No value in volume modal");
                    return Ok(());
                }
            },
            Some(_) => {
                log::error!("Invalid components in volume modal");
                return Ok(());
            }
            None => {
                log::error!("No components in volume modal");
                return Ok(());
            }
        };
        let val = match val.parse::<f64>() {
            Ok(v) => v,
            Err(e) => {
                log::trace!("Failed to parse volume: {}", e);
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
        } as f32
            / 100.0;
        if !(0.0..=1.0).contains(&val) {
            log::trace!("Volume out of range: {}", val * 100.0);
            if let Err(e) = interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(format!("`{}` is outside 0-100", val * 100.0))
                            .ephemeral(true),
                    ),
                )
                .await
            {
                log::error!("Failed to send response: {}", e);
            }
            return Ok(());
        }
        let guild_id = match interaction.guild_id {
            Some(id) => id,
            None => {
                log::trace!("This can only be used in a server");
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
                            .create_response(
                                &ctx.http,
                                CreateInteractionResponse::Message(
                                    CreateInteractionResponseMessage::new()
                                        .content("Failed to get mutual channel")
                                        .ephemeral(true),
                                ),
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
                    match raw {
                        "volume" => AudioPromiseCommand::Volume(SpecificVolume::SongVolume(val)),
                        "radiovolume" => {
                            AudioPromiseCommand::Volume(SpecificVolume::RadioVolume(val))
                        }
                        uh => {
                            log::error!("How: {}", uh);
                            return Ok(());
                        }
                    },
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
