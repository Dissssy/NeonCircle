use super::AudioPromiseCommand;
use anyhow::Result;
use serenity::all::*;
#[derive(Debug, Clone)]
pub struct Remove;
#[async_trait]
impl crate::CommandTrait for Remove {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .description("Remove a song from the queue")
                .set_options(vec![CreateCommandOption::new(
                    CommandOptionType::Integer,
                    "index",
                    "Index of song to remove",
                )
                .min_int_value(1)
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
            "index" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Integer(i)) => *i,
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
            next_step
                .send_command_or_respond(
                    interaction,
                    guild_id,
                    AudioPromiseCommand::Remove(option as usize),
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
        "remove"
    }
}
