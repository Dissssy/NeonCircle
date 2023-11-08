use serenity::futures::StreamExt;

use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;

use std::time::Duration;

use anyhow::Error;
use serenity::builder::CreateApplicationCommand;
use serenity::futures::channel::mpsc;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};

use serenity::prelude::Context;

use super::{AudioCommandHandler, AudioPromiseCommand};

#[derive(Debug, Clone)]
pub struct Skip;

#[serenity::async_trait]
impl crate::CommandTrait for Skip {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command
            .name(self.name())
            .description("Skip the current song");
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &serenity::model::prelude::application_command::ApplicationCommandInteraction,
    ) {
        // let interaction = interaction.application_command().unwrap();
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response
                    .interaction_response_data(|f| f.ephemeral(true))
                    .kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await
            .unwrap();
        let guild_id = match interaction.guild_id {
            Some(id) => id,
            None => {
                interaction
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content("This command can only be used in a server")
                    })
                    .await
                    .unwrap();
                return;
            }
        };

        if let (Some(v), Some(member)) = (
            ctx.data.read().await.get::<super::VoiceData>().cloned(),
            interaction.member.as_ref(),
        ) {
            let mut v = v.lock().await;
            let next_step = v.mutual_channel(ctx, &guild_id, &member.user.id);

            match next_step {
                super::VoiceAction::UserNotConnected => {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.content("You're not in a voice channel")
                        })
                        .await
                        .unwrap();
                    return;
                }
                super::VoiceAction::InDifferent(_channel) => {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.content("I'm in a different voice channel")
                        })
                        .await
                        .unwrap();
                    return;
                }
                super::VoiceAction::Join(_channel) => {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.content(
                                "I'm not in a channel, if you want me to join use /join or /play",
                            )
                        })
                        .await
                        .unwrap();
                    return;
                }
                super::VoiceAction::InSame(_channel) => {
                    let audio_command_handler = ctx
                        .data
                        .read()
                        .await
                        .get::<AudioCommandHandler>()
                        .expect("Expected AudioCommandHandler in TypeMap")
                        .clone();

                    let mut audio_command_handler = audio_command_handler.lock().await;

                    if let Some(tx) = audio_command_handler.get_mut(&guild_id.to_string()) {
                        let (rtx, mut rrx) = mpsc::unbounded::<String>();
                        tx.unbounded_send((rtx, AudioPromiseCommand::Skip)).unwrap();

                        let timeout =
                            tokio::time::timeout(Duration::from_secs(10), rrx.next()).await;
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
                                    response.content("Timed out waiting for song to skip")
                                })
                                .await
                                .unwrap();
                        }
                    } else {
                        interaction
                            .edit_original_interaction_response(&ctx.http, |response| {
                                response.content("Couldnt find the channel handler :( im broken.")
                            })
                            .await
                            .unwrap();
                    }
                }
            }
        } else {
            interaction
                .edit_original_interaction_response(&ctx.http, |response| {
                    response.content("TELL ETHAN THIS SHOULD NEVER HAPPEN :(")
                })
                .await
                .unwrap();
        }

        // let mutual = get_mutual_voice_channel(ctx, &interaction).await;
        // if let Some((join, _channel_id)) = mutual {
        //     if !join {
        //         let data_read = ctx.data.read().await;
        //         let audio_command_handler = data_read
        //             .get::<AudioCommandHandler>()
        //             .expect("Expected AudioCommandHandler in TypeMap")
        //             .clone();
        //         let mut audio_command_handler = audio_command_handler.lock().await;
        //         let tx = audio_command_handler
        //             .get_mut(&guild_id.to_string())
        //             .unwrap();
        //         let (rtx, mut rrx) = mpsc::unbounded::<String>();
        //         tx.unbounded_send((rtx, AudioPromiseCommand::Skip)).unwrap();

        //         let timeout = tokio::time::timeout(Duration::from_secs(10), rrx.next()).await;
        //         if let Ok(Some(msg)) = timeout {
        //             interaction
        //                 .edit_original_interaction_response(&ctx.http, |response| {
        //                     response.content(msg)
        //                 })
        //                 .await
        //                 .unwrap();
        //         } else {
        //             interaction
        //                 .edit_original_interaction_response(&ctx.http, |response| {
        //                     response.content("Timed out waiting for song to skip")
        //                 })
        //                 .await
        //                 .unwrap();
        //         }
        //     } else {
        //         interaction
        //             .edit_original_interaction_response(&ctx.http, |response| {
        //                 response.content("I'm not in a voice channel you dingus")
        //             })
        //             .await
        //             .unwrap();
        //     }
        // }
    }
    fn name(&self) -> &str {
        "skip"
    }
    async fn autocomplete(
        &self,
        _ctx: &Context,
        _auto: &AutocompleteInteraction,
    ) -> Result<(), Error> {
        Ok(())
    }
}
