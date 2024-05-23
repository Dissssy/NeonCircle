use anyhow::Result;
use serenity::all::*;
pub struct Command;
#[async_trait]
impl super::SubCommandTrait for Command {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "The default volume for queued songs",
        )
        .add_sub_option(
            CreateCommandOption::new(
                CommandOptionType::Number,
                "new_volume",
                "The new volume (between 0 and 100)",
            )
            .max_number_value(100.0)
            .min_number_value(0.0),
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
            .find(|o| o.name == "new_volume")
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
                            .content({
                                let mut string = format!(
                                    "The current volume is {:.2}",
                                    config.get_default_song_volume() * 100.0
                                )
                                .trim_end_matches('0')
                                .trim_end_matches('.')
                                .to_owned();
                                string.push('%');
                                string
                            })
                            .ephemeral(true),
                    )
                    .await?;
            }
            Some(timeout) => {
                let config = config.set_default_song_volume(timeout as f32 / 100.0);
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content({
                                let mut string = format!(
                                    "The new volume is {:.2}",
                                    config.get_default_song_volume() * 100.0
                                )
                                .trim_end_matches('0')
                                .trim_end_matches('.')
                                .to_owned();
                                string.push('%');
                                string
                            })
                            .ephemeral(true),
                    )
                    .await?;
                config.write();
            }
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "default_song_volume"
    }
}
