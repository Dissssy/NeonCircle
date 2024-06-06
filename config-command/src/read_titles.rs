use common::anyhow::Result;
use common::audio::{AudioCommandHandler, AudioPromiseCommand, MetaCommand};
use common::serenity::{
    all::*,
    futures::{stream::FuturesUnordered, StreamExt as _},
};
use common::{log, tokio, SubCommandTrait};
use std::sync::Arc;
pub struct Command;
#[async_trait]
impl SubCommandTrait for Command {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "Whether to read the titles of songs",
        )
        .add_sub_option(CreateCommandOption::new(
            CommandOptionType::Boolean,
            "new_value",
            "The new value",
        ))
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
        let read_titles = options
            .iter()
            .find(|o| o.name == "new_value")
            .and_then(|o| match o.value {
                ResolvedValue::Boolean(i) => Some(i),
                _ => None,
            });
        let config = common::global_data::guild_config::GuildConfig::get(guild_id);
        match read_titles {
            None => {
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content(format!(
                                "Currently reading titles by default: {}",
                                config.get_read_titles_by_default()
                            )) // Remove trailing zeros and periods
                            .ephemeral(true),
                    )
                    .await?;
            }
            Some(value) => {
                let config = config.set_read_titles_by_default(value);
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content(format!(
                                "Reading titles by default is now {}",
                                config.get_read_titles_by_default()
                            ))
                            .ephemeral(true),
                    )
                    .await?;
                config.write();
                // we need to iterate over EVERY guild that has a connection, and update the read_titles value
                let connection_handler = {
                    let data = ctx.data.read().await;
                    match data.get::<AudioCommandHandler>() {
                        Some(v) => Arc::clone(v),
                        None => {
                            log::error!("Failed to get audio command handler");
                            return Ok(());
                        }
                    }
                };
                tokio::task::spawn(async move {
                    let mut map = connection_handler.write().await;
                    let mut res = FuturesUnordered::new();
                    for sender in map.values_mut() {
                        if sender.guild_id != guild_id {
                            continue;
                        }
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        let _ = sender.send((
                            tx,
                            AudioPromiseCommand::MetaCommand(MetaCommand::ChangeReadTitles(value)),
                        ));
                        res.push(rx);
                    }
                    while let Some(r) = res.next().await {
                        if let Err(e) = r {
                            log::error!("Failed to change read titles: {:?}", e);
                        }
                    }
                });
            }
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "read_titles"
    }
    fn permissions(&self) -> Permissions {
        Permissions::MANAGE_GUILD
    }
}
