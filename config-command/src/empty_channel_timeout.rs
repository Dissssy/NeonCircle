use std::sync::Arc;
use std::time::Duration;

use common::anyhow::Result;
use common::audio::{AudioCommandHandler, AudioPromiseCommand, MetaCommand};
use common::serenity::all::*;
use common::utils::friendly_duration;
use common::{log, tokio, SubCommandTrait};
use futures::stream::FuturesUnordered;
use futures::StreamExt as _;
use long_term_storage::Guild;
pub struct Command;
#[async_trait]
impl SubCommandTrait for Command {
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
        let mut config = match Guild::load(guild_id).await {
            Ok(c) => c,
            Err(e) => {
                log::error!("Failed to load guild: {:?}", e);
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content("Failed to load guild")
                            .ephemeral(true),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };
        match timeout {
            None => {
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content(format!(
                                "The current timeout is {}",
                                friendly_duration(&config.empty_channel_timeout)
                            ))
                            .ephemeral(true),
                    )
                    .await?;
            }
            Some(timeout) => {
                config.empty_channel_timeout = tokio::time::Duration::from_secs(timeout as u64);
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content(format!(
                                "The new timeout is {}",
                                friendly_duration(&config.empty_channel_timeout)
                            ))
                            .ephemeral(true),
                    )
                    .await?;
                if let Err(e) = config.save().await {
                    log::error!("Failed to save new value: {:?}", e);
                    if let Err(e) = interaction
                        .create_followup(
                            &ctx.http,
                            CreateInteractionResponseFollowup::new()
                                .content("Failed to save new value")
                                .ephemeral(true),
                        )
                        .await
                    {
                        log::error!("Failed to send response: {}", e);
                    }
                }
                // we need to iterate over EVERY guild that has a connection, and update the timeout
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
                            AudioPromiseCommand::MetaCommand(MetaCommand::ChangeAloneTimeout(
                                Duration::from_secs(timeout as u64),
                            )),
                        ));
                        res.push(rx);
                    }
                    while let Some(r) = res.next().await {
                        if let Err(e) = r {
                            log::error!("Failed to change default song volume: {:?}", e);
                        }
                    }
                });
            }
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "empty_channel_timeout"
    }
    fn permissions(&self) -> Permissions {
        Permissions::MANAGE_GUILD
    }
}
