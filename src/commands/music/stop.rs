use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;

use std::time::Duration;

use anyhow::Error;
use serenity::builder::CreateApplicationCommand;

use serenity::model::application::interaction::InteractionResponseType;

use serenity::prelude::Context;

use super::{AudioCommandHandler, AudioPromiseCommand};

#[derive(Debug, Clone)]
pub struct Stop;

#[serenity::async_trait]
impl crate::CommandTrait for Stop {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command.name(self.name()).description("Stop all playback");
    }
    async fn run(&self, ctx: &Context, interaction: &serenity::model::prelude::application_command::ApplicationCommandInteraction) {
        if let Err(e) = interaction.create_interaction_response(&ctx.http, |response| response.interaction_response_data(|f| f.ephemeral(true)).kind(InteractionResponseType::DeferredChannelMessageWithSource)).await {
            eprintln!("Failed to create interaction response: {:?}", e);
        };
        let guild_id = match interaction.guild_id {
            Some(id) => id,
            None => {
                if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command can only be used in a server")).await {
                    eprintln!("Failed to edit original interaction response: {:?}", e);
                }
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
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("You're not in a voice channel")).await {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::InDifferent(_channel) => {
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("I'm in a different voice channel")).await {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::Join(_channel) => {
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("I'm not in a channel, if you want me to join use /join or /play")).await {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::InSame(_channel) => {
                    let audio_command_handler = ctx.data.read().await.get::<AudioCommandHandler>().expect("Expected AudioCommandHandler in TypeMap").clone();

                    let mut audio_command_handler = audio_command_handler.lock().await;

                    if let Some(tx) = audio_command_handler.get_mut(&guild_id.to_string()) {
                        let (rtx, rrx) = serenity::futures::channel::oneshot::channel::<String>();
                        if tx.unbounded_send((rtx, AudioPromiseCommand::Stop)).is_err() {
                            if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("Failed to send stop command")).await {
                                eprintln!("Failed to edit original interaction response: {:?}", e);
                            }
                            return;
                        }

                        let timeout = tokio::time::timeout(Duration::from_secs(10), rrx).await;
                        if let Ok(Ok(msg)) = timeout {
                            if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content(msg)).await {
                                eprintln!("Failed to edit original interaction response: {:?}", e);
                            }
                        } else if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("Timed out waiting for music to stop")).await {
                            eprintln!("Failed to edit original interaction response: {:?}", e);
                        }
                        while !tx.is_closed() {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }

                        audio_command_handler.remove(&guild_id.to_string());
                    } else if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("Couldnt find the channel handler :( im broken.")).await {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                }
            }
        } else if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("TELL ETHAN THIS SHOULD NEVER HAPPEN :(")).await {
            eprintln!("Failed to edit original interaction response: {:?}", e);
        }
    }
    fn name(&self) -> &str {
        "stop"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &AutocompleteInteraction) -> Result<(), Error> {
        Ok(())
    }
}
