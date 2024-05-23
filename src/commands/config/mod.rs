use anyhow::Result;
use serenity::all::*;
mod default_radio_volume;
mod default_song_volume;
mod empty_channel_timeout;
pub struct Command {
    subcommands: Vec<Box<dyn SubCommandTrait>>,
}
impl Command {
    pub fn new() -> Self {
        Self {
            subcommands: vec![
                Box::new(empty_channel_timeout::Command),
                Box::new(default_song_volume::Command),
                Box::new(default_radio_volume::Command),
            ],
        }
    }
}
#[async_trait]
impl crate::CommandTrait for Command {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .description("Configure this guild's settings")
                .set_options(
                    self.subcommands
                        .iter()
                        .map(|sc| sc.register_command())
                        .collect(),
                ),
        )
    }
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        if let Err(e) = interaction.defer_ephemeral(&ctx.http).await {
            log::error!("Failed to send response: {}", e);
        }
        if let Some(ref member) = interaction.member {
            if !member.permissions(&ctx.cache)?.manage_guild() {
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .ephemeral(true)
                            .content("You do not have permission to run this command"),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        } else {
            if let Err(e) = interaction
                .create_followup(
                    &ctx.http,
                    CreateInteractionResponseFollowup::new()
                        .ephemeral(true)
                        .content("This command can only be run in a guild"),
                )
                .await
            {
                log::error!("Failed to send response: {}", e);
            }
            return Ok(());
        }
        let (subcommand, opts) = match interaction.data.options().into_iter().find_map(|o| match o
            .value
        {
            ResolvedValue::SubCommand(opts) => Some((o.name, opts)),
            _ => None,
        }) {
            None => {
                todo!("Print the entire config as a fancy embed probably");
            }
            Some(s) => s,
        };
        for sc in &self.subcommands {
            if sc.command_name() == subcommand {
                return sc.run(ctx, interaction, &opts).await;
            }
        }
        if let Err(e) = interaction
            .create_followup(
                &ctx.http,
                CreateInteractionResponseFollowup::new()
                    .ephemeral(true)
                    .content("Invalid subcommand"),
            )
            .await
        {
            log::error!("Failed to send response: {}", e);
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "config"
    }
}
#[async_trait]
trait SubCommandTrait
where
    Self: Send + Sync,
{
    fn register_command(&self) -> CreateCommandOption;
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()>;
    fn command_name(&self) -> &str;
}
