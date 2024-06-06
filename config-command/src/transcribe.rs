use common::anyhow::Result;
use common::serenity::all::*;
use common::{log, SubCommandTrait};
pub struct Command {
    subcommands: Vec<Box<dyn SubCommandTrait>>,
}
impl Command {
    pub fn new() -> Self {
        Self {
            subcommands: vec![
                Box::new(List),
                Box::new(Add),
                Box::new(Remove),
                Box::new(Clear),
            ],
        }
    }
}
#[async_trait]
impl SubCommandTrait for Command {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommandGroup,
            self.command_name(),
            "Transcription settings",
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
        "transcribe"
    }
    fn permissions(&self) -> Permissions {
        Permissions::empty() // we will check permissions in the subcommands
    }
}
// - list (lists all transcribed channels for a specific voice channel)
struct List;
#[async_trait]
impl SubCommandTrait for List {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "List all transcribed channels for a specific voice channel",
        )
        .add_sub_option(
            CreateCommandOption::new(
                CommandOptionType::Channel,
                "voice_channel",
                "The voice channel to list transcribed channels for",
            )
            .required(true),
        )
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        let voice_channel = match options.iter().find_map(|o| match o.name {
            "voice_channel" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Channel(c)) => c,
            _ => {
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .ephemeral(true)
                            .content("Invalid voice channel"),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };
        // ensure it's actually a voice channel
        if voice_channel.kind != ChannelType::Voice {
            if let Err(e) = interaction
                .create_followup(
                    &ctx.http,
                    CreateInteractionResponseFollowup::new()
                        .ephemeral(true)
                        .content("Invalid channel, it is not a voice channel"),
                )
                .await
            {
                log::error!("Failed to send response: {}", e);
            }
            return Ok(());
        }
        let channels = common::global_data::transcribe::list_all_channels(voice_channel.id).await;
        if channels.is_empty() {
            if let Err(e) = interaction
                .create_followup(
                    &ctx.http,
                    CreateInteractionResponseFollowup::new()
                        .ephemeral(true)
                        .content("No transcribed channels for this voice channel"),
                )
                .await
            {
                log::error!("Failed to send response: {}", e);
            }
            return Ok(());
        } else {
            let mut content = format!("Transcribed channels for {}:\n", voice_channel.id.mention());
            for channel in channels {
                content.push_str(&format!("{}\n", channel.mention(),));
            }
            if let Err(e) = interaction
                .create_followup(
                    &ctx.http,
                    CreateInteractionResponseFollowup::new()
                        .ephemeral(true)
                        .content(content),
                )
                .await
            {
                log::error!("Failed to send response: {}", e);
            }
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "list"
    }
    fn permissions(&self) -> Permissions {
        Permissions::empty() // no permissions required
    }
}
// - add (adds a channel to the transcription list for a specific voice channel) (requires manage channels)
struct Add;
#[async_trait]
impl SubCommandTrait for Add {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "Add a channel to the transcription list for a specific voice channel",
        )
        .add_sub_option(
            CreateCommandOption::new(
                CommandOptionType::Channel,
                "voice_channel",
                "The voice channel to add the transcribed channel to",
            )
            .required(true),
        )
        .add_sub_option(
            CreateCommandOption::new(
                CommandOptionType::Channel,
                "transcribed_channel",
                "The channel to transcribe in the voice channel",
            )
            .required(true),
        )
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        let voice_channel = match options.iter().find_map(|o| match o.name {
            "voice_channel" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Channel(c)) => c,
            _ => {
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .ephemeral(true)
                            .content("Invalid voice channel"),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };
        // ensure it's actually a voice channel
        if voice_channel.kind != ChannelType::Voice {
            if let Err(e) = interaction
                .create_followup(
                    &ctx.http,
                    CreateInteractionResponseFollowup::new()
                        .ephemeral(true)
                        .content("Invalid channel, it is not a voice channel"),
                )
                .await
            {
                log::error!("Failed to send response: {}", e);
            }
            return Ok(());
        }
        let transcribed_channel = match options.iter().find_map(|o| match o.name {
            "transcribed_channel" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Channel(c)) => c,
            _ => {
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .ephemeral(true)
                            .content("Invalid transcribed channel"),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };
        // it can be voice or text channel both work fine
        common::global_data::transcribe::set_channel(
            voice_channel.id,
            transcribed_channel.id,
            true,
        )
        .await;
        if let Err(e) = interaction
            .create_followup(
                &ctx.http,
                CreateInteractionResponseFollowup::new()
                    .ephemeral(true)
                    .content(format!(
                        "Added {} to the transcription list for {}",
                        transcribed_channel.id.mention(),
                        voice_channel.id.mention()
                    )),
            )
            .await
        {
            log::error!("Failed to send response: {}", e);
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "add"
    }
    fn permissions(&self) -> Permissions {
        Permissions::MANAGE_CHANNELS
    }
}
// - remove (removes a channel from the transcription list for a specific voice channel) (requires manage channels)
struct Remove;
#[async_trait]
impl SubCommandTrait for Remove {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "Remove a channel from the transcription list for a specific voice channel",
        )
        .add_sub_option(
            CreateCommandOption::new(
                CommandOptionType::Channel,
                "voice_channel",
                "The voice channel to remove the transcribed channel from",
            )
            .required(true),
        )
        .add_sub_option(
            CreateCommandOption::new(
                CommandOptionType::Channel,
                "transcribed_channel",
                "The channel to remove from the voice channel",
            )
            .required(true),
        )
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        let voice_channel = match options.iter().find_map(|o| match o.name {
            "voice_channel" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Channel(c)) => c,
            _ => {
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .ephemeral(true)
                            .content("Invalid voice channel"),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };
        // ensure it's actually a voice channel
        if voice_channel.kind != ChannelType::Voice {
            if let Err(e) = interaction
                .create_followup(
                    &ctx.http,
                    CreateInteractionResponseFollowup::new()
                        .ephemeral(true)
                        .content("Invalid channel, it is not a voice channel"),
                )
                .await
            {
                log::error!("Failed to send response: {}", e);
            }
            return Ok(());
        }
        let transcribed_channel = match options.iter().find_map(|o| match o.name {
            "transcribed_channel" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Channel(c)) => c,
            _ => {
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .ephemeral(true)
                            .content("Invalid transcribed channel"),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };
        // it can be voice or text channel both work fine
        common::global_data::transcribe::set_channel(
            voice_channel.id,
            transcribed_channel.id,
            false,
        )
        .await;
        if let Err(e) = interaction
            .create_followup(
                &ctx.http,
                CreateInteractionResponseFollowup::new()
                    .ephemeral(true)
                    .content(format!(
                        "Removed {} from the transcription list for {}",
                        transcribed_channel.id.mention(),
                        voice_channel.id.mention()
                    )),
            )
            .await
        {
            log::error!("Failed to send response: {}", e);
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "remove"
    }
    fn permissions(&self) -> Permissions {
        Permissions::MANAGE_CHANNELS
    }
}
// - clear (clears the transcription list for a specific voice channel) (requires manage channels)
struct Clear;
#[async_trait]
impl SubCommandTrait for Clear {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "Clear the transcription list for a specific voice channel",
        )
        .add_sub_option(
            CreateCommandOption::new(
                CommandOptionType::Channel,
                "voice_channel",
                "The voice channel to clear the transcription list for",
            )
            .required(true),
        )
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        let voice_channel = match options.iter().find_map(|o| match o.name {
            "voice_channel" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Channel(c)) => c,
            _ => {
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .ephemeral(true)
                            .content("Invalid voice channel"),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };
        // ensure it's actually a voice channel
        if voice_channel.kind != ChannelType::Voice {
            if let Err(e) = interaction
                .create_followup(
                    &ctx.http,
                    CreateInteractionResponseFollowup::new()
                        .ephemeral(true)
                        .content("Invalid channel, it is not a voice channel"),
                )
                .await
            {
                log::error!("Failed to send response: {}", e);
            }
            return Ok(());
        }
        common::global_data::transcribe::clear_channel(voice_channel.id).await;
        if let Err(e) = interaction
            .create_followup(
                &ctx.http,
                CreateInteractionResponseFollowup::new()
                    .ephemeral(true)
                    .content(format!(
                        "Cleared the transcription list for {}",
                        voice_channel.id.mention()
                    )),
            )
            .await
        {
            log::error!("Failed to send response: {}", e);
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "clear"
    }
    fn permissions(&self) -> Permissions {
        Permissions::MANAGE_CHANNELS
    }
}
