use super::music::VideoType;
use crate::video::MediaType;
use anyhow::Result;
use serenity::all::*;
#[derive(Debug, Clone)]
pub struct Video;
#[async_trait]
impl crate::traits::CommandTrait for Video {
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        dotheroar(ctx, interaction).await;
        Ok(())
    }
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .description("Embed a video using ytdl")
                .set_options(vec![
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "video_url",
                        "The url of the video to embed",
                    )
                    .required(true),
                    CreateCommandOption::new(
                        CommandOptionType::Boolean,
                        "spoiler",
                        "Whether to spoiler the video",
                    )
                    .required(false),
                ]),
        )
    }
    fn command_name(&self) -> &str {
        "embed_video"
    }
}
#[derive(Debug, Clone)]
pub struct Audio;
#[async_trait]
impl crate::traits::CommandTrait for Audio {
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        dotheroar(ctx, interaction).await;
        Ok(())
    }
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .description("Embed some audio using ytdl")
                .set_options(vec![CreateCommandOption::new(
                    CommandOptionType::String,
                    "audio_url",
                    "The url of the audio to embed",
                )
                .required(true)]),
        )
    }
    fn command_name(&self) -> &str {
        "embed_audio"
    }
}
#[allow(dead_code)]
fn get_command_data_option_name(option: &CommandDataOptionValue) -> String {
    match option {
        CommandDataOptionValue::Attachment(_) => "attachment",
        CommandDataOptionValue::Boolean(_) => "boolean",
        CommandDataOptionValue::Channel(_) => "channel",
        CommandDataOptionValue::Integer(_) => "integer",
        CommandDataOptionValue::Number(_) => "number",
        CommandDataOptionValue::Role(_) => "role",
        CommandDataOptionValue::String(_) => "string",
        CommandDataOptionValue::User(_) => "user",
        _ => "unknown",
    }
    .to_owned()
}
async fn dotheroar(ctx: &Context, interaction: &CommandInteraction) {
    match interaction.defer_ephemeral(&ctx.http).await {
        Ok(_) => {}
        Err(e) => {
            log::error!("Error deferring: {}", e);
        }
    }
    let options = interaction.data.options();
    let (option, media_type) = match options.iter().find_map(|o| match o.name {
        "audio_url" => Some((&o.value, MediaType::Audio)),
        "video_url" => Some((&o.value, MediaType::Video)),
        _ => None,
    }) {
        Some((ResolvedValue::String(s), kind)) => (s, kind),
        _ => {
            if let Err(e) = interaction
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new().add_embed(
                        builder::CreateEmbed::default()
                            .title("Error")
                            .description("This command requires a video url")
                            .color(Color::RED),
                    ),
                )
                .await
            {
                log::error!("Error editing original interaction response: {}", e);
            }
            return;
        }
    };
    let spoiler = match options.iter().find_map(|o| match o.name {
        "spoiler" => Some(&o.value),
        _ => None,
    }) {
        Some(ResolvedValue::Boolean(spoiler)) => *spoiler,
        _ => false,
    };
    let mut max_size = "8M";
    if let Some(guild_id) = interaction.guild_id {
        let guild = guild_id.to_guild_cached(&ctx.cache);
        if let Some(guild) = guild {
            match guild.premium_tier {
                PremiumTier::Tier3 => max_size = "100M",
                PremiumTier::Tier2 => max_size = "50M",
                _ => {}
            }
        } else {
            log::warn!("No guild in cache");
        }
    } else {
        log::trace!("No guild id in interaction");
    }
    match crate::video::Video::download_video(option, media_type, spoiler, max_size).await {
        Err(e) => match interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().add_embed(
                    builder::CreateEmbed::default()
                        .title("Error")
                        .description(format!("{}", e))
                        .color(Color::RED),
                ),
            )
            .await
        {
            Ok(_) => {}
            Err(e) => {
                log::error!("Fatal error creating followup message: {}", e)
            }
        },
        Ok(video) => {
            match video {
                VideoType::Disk(video) => {
                    let file = match CreateAttachment::path(&video.path()).await {
                        Ok(f) => f,
                        Err(e) => {
                            match interaction
                                .create_followup(
                                    &ctx.http,
                                    CreateInteractionResponseFollowup::new()
                                        .add_embed(
                                            builder::CreateEmbed::default()
                                                .title("Error")
                                                .description(format!("{}", e))
                                                .color(Color::RED),
                                        )
                                        .ephemeral(true),
                                )
                                .await
                            {
                                Ok(_) => {}
                                Err(e) => {
                                    log::error!("Fatal error creating followup message: {}", e)
                                }
                            }
                            return;
                        }
                    };
                    match interaction.delete_response(&ctx.http).await {
                        Ok(_) => {}
                        Err(e) => {
                            log::error!("Error deleting original interaction response: {}", e)
                        }
                    };
                    if let Err(e) = interaction
                        .create_followup(
                            &ctx.http,
                            CreateInteractionResponseFollowup::new().add_file(file),
                        )
                        .await
                    {
                        match interaction
                            .create_followup(
                                &ctx.http,
                                CreateInteractionResponseFollowup::new()
                                    .add_embed(
                                        builder::CreateEmbed::default()
                                            .title("Error")
                                            .description(format!("{}", e))
                                            .color(Color::RED),
                                    )
                                    .ephemeral(true),
                            )
                            .await
                        {
                            Ok(_) => {}
                            Err(e) => {
                                log::error!("Fatal error creating followup message: {}", e)
                            }
                        }
                    };
                }
                _ => unreachable!(),
            };
        }
    }
}
