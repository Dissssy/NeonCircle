use crate::commands::music::mainloop::friendly_duration;
use anyhow::Result;
use serenity::all::*;
pub struct Command;
#[async_trait]
impl super::SubCommandTrait for Command {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "The timeout for the bots to leave empty channels",
        )
        .add_sub_option(
            CreateCommandOption::new(
                CommandOptionType::Integer,
                "new_timeout",
                "The timeout in seconds (between 0 and 600)",
            )
            .max_int_value(600)
            .min_int_value(0),
        )
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        let guild_id = match interaction.guild_id {
            Some(g) => g,
            None => {
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content("This command can only be used in a server")
                            .ephemeral(true),
                    )
                    .await?;
                return Ok(());
            }
        };
        let timeout = options
            .iter()
            .find(|o| o.name == "new_timeout")
            .and_then(|o| match o.value {
                ResolvedValue::Integer(i) => Some(i),
                _ => None,
            });
        let config = crate::global_data::guild_config::GuildConfig::get(guild_id);
        match timeout {
            None => {
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content(format!(
                                "The current timeout is {}",
                                friendly_duration(&config.get_empty_channel_timeout())
                            ))
                            .ephemeral(true),
                    )
                    .await?;
            }
            Some(timeout) => {
                let timeout = std::time::Duration::from_secs(timeout as u64);
                let config = config.set_empty_channel_timeout(timeout);
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content(format!(
                                "The new timeout is {}",
                                friendly_duration(&config.get_empty_channel_timeout())
                            ))
                            .ephemeral(true),
                    )
                    .await?;
                config.write();
            }
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "empty_channel_timeout"
    }
}
