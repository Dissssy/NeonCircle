use super::AudioPromiseCommand;
use anyhow::Error;
use serenity::all::*;

#[derive(Debug, Clone)]
pub struct Pause;

#[async_trait]
impl crate::CommandTrait for Pause {
    fn register(&self) -> CreateCommand {
        CreateCommand::new(self.name()).description("Pause playback")
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
            eprintln!("Failed to create interaction response: {:?}", e);
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
                    eprintln!("Failed to edit original interaction response: {:?}", e);
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

            next_step.send_command_or_respond(
                ctx,
                interaction,
                guild_id,
                AudioPromiseCommand::Paused(super::OrToggle::Specific(true)),
            );
        } else if let Err(e) = interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("TELL ETHAN THIS SHOULD NEVER HAPPEN :("),
            )
            .await
        {
            eprintln!("Failed to edit original interaction response: {:?}", e);
        }
    }
    fn name(&self) -> &str {
        "pause"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &CommandInteraction) -> Result<(), Error> {
        Ok(())
    }
}
