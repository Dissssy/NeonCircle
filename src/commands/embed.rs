use std::io::{BufWriter, Cursor};

use anyhow::Error;
use serenity::builder::CreateApplicationCommand;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::model::prelude::command::CommandOptionType;
use serenity::model::prelude::PremiumTier;

use image::{
    codecs::gif::{GifDecoder, GifEncoder, Repeat::Infinite},
    AnimationDecoder, DynamicImage, Frame, GenericImage, GenericImageView, ImageFormat, Pixel,
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
    async fn run(&self, ctx: &Context, interaction: &serenity::model::prelude::application_command::ApplicationCommandInteraction) {
        dotheroar(ctx, interaction, false).await;
    }

    fn register(&self, command: &mut CreateApplicationCommand) {
        command.name(self.name()).description("Embed a video using ytdl").create_option(|option| option.name("video_url").description("The url of the video to embed").kind(CommandOptionType::String).required(true)).create_option(|option| option.name("spoiler").description("Whether to spoiler the video").kind(CommandOptionType::Boolean).required(false));
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
    async fn run(&self, ctx: &Context, interaction: &serenity::model::prelude::application_command::ApplicationCommandInteraction) {
        dotheroar(ctx, interaction, true).await;
    }

    fn register(&self, command: &mut CreateApplicationCommand) {
        command.name(self.name()).description("Embed some audio using ytdl").create_option(|option| option.name("audio_url").description("The url of the audio to embed").kind(CommandOptionType::String).required(true));
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
        command.name(self.name()).description("John").create_option(|option| option.name("image").description("Image").kind(CommandOptionType::Attachment).required(true));
    }
    fn name(&self) -> &str {
        "john"
    }
    async fn run(&self, ctx: &Context, interaction: &serenity::model::prelude::application_command::ApplicationCommandInteraction) {
        // let interaction = interaction.application_command().unwrap();
        if let Err(e) = interaction.create_interaction_response(&ctx.http, |response| response.kind(InteractionResponseType::DeferredChannelMessageWithSource)).await {
            println!("Error deferring: {}", e);
        }
        let options = interaction.data.options.clone();
        // deserialize the attachment
        let attachment = match options[0].resolved.as_ref() {
            Some(serenity::model::prelude::interaction::application_command::CommandDataOptionValue::Attachment(a)) => a,
            _ => unreachable!("Attachment not found"),
        };
        let f = match attachment.download().await {
            Err(e) => {
                if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content(format!("Error: {}", e))).await {
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
                let file = serenity::model::channel::AttachmentType::Bytes { data: john.into(), filename: format!("john_{}", filename) };
                let _ = interaction.delete_original_interaction_response(&ctx.http).await;
                let _ = interaction
                    .create_followup_message(&ctx.http, |m| {
                        m.add_file(file);
                        m
                    })
                    .await;
            }
            Err(e) => {
                if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content(format!("Error: {}", e))).await {
                    println!("Error editing original interaction response: {}", e);
                }
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
        let file_in = Cursor::new(image.as_slice());
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
        no.write_to(&mut output, ImageFormat::Png)?;
        Ok(output.into_inner())
    }
}

#[allow(dead_code)]
fn get_command_data_option_name(option: &serenity::model::application::interaction::application_command::CommandDataOptionValue) -> String {
    match option {
        serenity::model::application::interaction::application_command::CommandDataOptionValue::Attachment(_) => "attachment",
        serenity::model::application::interaction::application_command::CommandDataOptionValue::Boolean(_) => "boolean",
        serenity::model::application::interaction::application_command::CommandDataOptionValue::Channel(_) => "channel",
        serenity::model::application::interaction::application_command::CommandDataOptionValue::Integer(_) => "integer",
        serenity::model::application::interaction::application_command::CommandDataOptionValue::Number(_) => "number",
        serenity::model::application::interaction::application_command::CommandDataOptionValue::Role(_) => "role",
        serenity::model::application::interaction::application_command::CommandDataOptionValue::String(_) => "string",
        serenity::model::application::interaction::application_command::CommandDataOptionValue::User(..) => "user",
        _ => "unknown",
    }
    .to_owned()
}

async fn dotheroar(ctx: &Context, interaction: &serenity::model::prelude::application_command::ApplicationCommandInteraction, audio_only: bool) {
    // let interaction = interaction.application_command().expect("Not a command");
    match interaction.defer_ephemeral(&ctx.http).await {
        Ok(_) => {}
        Err(e) => {
            println!("Error deferring: {}", e);
        }
    }
    // .create_interaction_response(&ctx.http, |response| {
    //     response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
    // })
    // .await
    // .unwrap();

    // let mut spoiler = false;
    // let mut v = Err(anyhow::anyhow!("No url provided"));

    // for option in interaction.data.options.clone() {
    //     // println!("{}: {:?}", option.name, option.resolved);
    //     match option.name.as_str() {
    //         "spoiler" => {
    //             spoiler = match option.resolved {
    //                 Some(serenity::model::application::interaction::application_command::CommandDataOptionValue::Boolean(b)) => b,
    //                 _ => false,
    //             }
    //         }
    //         "video_url" | "audio_url" => {
    //             v = match option.resolved {
    //                 Some(serenity::model::application::interaction::application_command::CommandDataOptionValue::String(s)) => {
    //                     Ok(s)
    //                 }
    //                 _ => Err(anyhow::anyhow!("No value provided")),
    //             }
    //         }
    //         s => {
    //             println!("Unknown option: {}", s);
    //         }
    //     }
    // }

    let option = match interaction.data.options.iter().find(|o| o.name == "video_url" || o.name == "audio_url") {
        Some(o) => match o.value.as_ref() {
            Some(v) => {
                if let Some(v) = v.as_str() {
                    v
                } else {
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command requires an option")).await {
                        println!("Error editing original interaction response: {}", e);
                    }
                    return;
                }
            }
            None => {
                if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command requires an option")).await {
                    println!("Error editing original interaction response: {}", e);
                }
                return;
            }
        },
        None => {
            if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command requires an option")).await {
                println!("Error editing original interaction response: {}", e);
            }
            return;
        }
    };

    let spoiler = match interaction.data.options.iter().find(|o| o.name == "spoiler") {
        Some(o) => match o.value.as_ref() {
            Some(v) => v.as_bool().unwrap_or_default(),
            None => false,
        },
        None => false,
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

    // .and_then(|f| {
    //     f.to_guild_cached(ctx.cache)
    //         .and_then(|f| match f.premium_tier {
    //             PremiumTier::Tier3 => "100MB",
    //             PremiumTier::Tier2 => "50MB",
    //             _ => "8MB",
    //         })
    // });

    // let spoiler = match options.get(1).and_then(|m| m.resolved) {
    //         Some(ref value) => match value {
    //             serenity::model::application::interaction::application_command::CommandDataOptionValue::Boolean(ref bool) => *bool,
    //             _ => false,
    //         },
    //         None => false,
    //     };

    // let v = match options.get(0) {
    //         Some(option) => match option.resolved {
    //             Some(ref value) => match value {
    //                 serenity::model::application::interaction::application_command::CommandDataOptionValue::String(ref string) => crate::video::Video::download_video(string.to_owned(), audio_only, spoiler).await,
    //                 v => Err(anyhow::anyhow!("How dawg, how did you put a {} in a string option", get_command_data_option_name(v))),
    //             },
    //             None => Err(anyhow::anyhow!("No value provided")),
    //         },
    //         None => Err(anyhow::anyhow!("No url provided")),
    //     };

    // let fuckyouclosures = match v {
    //     Ok(v) => crate::video::Video::download_video(v, audio_only, spoiler, max_size).await,
    //     Err(e) => Err(e),
    // };

    match crate::video::Video::download_video(option, audio_only, spoiler, max_size).await {
        Err(e) => match interaction.edit_original_interaction_response(&ctx.http, |response| response.add_embed(serenity::builder::CreateEmbed::default().title("Error").description(format!("{}", e)).color(serenity::utils::Colour::RED).to_owned())).await {
            Ok(_) => {}
            Err(e) => {
                println!("Fatal error creating followup message: {}", e)
            }
        },
        Ok(video) => {
            // match video {
            // Some(video) =>
            match video {
                VideoType::Disk(video) => {
                    let file = serenity::model::channel::AttachmentType::Path(&video.path);
                    match interaction.delete_original_interaction_response(&ctx.http).await {
                        Ok(_) => {}
                        Err(e) => {
                            println!("Error deleting original interaction response: {}", e)
                        }
                    };
                    if let Err(e) = interaction
                        .create_followup_message(&ctx.http, |m| {
                            m.add_file(file);
                            m.flags(serenity::model::prelude::InteractionApplicationCommandCallbackDataFlags::empty());
                            m
                        })
                        .await
                    {
                        match interaction.create_followup_message(&ctx.http, |m| m.add_embed(serenity::builder::CreateEmbed::default().title("Error").description(format!("{}", e)).color(serenity::utils::Colour::RED).to_owned()).flags(serenity::model::prelude::InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)).await {
                            Ok(_) => {}
                            Err(e) => {
                                println!("Fatal error creating followup message: {}", e)
                            }
                        }
                    };
                    // println!(
                    //     "video size was {}",
                    //     std::fs::metadata(&video.path).unwrap().len()
                    // );
                    // println!("Deleting video {}", video.path.display());
                    match video.delete() {
                        Ok(_) => {}
                        Err(e) => {
                            println!("Error deleting video: {}", e)
                        }
                    };
                }
                _ => unreachable!(),
            };
            //     None => {
            //         interaction
            //             .edit_original_interaction_response(&ctx.http, |response| {
            //                 response.content("No videos found")
            //             })
            //             .await
            //             .unwrap();
            //         return;
            //     }
            // };
        }
    }
}
