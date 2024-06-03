use anyhow::Result;
use common::log;
use common::serenity::all::*;
pub struct Command {
    subcommands: Vec<Box<dyn crate::traits::SubCommandTrait>>,
}
impl Command {
    pub fn new() -> Self {
        Self {
            subcommands: vec![],
        }
    }
}
#[async_trait]
impl crate::traits::CommandTrait for Command {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .description("Manage your reminders")
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
        let (subcommand, opts) = match interaction.data.options().into_iter().find_map(|o| match o
            .value
        {
            ResolvedValue::SubCommand(opts) => Some((o.name, opts)),
            ResolvedValue::SubCommandGroup(opts) => Some((o.name, opts)),
            _ => None,
        }) {
            None => {
                unreachable!();
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
        "reminder"
    }
}
