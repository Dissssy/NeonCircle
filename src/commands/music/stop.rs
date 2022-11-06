use serenity::futures::StreamExt;

use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;

use std::time::Duration;

use anyhow::Error;
use serenity::builder::CreateApplicationCommand;
use serenity::futures::channel::mpsc;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};

use serenity::prelude::Context;

use super::{get_mutual_voice_channel, AudioCommandHandler, AudioPromiseCommand};

#[derive(Debug, Clone)]
pub struct Stop;

#[serenity::async_trait]
impl crate::CommandTrait for Stop {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command.name(self.name()).description("Stop all playback");
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
                // interaction
                //     .edit_original_interaction_response(&ctx.http, |response| response.content("Stopping playback"))
                //     .await
                //     .unwrap();

                let data_read = ctx.data.read().await;
                let audio_command_handler = data_read.get::<AudioCommandHandler>().expect("Expected AudioCommandHandler in TypeMap").clone();
                let mut audio_command_handler = audio_command_handler.lock().await;
                let tx = audio_command_handler.get_mut(&guild_id.to_string()).unwrap();
                let (rtx, mut rrx) = mpsc::unbounded::<String>();
                tx.unbounded_send((rtx, AudioPromiseCommand::Stop)).unwrap();
                // wait for up to 10 seconds for the rrx to receive a message
                let timeout = tokio::time::timeout(Duration::from_secs(10), rrx.next()).await;
                if let Ok(Some(msg)) = timeout {
                    interaction.edit_original_interaction_response(&ctx.http, |response| response.content(msg)).await.unwrap();
                } else {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |response| response.content("Timed out waiting for song to stop playing"))
                        .await
                        .unwrap();
                }
                // wait until tx is closed
                while !tx.is_closed() {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                // remove the audio command handler
                audio_command_handler.remove(&guild_id.to_string());

                // // get the voice connection manager
                // let manager = songbird::get(ctx).await.unwrap().clone();
                // // get the voice connection for the guild
                // if let Some(call) = manager.get(guild_id) {
                //     // disconnect from the voice channel
                //     call.lock().await.leave().await.unwrap();
                // }
            } else {
                interaction
                    .edit_original_interaction_response(&ctx.http, |response| response.content("I'm not in a voice channel you dingus"))
                    .await
                    .unwrap();
            }
        }

        // {
        //     let data_read = ctx.data.read().await;
        //     let guild_id = interaction.guild_id.unwrap();
        //     let audio_command_handler = data_read.get::<AudioCommandHandler>().expect("Expected AudioCommandHandler in TypeMap").clone();
        //     let mut audio_command_handler = audio_command_handler.lock().await;
        //     let tx = audio_command_handler.get_mut(&guild_id.to_string()).unwrap();
        //     tx.unbounded_send(AudioPromiseCommand::Stop).unwrap();
        // }
    }
    fn name(&self) -> &str {
        "stop"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &AutocompleteInteraction) -> Result<(), Error> {
        Ok(())
    }
}
