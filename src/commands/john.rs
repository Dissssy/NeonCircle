use crate::traits::CommandTrait;
use anyhow::Result;
use image::{
    codecs::gif::{GifDecoder, GifEncoder, Repeat::Infinite},
    AnimationDecoder, DynamicImage, Frame, GenericImage, GenericImageView, ImageFormat, Pixel,
};
use serenity::all::*;
use std::io::{BufWriter, Cursor};
#[derive(Debug, Clone)]
pub struct Command;
#[async_trait]
impl CommandTrait for Command {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .description("John")
                .set_options(vec![CreateCommandOption::new(
                    CommandOptionType::Attachment,
                    "image",
                    "Image",
                )
                .required(true)]),
        )
    }
    fn command_name(&self) -> &str {
        "john"
    }
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        if let Err(e) = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()),
            )
            .await
        {
            log::error!("Error deferring: {}", e);
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
                    log::error!("Error editing original interaction response: {}", e);
                }
                return Ok(());
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
                    log::error!("Error editing original interaction response: {}", e);
                }
                return Ok(());
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
                    log::error!("Error editing original interaction response: {}", e);
                }
            }
        }
        Ok(())
    }
}
fn get_green_channel(src: &DynamicImage) -> anyhow::Result<DynamicImage> {
    let mut dst = DynamicImage::new_rgba8(src.width(), src.height());
    for (x, y, pixel) in src.pixels() {
        let channels = pixel.channels();
        dst.put_pixel(
            x,
            y,
            image::Rgba([
                0,
                channels.get(1).copied().unwrap_or_default(),
                0,
                channels.get(3).copied().unwrap_or_default(),
            ]),
        );
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
        // let r = orig_channels[0].saturating_add(green_channels[0]);
        let r = orig_channels
            .first()
            .and_then(|r| green_channels.first().map(|r2| r.saturating_add(*r2)))
            .unwrap_or_default();
        // let g = orig_channels[1].saturating_add(green_channels[1]);
        let g = orig_channels
            .get(1)
            .and_then(|g| green_channels.get(1).map(|g2| g.saturating_add(*g2)))
            .unwrap_or_default();
        // let b = orig_channels[2].saturating_add(green_channels[2]);
        let b = orig_channels
            .get(2)
            .and_then(|b| green_channels.get(2).map(|b2| b.saturating_add(*b2)))
            .unwrap_or_default();
        output.put_pixel(
            x,
            y,
            image::Rgba([r, g, b, orig_channels.get(3).copied().unwrap_or_default()]),
        );
    }
    Ok(output)
}
fn john(image: Vec<u8>, filename: &str) -> Result<Vec<u8>> {
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
