use super::AudioPromiseCommand;
use anyhow::Result;
use serenity::all::*;
#[derive(Debug, Clone)]
pub struct Skip;
#[async_trait]
impl crate::CommandTrait for Skip {
    fn register(&self) -> CreateCommand {
        CreateCommand::new(self.name()).description("Skip the current song")
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
            next_step
                .send_command_or_respond(ctx, interaction, guild_id, AudioPromiseCommand::Skip)
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
        "skip"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &CommandInteraction) -> Result<()> {
        Ok(())
    }
}
