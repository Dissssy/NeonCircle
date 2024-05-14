use super::music::VideoType;
use crate::{video::MediaType, CommandTrait};
use anyhow::Error;
use image::{
    codecs::gif::{GifDecoder, GifEncoder, Repeat::Infinite},
    AnimationDecoder, DynamicImage, Frame, GenericImage, GenericImageView, ImageFormat, Pixel,
};
use serenity::all::*;
use std::io::{BufWriter, Cursor};
#[derive(Debug, Clone)]
pub struct Video;
#[async_trait]
impl crate::CommandTrait for Video {
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) {
        dotheroar(ctx, interaction).await;
    }
    fn register(&self) -> CreateCommand {
        CreateCommand::new(self.name())
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
            ])
    }
    fn name(&self) -> &str {
        "embed_video"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &CommandInteraction) -> Result<(), Error> {
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct Audio;
#[async_trait]
impl crate::CommandTrait for Audio {
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) {
        dotheroar(ctx, interaction).await;
    }
    fn register(&self) -> CreateCommand {
        CreateCommand::new(self.name())
            .description("Embed some audio using ytdl")
            .set_options(vec![CreateCommandOption::new(
                CommandOptionType::String,
                "audio_url",
                "The url of the audio to embed",
            )
            .required(true)])
    }
    fn name(&self) -> &str {
        "embed_audio"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &CommandInteraction) -> Result<(), Error> {
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct John;
#[async_trait]
impl CommandTrait for John {
    fn register(&self) -> CreateCommand {
        CreateCommand::new(self.name())
            .description("John")
            .set_options(vec![CreateCommandOption::new(
                CommandOptionType::Attachment,
                "image",
                "Image",
            )
            .required(true)])
    }
    fn name(&self) -> &str {
        "john"
    }
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) {
        if let Err(e) = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
            )
            .await
        {
            println!("Error deferring: {}", e);
        }
        let options = interaction.data.options();
        let attachment = match options.iter().find(|o| o.name == "image").map(|c| &c.value) {
            Some(ResolvedValue::Attachment(a)) => a,
            _ => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new()
                            .content("this command requires an attachment"),
                    )
                    .await
                {
                    println!("Error editing original interaction response: {}", e);
                }
                return;
            }
        };
        let f = match attachment.download().await {
            Err(e) => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content(format!("Error: {}", e)),
                    )
                    .await
                {
                    println!("Error editing original interaction response: {}", e);
                }
                return;
            }
            Ok(f) => f,
        };
        let filename = &attachment.filename;
        let john = john(f, filename);
        match john {
            Ok(john) => {
                let file = CreateAttachment::bytes(john, format!("john_{}", filename));
                let _ = interaction.delete_response(&ctx.http).await;
                let _ = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new().add_file(file),
                    )
                    .await;
            }
            Err(e) => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content(format!("Error: {}", e)),
                    )
                    .await
                {
                    println!("Error editing original interaction response: {}", e);
                }
            }
        }
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &CommandInteraction) -> Result<(), Error> {
        Ok(())
    }
}
fn get_green_channel(src: &DynamicImage) -> anyhow::Result<DynamicImage> {
    let mut dst = DynamicImage::new_rgba8(src.width(), src.height());
    for (x, y, pixel) in src.pixels() {
        let channels = pixel.channels();
        dst.put_pixel(x, y, image::Rgba([0, channels[1], 0, channels[3]]));
    }
    Ok(dst)
}
fn john_the_image(image: DynamicImage) -> anyhow::Result<DynamicImage> {
    let green = get_green_channel(&image)?;
    //green.save("john_2_green.png")?;
    let green_hue = green.huerotate(-75);
    //green_hue.save("john_3_green_hue.png")?;
    let mut output = DynamicImage::new_rgba8(image.width(), image.height());
    for (x, y, orig_pixel) in image.pixels() {
        let green_pixel = green_hue.get_pixel(x, y);
        let orig_channels = orig_pixel.channels();
        let green_channels = green_pixel.channels();
        let r = orig_channels[0].saturating_add(green_channels[0]);
        let g = orig_channels[1].saturating_add(green_channels[1]);
        let b = orig_channels[2].saturating_add(green_channels[2]);
        output.put_pixel(x, y, image::Rgba([r, g, b, orig_channels[3]]));
    }
    Ok(output)
}
fn john(image: Vec<u8>, filename: &str) -> Result<Vec<u8>, Error> {
    if filename.ends_with("gif") {
        let file_in = Cursor::new(image.as_slice());
        let decoder = GifDecoder::new(file_in)?;
        let frames = decoder.into_frames();
        let frames = frames.collect_frames()?;
        let mut frames_output = Vec::new();
        for frame in frames {
            let buffer = frame.buffer();
            let dynamic_image = DynamicImage::ImageRgba8(buffer.clone());
            let johned_image = john_the_image(dynamic_image)?;
            let johned_frame = Frame::from_parts(
                johned_image.to_rgba8(),
                frame.left(),
                frame.top(),
                frame.delay(),
            );
            frames_output.push(johned_frame);
        }
        let mut output = BufWriter::new(Vec::new());
        {
            let mut encoder = GifEncoder::new(&mut output);
            encoder.set_repeat(Infinite)?;
            encoder.encode_frames(frames_output)?;
        }
        output.into_inner().map_err(|e| e.into())
    } else {
        let image = image::load_from_memory(&image)?;
        let no = john_the_image(image)?;
        let mut output = Cursor::new(Vec::new());
        no.write_to(&mut output, ImageFormat::Png)?;
        Ok(output.into_inner())
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
            println!("Error deferring: {}", e);
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
                            .color(Color::RED)
                            .to_owned(),
                    ),
                )
                .await
            {
                println!("Error editing original interaction response: {}", e);
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
            println!("No guild in cache");
        }
    } else {
        println!("No guild id");
    }
    match crate::video::Video::download_video(option, media_type, spoiler, max_size).await {
        Err(e) => match interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().add_embed(
                    builder::CreateEmbed::default()
                        .title("Error")
                        .description(format!("{}", e))
                        .color(Color::RED)
                        .to_owned(),
                ),
            )
            .await
        {
            Ok(_) => {}
            Err(e) => {
                println!("Fatal error creating followup message: {}", e)
            }
        },
        Ok(video) => {
            match video {
                VideoType::Disk(video) => {
                    let file = match CreateAttachment::path(&video.path).await {
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
                                                .color(Color::RED)
                                                .to_owned(),
                                        )
                                        .ephemeral(true),
                                )
                                .await
                            {
                                Ok(_) => {}
                                Err(e) => {
                                    println!("Fatal error creating followup message: {}", e)
                                }
                            }
                            return;
                        }
                    };
                    match interaction.delete_response(&ctx.http).await {
                        Ok(_) => {}
                        Err(e) => {
                            println!("Error deleting original interaction response: {}", e)
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
                                            .color(Color::RED)
                                            .to_owned(),
                                    )
                                    .ephemeral(true),
                            )
                            .await
                        {
                            Ok(_) => {}
                            Err(e) => {
                                println!("Fatal error creating followup message: {}", e)
                            }
                        }
                    };
                    match video.delete() {
                        Ok(_) => {}
                        Err(e) => {
                            println!("Error deleting video: {}", e)
                        }
                    };
                }
                _ => unreachable!(),
            };
        }
    }
}
