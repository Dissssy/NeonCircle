use anyhow::Error;
use serenity::builder::CreateApplicationCommand;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::prelude::command::CommandOptionType;

use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;
use serenity::prelude::Context;

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
            let file = serenity::model::channel::AttachmentType::Path(&video.path);
            let _ = interaction.delete_original_interaction_response(&ctx.http);
            let _ = interaction
                .create_followup_message(&ctx.http, |m| {
                    m.add_file(file); //.content(format!("{} - {}", video.title, video.duration));
                    m
                })
                .await;
            // delete the file
            video.delete().unwrap();
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
            .create_option(|option| option.name("url").description("The url of the video to embed").kind(CommandOptionType::String).required(true));
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
            let file = serenity::model::channel::AttachmentType::Path(&video.path);
            let _ = interaction.delete_original_interaction_response(&ctx.http);
            let _ = interaction
                .create_followup_message(&ctx.http, |m| {
                    m.add_file(file); //.content(format!("{} - {}", video.title, video.duration));
                    m
                })
                .await;
            // delete the file
            video.delete().unwrap();
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
            .create_option(|option| option.name("url").description("The url of the audio to embed").kind(CommandOptionType::String).required(true));
    }

    fn name(&self) -> &str {
        "embed_audio"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &AutocompleteInteraction) -> Result<(), Error> {
        Ok(())
    }
}
