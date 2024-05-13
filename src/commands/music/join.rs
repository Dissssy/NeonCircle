use std::sync::Arc;

use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;

use anyhow::Error;
use serenity::builder::CreateApplicationCommand;
use serenity::futures::channel::mpsc;
use serenity::model::application::interaction::InteractionResponseType;

use serenity::prelude::Context;
use tokio::sync::Mutex;

use super::mainloop::the_lüüp;

use super::{AudioCommandHandler, AudioHandler, AudioPromiseCommand, MessageReference};

#[derive(Debug, Clone)]
pub struct Join;

#[serenity::async_trait]
impl crate::CommandTrait for Join {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command.name(self.name()).description("Join vc");
    }
    async fn run(&self, ctx: &Context, interaction: &serenity::model::prelude::application_command::ApplicationCommandInteraction) {
        if let Err(e) = interaction.create_interaction_response(&ctx.http, |response| response.interaction_response_data(|f| f.ephemeral(true)).kind(InteractionResponseType::DeferredChannelMessageWithSource)).await {
            println!("Failed to create interaction response: {:?}", e);
        }
        let guild_id = match interaction.guild_id {
            Some(id) => id,
            None => {
                if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command can only be used in a server")).await {
                    println!("Failed to edit original interaction response: {:?}", e);
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
                        println!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::InDifferent(_channel) => {
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("I'm in a different voice channel")).await {
                        println!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::InSame(_channel) => {
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("I'm already in the same voice channel as you, what do you want from me?")).await {
                        println!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::Join(channel) => {
                    let manager = songbird::get(ctx).await.expect("Songbird Voice client placed in at initialisation.").clone();
                    {
                        let audio_handler = { ctx.data.read().await.get::<AudioHandler>().expect("Expected AudioHandler in TypeMap").clone() };
                        let mut audio_handler = audio_handler.lock().await;
                        let (call, result) = manager.join(guild_id, channel).await;
                        if result.is_ok() {
                            let (tx, rx) = mpsc::unbounded::<(serenity::futures::channel::oneshot::Sender<String>, AudioPromiseCommand)>();
                            let msg = match interaction.channel_id.send_message(&ctx.http, |m| m.content("Joining voice channel").flags(serenity::model::channel::MessageFlags::from_bits(1u64 << 12).expect("Failed to create message flags"))).await {
                                Ok(msg) => msg,
                                Err(e) => {
                                    println!("Failed to send message: {:?}", e);
                                    return;
                                }
                            };
                            let messageref = MessageReference::new(ctx.http.clone(), ctx.cache.clone(), guild_id, msg.channel_id, msg);
                            let cfg = crate::Config::get();
                            let mut nothing_path = cfg.data_path.clone();
                            nothing_path.push("override.mp3");

                            let nothing_path = if nothing_path.exists() { Some(nothing_path) } else { None };

                            let guild_id = match interaction.guild_id {
                                Some(guild) => guild,
                                None => return,
                            };

                            let em = match ctx.data.read().await.get::<super::transcribe::TranscribeData>().expect("Expected TranscribeData in TypeMap.").lock().await.entry(guild_id) {
                                std::collections::hash_map::Entry::Occupied(ref mut e) => e.get_mut(),
                                std::collections::hash_map::Entry::Vacant(e) => e.insert(Arc::new(Mutex::new(super::transcribe::TranscribeChannelHandler::new()))),
                            }
                            .clone();

                            if let Err(e) = em.lock().await.register(channel).await {
                                println!("Error registering channel: {:?}", e);
                            }

                            let http = Arc::clone(&ctx.http);

                            let handle = {
                                let tx = tx.clone();
                                tokio::task::spawn(async move {
                                    the_lüüp(call, rx, tx, messageref, cfg.looptime, nothing_path, em, http).await;
                                })
                            };

                            audio_handler.insert(guild_id.to_string(), handle);
                            let audio_command_handler = {
                                let read_lock = ctx.data.read().await;
                                read_lock.get::<AudioCommandHandler>().expect("Expected AudioCommandHandler in TypeMap").clone()
                            };
                            let mut audio_command_handler = audio_command_handler.lock().await;
                            audio_command_handler.insert(guild_id.to_string(), tx);

                            if let Err(e) = interaction.delete_original_interaction_response(&ctx.http).await {
                                println!("Error deleting interaction: {:?}", e);
                            }
                        }
                    }
                }
            }
        } else if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("TELL ETHAN THIS SHOULD NEVER HAPPEN :(")).await {
            println!("Failed to edit original interaction response: {:?}", e);
        }
    }
    fn name(&self) -> &str {
        "join"
    }

    async fn autocomplete(&self, _ctx: &Context, _auto: &AutocompleteInteraction) -> Result<(), Error> {
        Ok(())
    }
}
