use common::anyhow::Result;
use common::audio::{AudioCommandHandler, AudioPromiseCommand, MetaCommand};
use common::global_data::guild_config::GuildConfig;
use common::serenity::{
    all::*,
    futures::{stream::FuturesUnordered, StreamExt},
};
use common::SubCommandTrait;
use common::{log, tokio};
use std::sync::Arc;
pub struct Command {
    subcommands: Vec<Box<dyn SubCommandTrait>>,
}
impl Command {
    pub fn new() -> Self {
        Self {
            subcommands: vec![Box::new(Radio), Box::new(Song)],
        }
    }
}
#[async_trait]
impl SubCommandTrait for Command {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommandGroup,
            self.command_name(),
            "Configure the default volume settings for this guild",
        )
        .set_sub_options(self.subcommands.iter().map(|sc| sc.register_command()))
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
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
        let (subcommand, opts) = match options.iter().find_map(|o| match o.value {
            ResolvedValue::SubCommand(ref opts) => Some((o.name, opts)),
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
                    return sc.run(ctx, interaction, opts).await;
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
        "default_volume"
    }
    fn permissions(&self) -> Permissions {
        Permissions::empty() // we will check permissions in the subcommands
    }
}
pub struct Song;
#[async_trait]
impl SubCommandTrait for Song {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "The default volume for songs in this guild",
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
                ResolvedValue::Number(i) => Some(i),
                ResolvedValue::Integer(i) => Some(i as f64),
                _ => None,
            });
        let config = GuildConfig::get(guild_id);
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
            Some(volume) => {
                let config = config.set_default_song_volume(volume as f32 / 100.0);
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
                // we need to iterate over EVERY guild that has a connection, and update the song volume
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
                            AudioPromiseCommand::MetaCommand(MetaCommand::ChangeDefaultSongVolume(
                                volume as f32 / 100.0,
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
        "song"
    }
    fn permissions(&self) -> Permissions {
        Permissions::MANAGE_GUILD
    }
}
pub struct Radio;
#[async_trait]
impl SubCommandTrait for Radio {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "The default volume for the radio in this guild",
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
                ResolvedValue::Number(i) => Some(i),
                ResolvedValue::Integer(i) => Some(i as f64),
                _ => None,
            });
        let config = GuildConfig::get(guild_id);
        match timeout {
            None => {
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content({
                                let mut string = format!(
                                    "The current volume is {:.2}",
                                    config.get_default_radio_volume() * 100.0
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
            Some(volume) => {
                let config = config.set_default_radio_volume(volume as f32 / 100.0);
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content({
                                let mut string = format!(
                                    "The new volume is {:.2}",
                                    config.get_default_radio_volume() * 100.0
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
                // we need to iterate over EVERY guild that has a connection, and update the radio volume
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
                            AudioPromiseCommand::MetaCommand(
                                MetaCommand::ChangeDefaultRadioVolume(volume as f32 / 100.0),
                            ),
                        ));
                        res.push(rx);
                    }
                    while let Some(r) = res.next().await {
                        if let Err(e) = r {
                            log::error!("Failed to change default radio volume: {:?}", e);
                        }
                    }
                });
            }
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "radio"
    }
    fn permissions(&self) -> Permissions {
        Permissions::MANAGE_GUILD
    }
}
