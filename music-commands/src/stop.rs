use common::anyhow::Result;
use common::audio::AudioPromiseCommand;
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
                .description("Stop all playback"),
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
            next_step
                .send_command_or_respond(interaction, guild_id, AudioPromiseCommand::Stop(None))
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
        "stop"
    }
}
