use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;

use std::time::Duration;

use anyhow::Error;
use serenity::builder::CreateApplicationCommand;

use serenity::model::application::interaction::InteractionResponseType;
use serenity::model::prelude::command::CommandOptionType;

use serenity::prelude::Context;

use super::{AudioCommandHandler, AudioPromiseCommand};

#[derive(Debug, Clone)]
pub struct SetBitrate;

#[serenity::async_trait]
impl crate::CommandTrait for SetBitrate {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command
            .name(self.name())
            .description("Set the bot's bitrate")
            .create_option(|option| {
                option
                    .name("bitrate")
                    .description("the bitrate to set the bot to")
                    .min_int_value(512)
                    .max_int_value(512_000)
                    .kind(CommandOptionType::Integer)
                    .required(true)
            });
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

        let option = match interaction
            .data
            .options
            .iter()
            .find(|o| o.name == "bitrate")
        {
            Some(o) => match o.value.as_ref() {
                Some(v) => {
                    if let Some(v) = v.as_i64() {
                        v
                    } else {
                        interaction
                            .edit_original_interaction_response(&ctx.http, |response| {
                                response.content("This command requires an option")
                            })
                            .await
                            .unwrap();
                        return;
                    }
                }
                None => {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.content("This command requires an option")
                        })
                        .await
                        .unwrap();
                    return;
                }
            },
            None => {
                interaction
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content("This command requires an option")
                    })
                    .await
                    .unwrap();
                return;
            }
        };

        let ungus = {
            let bingus = ctx.data.read().await;
            let bungly = bingus.get::<super::VoiceData>();

            bungly.cloned()
        };

        if let (Some(v), Some(member)) = (ungus, interaction.member.as_ref()) {
            let next_step = {
                let mut v = v.lock().await;
                v.mutual_channel(ctx, &guild_id, &member.user.id)
            };

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
                        let (rtx, rrx) = serenity::futures::channel::oneshot::channel::<String>();
                        tx.unbounded_send((
                            rtx,
                            AudioPromiseCommand::SetBitrate(super::OrAuto::Specific(option)),
                        ))
                        .unwrap();

                        let timeout = tokio::time::timeout(Duration::from_secs(10), rrx).await;
                        if let Ok(Ok(msg)) = timeout {
                            interaction
                                .edit_original_interaction_response(&ctx.http, |response| {
                                    response.content(msg)
                                })
                                .await
                                .unwrap();
                        } else {
                            interaction
                                .edit_original_interaction_response(&ctx.http, |response| {
                                    response.content("Timed out waiting for queue to shuffle")
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
        // let interaction = interaction.application_command().unwrap();
        // interaction
        //     .create_interaction_response(&ctx.http, |response| {
        //         response
        //             .interaction_response_data(|f| f.ephemeral(true))
        //             .kind(InteractionResponseType::DeferredChannelMessageWithSource)
        //     })
        //     .await
        //     .unwrap();
        // let guild_id = match interaction.guild_id {
        //     Some(id) => id,
        //     None => {
        //         interaction
        //             .edit_original_interaction_response(&ctx.http, |response| {
        //                 response.content("This command can only be used in a server")
        //             })
        //             .await
        //             .unwrap();
        //         return;
        //     }
        // };

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
        //         let (rtx, mut rrx) = serenity::futures::channel::oneshot::channel::<String>();

        //         let bitrate = interaction.data.options[0]
        //             .value
        //             .as_ref()
        //             .unwrap()
        //             .as_i64()
        //             .unwrap();

        //         if !(1200..=384000).contains(&bitrate) {
        //             interaction
        //                 .edit_original_interaction_response(&ctx.http, |response| {
        //                     response.content("Bitrate must be between 1200 and 384000")
        //                 })
        //                 .await
        //                 .unwrap();
        //             return;
        //         }

        //         tx.unbounded_send((rtx, AudioPromiseCommand::SetBitrate(bitrate)))
        //             .unwrap();

        //         let timeout = tokio::time::timeout(Duration::from_secs(10), rrx).await;
        //         if let Ok(Ok(msg)) = timeout {
        //             interaction
        //                 .edit_original_interaction_response(&ctx.http, |response| {
        //                     response.content(msg)
        //                 })
        //                 .await
        //                 .unwrap();
        //         } else {
        //             interaction
        //                 .edit_original_interaction_response(&ctx.http, |response| {
        //                     response.content("Timed out waiting for bitrate to set")
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
        "set_bitrate"
    }
    async fn autocomplete(
        &self,
        _ctx: &Context,
        _auto: &AutocompleteInteraction,
    ) -> Result<(), Error> {
        Ok(())
    }
}
