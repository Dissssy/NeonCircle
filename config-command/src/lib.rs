#![feature(try_blocks)]
use common::anyhow::Result;
use common::serenity::all::*;
mod default_volume;
mod empty_channel_timeout;
mod radio_source;
use common::{log, CommandTrait, SubCommandTrait};
mod read_titles;
mod transcribe;
pub struct Command {
    subcommands: Vec<Box<dyn SubCommandTrait>>,
}
impl Command {
    pub fn new() -> Self {
        Self {
            subcommands: vec![
                Box::new(empty_channel_timeout::Command),
                Box::new(default_volume::Command::new()),
                Box::new(read_titles::Command),
                Box::new(transcribe::Command::new()),
                Box::new(radio_source::Command::new()),
            ],
        }
    }
}
impl Default for Command {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl CommandTrait for Command {
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
        let member = match interaction.member {
            Some(ref member) => member,
            None => {
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
        };
        let (subcommand, opts) = match interaction.data.options().into_iter().find_map(|o| match o
            .value
        {
            ResolvedValue::SubCommand(opts) => Some((o.name, opts)),
            ResolvedValue::SubCommandGroup(opts) => Some((o.name, opts)),
            _ => None,
        }) {
            None => {
                todo!("Print the entire config as a fancy embed probably");
            }
            Some(s) => s,
        };
        for sc in &self.subcommands {
            if sc.command_name() == subcommand {
                if member
                    .permissions(&ctx.cache)
                    .map(|p| p.contains(sc.permissions()))
                    .unwrap_or(false)
                {
                    return sc.run(ctx, interaction, &opts).await;
                } else if let Err(e) = interaction
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
