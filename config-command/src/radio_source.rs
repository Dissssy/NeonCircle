use common::anyhow::{self, Result};
use common::audio::{AudioCommandHandler, AudioPromiseCommand, MetaCommand};
use common::radio::{RadioData, RadioDataKind};
use common::serenity::{
    all::*,
    futures::{stream::FuturesUnordered, StreamExt},
};
use common::{log, tokio, SubCommandTrait};
use long_term_storage::Guild;
use serde::Deserialize;
use std::sync::Arc;
pub struct Command {
    subcommands: Vec<Box<dyn SubCommandTrait>>,
}
impl Command {
    pub fn new() -> Self {
        Self {
            subcommands: vec![Box::new(StreamUrl), Box::new(DataUrl), Box::new(Reset)],
        }
    }
}
#[async_trait]
impl SubCommandTrait for Command {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommandGroup,
            self.command_name(),
            "Configure the radio source settings",
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
        "radio_source"
    }
    fn permissions(&self) -> Permissions {
        Permissions::empty() // we will check permissions in the subcommands (both will require MANAGE_GUILD)
    }
}
// /config radio_source stream_url <optional url> - Set the stream url for the radio, if no url is provided, the current url will be displayed
struct StreamUrl;
#[async_trait]
impl SubCommandTrait for StreamUrl {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "stream_url",
            "Set the stream url for the radio",
        )
        .set_sub_options(vec![CreateCommandOption::new(
            CommandOptionType::String,
            "url",
            "The stream url to set",
        )
        .required(false)])
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        let url: Option<Arc<str>> = options.iter().find_map(|o| match o.value {
            ResolvedValue::String(s) => Some(s.into()),
            _ => None,
        });
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
        match url {
            None => {
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content(format!(
                                "Stream url is currently set to `{}`",
                                match config.radio_audio_url {
                                    Some(url) => url.to_string(),
                                    None => "Default".to_owned(),
                                }
                            ))
                            .ephemeral(true),
                    )
                    .await?;
            }
            Some(value) => {
                if let Err(e) = validate_stream(&value).await {
                    interaction
                        .create_followup(
                            &ctx.http,
                            CreateInteractionResponseFollowup::new()
                                .content(format!("Failed to get audio stream url: {}", e))
                                .ephemeral(true),
                        )
                        .await?;
                    return Ok(());
                };
                let note: Option<String> = try {
                    let url = config.radio_audio_url?;
                    let d = RadioData::get(url.as_ref()).await.ok()?;
                    (d.kind() == RadioDataKind::IceCast).then_some(())?;
                    let search_for = value.split('/').last()?;
                    format!(
                        "\n\nNote:\n{}\n{}\n{} `{}`\n{}",
                        "Your data url was detected to be IceCast",
                        "IceCast does not have endpoints for specific audio streams",
                        "Please ensure that one of the `listenurl` entries in the output of the url you provided ends with",
                        search_for,
                        "Otherwise the data will always be unknown"
                    )
                };
                config.radio_audio_url = Some(Arc::clone(&value));
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content(format!(
                                "Stream url is now `{}`{}",
                                value,
                                note.unwrap_or_default()
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
                // we need to iterate over EVERY guild that has a connection, and update the stream url
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
                            AudioPromiseCommand::MetaCommand(MetaCommand::ChangeRadioAudioUrl(
                                Arc::clone(&value),
                            )),
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
        "stream_url"
    }
    fn permissions(&self) -> Permissions {
        Permissions::MANAGE_GUILD
    }
}
// /config radio_source data_url <optional url> - Set the data url for the radio, if no url is provided, the current url will be displayed (supports azuracast and icecast)
struct DataUrl;
#[async_trait]
impl SubCommandTrait for DataUrl {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "data_url",
            "Set the data url for the radio",
        )
        .set_sub_options(vec![CreateCommandOption::new(
            CommandOptionType::String,
            "url",
            "The data url to set",
        )
        .required(false)])
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        let url: Option<Arc<str>> = options.iter().find_map(|o| match o.value {
            ResolvedValue::String(s) => Some(s.into()),
            _ => None,
        });
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
        match url {
            None => {
                interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content(format!(
                                "Data url is currently set to `{}`",
                                match config.radio_data_url {
                                    Some(url) => url.to_string(),
                                    None => "Default".to_owned(),
                                }
                            ))
                            .ephemeral(true),
                    )
                    .await?;
            }
            Some(value) => {
                match RadioData::get(value.as_ref()).await {
                    Err(e) => {
                        interaction
                            .create_followup(
                                &ctx.http,
                                CreateInteractionResponseFollowup::new()
                                    .content(format!(
                                        "Failed to get radio data for that url\n{}\n```\n{}```",
                                        "Please ensure that the url is a valid azuracast or icecast json endpoint",
                                        e
                                    ))
                                    .ephemeral(true),
                            )
                            .await?;
                        return Ok(());
                    }
                    Ok(d) => {
                        let note: Option<String> = try {
                            let url = config.radio_data_url?;
                            (d.kind() == RadioDataKind::IceCast).then_some(())?;
                            let search_for = url.split('/').last()?;
                            format!(
                                "\n\nNote:\n{}\n{}\n{} `{}`\n{}",
                                "Your data url was detected to be IceCast",
                                "IceCast does not have endpoints for specific audio streams",
                                "Please ensure that one of the `listenurl` entries in the output of the url you provided ends with",
                                search_for,
                                "Otherwise the data will always be unknown"
                            )
                        };
                        config.radio_data_url = Some(Arc::clone(&value));
                        interaction
                            .create_followup(
                                &ctx.http,
                                CreateInteractionResponseFollowup::new()
                                    .content(format!(
                                        "Data url is now `{}`{}",
                                        value,
                                        note.unwrap_or_default()
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
                        // we need to iterate over EVERY guild that has a connection, and update the data url
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
                                        MetaCommand::ChangeRadioDataUrl(Arc::clone(&value)),
                                    ),
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
            }
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "data_url"
    }
    fn permissions(&self) -> Permissions {
        Permissions::MANAGE_GUILD
    }
}
// /config radio_source reset - Reset the radio source settings to the default values (stream url and data url will be cleared)
struct Reset;
#[async_trait]
impl SubCommandTrait for Reset {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            "reset",
            "Reset the radio source settings to the default values",
        )
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        _options: &[ResolvedOption],
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
        config.radio_audio_url = None;
        config.radio_data_url = None;
        interaction
            .create_followup(
                &ctx.http,
                CreateInteractionResponseFollowup::new()
                    .content("Radio source settings have been reset, enjoy the default tunes!")
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
        // we need to iterate over EVERY guild that has a connection, and update the stream url
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
                    AudioPromiseCommand::MetaCommand(MetaCommand::ResetCustomRadioData),
                ));
                res.push(rx);
            }
            while let Some(r) = res.next().await {
                if let Err(e) = r {
                    log::error!("Failed to change read titles: {:?}", e);
                }
            }
        });
        Ok(())
    }
    fn command_name(&self) -> &str {
        "reset"
    }
    fn permissions(&self) -> Permissions {
        Permissions::MANAGE_GUILD
    }
}
async fn validate_stream(url: &str) -> Result<()> {
    // use the yt-dlp cli to validate that the url is a valid audio stream
    let output = tokio::process::Command::new("yt-dlp")
        .arg("--no-playlist")
        .args(["-O", "%(.{formats})j"])
        .arg("--force-ipv4")
        .arg(url)
        .output()
        .await?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to validate audio url: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    // take the first newline separated line from the output
    let output = String::from_utf8_lossy(&output.stdout);
    let output = output
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Failed to get output from yt-dlp"))?;
    // it's json, we'll deserialize it to get the url of the first stream on the page
    let formats: AudioFormats = serde_json::from_str(output)?;
    formats
        .formats
        .iter()
        .find(|f| f.audio_ext.is_some())
        .ok_or_else(|| anyhow::anyhow!("No audio formats found in yt-dlp output"))?;
    Ok(())
}
#[derive(Deserialize)]
struct AudioFormat {
    // url: Arc<str>,
    // "audio_ext": "m4a"
    audio_ext: Option<Arc<str>>,
}
#[derive(Deserialize)]
struct AudioFormats {
    formats: Vec<AudioFormat>,
}
