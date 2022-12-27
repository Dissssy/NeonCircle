use std::io::{BufReader, BufWriter, Cursor, Write};

use anyhow::Error;
use image::ImageOutputFormat;
use serenity::builder::CreateApplicationCommand;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::prelude::command::CommandOptionType;

use image::{
    codecs::gif::{GifDecoder, GifEncoder, Repeat::Infinite},
    AnimationDecoder, DynamicImage, Frame, GenericImage, GenericImageView, Pixel, RgbaImage,
};

use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;
use serenity::prelude::Context;

use crate::CommandTrait;

use super::music::VideoType;

// command to download any video that ytdlp can download
#[derive(Debug, Clone)]
pub struct Video;

#[serenity::async_trait]
impl crate::CommandTrait for Video {
    async fn run(&self, ctx: &Context, interaction: Interaction) {
        let interaction = interaction.application_command().unwrap();
        interaction
            .create_interaction_response(&ctx.http, |response| response.kind(InteractionResponseType::DeferredChannelMessageWithSource))
            .await
            .unwrap();
        let options = interaction.data.options.clone();
        let url = options[0].value.as_ref().unwrap().as_str().unwrap();
        let video = crate::video::Video::get_video(url.to_owned(), false, false).await;
        if let Ok(video) = video {
            let video = video[0].clone();
            match video {
                VideoType::Disk(video) => {
                    let file = serenity::model::channel::AttachmentType::Path(&video.path);
                    let _ = interaction.delete_original_interaction_response(&ctx.http);
                    let _ = interaction
                        .create_followup_message(&ctx.http, |m| {
                            m.add_file(file);
                            m
                        })
                        .await;

                    video.delete().unwrap();
                }
                _ => unreachable!(),
            }
        } else {
            interaction
                .edit_original_interaction_response(&ctx.http, |response| response.content(format!("Error: {}", video.unwrap_err())))
                .await
                .unwrap();
        }
    }

    fn register(&self, command: &mut CreateApplicationCommand) {
        command
            .name(self.name())
            .description("Embed a video using ytdl")
            .create_option(|option| option.name("video_url").description("The url of the video to embed").kind(CommandOptionType::String).required(true));
    }

    fn name(&self) -> &str {
        "embed_video"
    }

    async fn autocomplete(&self, _ctx: &Context, _auto: &AutocompleteInteraction) -> Result<(), Error> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Audio;

#[serenity::async_trait]
impl crate::CommandTrait for Audio {
    async fn run(&self, ctx: &Context, interaction: Interaction) {
        let interaction = interaction.application_command().unwrap();
        interaction
            .create_interaction_response(&ctx.http, |response| response.kind(InteractionResponseType::DeferredChannelMessageWithSource))
            .await
            .unwrap();
        let options = interaction.data.options.clone();
        let url = options[0].value.as_ref().unwrap().as_str().unwrap();
        let video = crate::video::Video::get_video(url.to_owned(), true, false).await;
        if let Ok(video) = video {
            let video = video[0].clone();
            match video {
                VideoType::Disk(video) => {
                    let file = serenity::model::channel::AttachmentType::Path(&video.path);
                    let _ = interaction.delete_original_interaction_response(&ctx.http);
                    let _ = interaction
                        .create_followup_message(&ctx.http, |m| {
                            m.add_file(file);
                            m
                        })
                        .await;

                    video.delete().unwrap();
                }
                _ => unreachable!(),
            }
        } else {
            interaction
                .edit_original_interaction_response(&ctx.http, |response| response.content(format!("Error: {}", video.unwrap_err())))
                .await
                .unwrap();
        }
    }

    fn register(&self, command: &mut CreateApplicationCommand) {
        command
            .name(self.name())
            .description("Embed some audio using ytdl")
            .create_option(|option| option.name("audio_url").description("The url of the audio to embed").kind(CommandOptionType::String).required(true));
    }

    fn name(&self) -> &str {
        "embed_audio"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &AutocompleteInteraction) -> Result<(), Error> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct John;

#[serenity::async_trait]
impl CommandTrait for John {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command
            .name(self.name())
            .description("John")
            .create_option(|option| option.name("image").description("Image").kind(CommandOptionType::Attachment).required(true));
    }
    fn name(&self) -> &str {
        "john"
    }
    async fn run(&self, ctx: &Context, interaction: Interaction) {
        let interaction = interaction.application_command().unwrap();
        interaction
            .create_interaction_response(&ctx.http, |response| response.kind(InteractionResponseType::DeferredChannelMessageWithSource))
            .await
            .unwrap();
        let options = interaction.data.options.clone();
        // deserialize the attachment
        let attachment = match options[0].resolved.as_ref().unwrap() {
            serenity::model::prelude::interaction::application_command::CommandDataOptionValue::Attachment(a) => a,
            _ => unreachable!(),
        };
        let f = match attachment.download().await {
            Err(e) => {
                interaction
                    .edit_original_interaction_response(&ctx.http, |response| response.content(format!("Error: {}", e)))
                    .await
                    .unwrap();
                return;
            }
            Ok(f) => f,
        };
        let filename = &attachment.filename;
        let john = john(f, filename);

        match john {
            Ok(john) => {
                let file = serenity::model::channel::AttachmentType::Bytes {
                    data: john.into(),
                    filename: format!("john_{}", filename),
                };
                let _ = interaction.delete_original_interaction_response(&ctx.http);
                let _ = interaction
                    .create_followup_message(&ctx.http, |m| {
                        m.add_file(file);
                        m
                    })
                    .await;
            }
            Err(e) => {
                interaction
                    .edit_original_interaction_response(&ctx.http, |response| response.content(format!("Error: {}", e)))
                    .await
                    .unwrap();
            }
        }
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &AutocompleteInteraction) -> Result<(), Error> {
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
        let file_in = BufReader::new(image.as_slice());
        let decoder = GifDecoder::new(file_in)?;
        let frames = decoder.into_frames();
        let frames = frames.collect_frames()?;

        let mut frames_output = Vec::new();
        for frame in frames {
            let buffer = frame.buffer();
            let dynamic_image = DynamicImage::ImageRgba8(buffer.clone());

            let johned_image = john_the_image(dynamic_image)?;
            let johned_frame = Frame::from_parts(johned_image.to_rgba8(), frame.left(), frame.top(), frame.delay());

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
        no.write_to(&mut output, ImageOutputFormat::Png)?;
        Ok(output.into_inner())
    }
}
