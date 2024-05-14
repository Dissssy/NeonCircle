use super::AudioPromiseCommand;
use anyhow::Error;
use serenity::all::*;
#[derive(Debug, Clone)]
pub struct SetBitrate;
#[async_trait]
impl crate::CommandTrait for SetBitrate {
    fn register(&self) -> CreateCommand {
        CreateCommand::new(self.name())
            .description("Set the bot's bitrate")
            .set_options(vec![CreateCommandOption::new(
                CommandOptionType::Integer,
                "bitrate",
                "the bitrate to set the bot to, otherwise auto",
            )
            .max_int_value(512_000)
            .min_int_value(512)])
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
        let options = interaction.data.options();
        let option = match options.iter().find_map(|o| match o.name {
            "bitrate" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Integer(i)) => super::OrAuto::Specific(*i),
            None => super::OrAuto::Auto,
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
                let mut v = v.lock().await;
                v.mutual_channel(ctx, &guild_id, &member.user.id)
            };
            next_step
                .send_command_or_respond(
                    ctx,
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
    }
    fn name(&self) -> &str {
        "set_bitrate"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &CommandInteraction) -> Result<(), Error> {
        Ok(())
    }
}
