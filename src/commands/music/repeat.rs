use serenity::futures::StreamExt;

use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;

use std::time::Duration;

use anyhow::Error;
use serenity::builder::CreateApplicationCommand;
use serenity::futures::channel::mpsc;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::prelude::command::CommandOptionType;

use serenity::prelude::Context;

use super::{get_mutual_voice_channel, AudioCommandHandler, AudioPromiseCommand};

#[derive(Debug, Clone)]
pub struct Repeat;

#[serenity::async_trait]
impl crate::CommandTrait for Repeat {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command
            .name(self.name())
            .description("Repeat the current song")
            .create_option(|option| {
                option
                    .name("repeat")
                    .description("Repeat the current song")
                    .kind(CommandOptionType::Boolean)
                    .required(true)
            });
    }
    async fn run(&self, ctx: &Context, interaction: Interaction) {
        let interaction = interaction.application_command().unwrap();
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response
                    .interaction_response_data(|f| f.ephemeral(true))
                    .kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await
            .unwrap();
        let guild_id = interaction.guild_id.unwrap();

        let mutual = get_mutual_voice_channel(ctx, &interaction).await;
        if let Some((join, _channel_id)) = mutual {
            if !join {
                let data_read = ctx.data.read().await;
                let audio_command_handler = data_read
                    .get::<AudioCommandHandler>()
                    .expect("Expected AudioCommandHandler in TypeMap")
                    .clone();
                let mut audio_command_handler = audio_command_handler.lock().await;
                let tx = audio_command_handler
                    .get_mut(&guild_id.to_string())
                    .unwrap();
                let (rtx, mut rrx) = mpsc::unbounded::<String>();
                tx.unbounded_send((
                    rtx,
                    AudioPromiseCommand::Repeat(
                        interaction.data.options[0]
                            .value
                            .as_ref()
                            .unwrap()
                            .as_bool()
                            .unwrap(),
                    ),
                ))
                .unwrap();

                let timeout = tokio::time::timeout(Duration::from_secs(10), rrx.next()).await;
                if let Ok(Some(msg)) = timeout {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.content(msg)
                        })
                        .await
                        .unwrap();
                } else {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.content("Timed out waiting for song to repeat")
                        })
                        .await
                        .unwrap();
                }
            } else {
                interaction
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content("I'm not in a voice channel you dingus")
                    })
                    .await
                    .unwrap();
            }
        }
    }
    fn name(&self) -> &str {
        "repeat"
    }
    async fn autocomplete(
        &self,
        _ctx: &Context,
        _auto: &AutocompleteInteraction,
    ) -> Result<(), Error> {
        Ok(())
    }
}
