use std::sync::Arc;

use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;

use anyhow::Error;
use serenity::builder::CreateApplicationCommand;
use serenity::futures::channel::mpsc;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};

use serenity::prelude::Context;
use tokio::sync::Mutex;

use super::mainloop::the_lüüp;

use super::{
    get_mutual_voice_channel, AudioCommandHandler, AudioHandler, AudioPromiseCommand,
    MessageReference,
};

#[derive(Debug, Clone)]
pub struct Join;

#[serenity::async_trait]
impl crate::CommandTrait for Join {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command.name(self.name()).description("Join vc");
    }
    async fn run(&self, ctx: &Context, rawinteraction: Interaction) {
        let interaction = rawinteraction.application_command().unwrap();
        // check if the promise for this guild exists
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response
                    .interaction_response_data(|f| f.ephemeral(true))
                    .kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await
            .unwrap();
        let guild_id = interaction.guild_id;
        if let Some(guild_id) = guild_id {
            let mutual = get_mutual_voice_channel(ctx, &interaction).await;
            // get the voice state for the user that issued the command

            if let Some((joins, channel_id)) = mutual {
                let manager = songbird::get(ctx)
                    .await
                    .expect("Songbird Voice client placed in at initialisation.")
                    .clone();
                {
                    let data_read = ctx.data.read().await;
                    let audio_handler = data_read
                        .get::<AudioHandler>()
                        .expect("Expected AudioHandler in TypeMap")
                        .clone();
                    let mut audio_handler = audio_handler.lock().await;

                    // if let std::collections::hash_map::Entry::Vacant(e) = audio_handler.entry(guild_id.to_string()) {
                    if joins {
                        let (call, result) = manager.join(guild_id, channel_id).await;
                        if result.is_ok() {
                            let (tx, mut rx) = mpsc::unbounded::<(
                                mpsc::UnboundedSender<String>,
                                AudioPromiseCommand,
                            )>();
                            // create the promise. this will be used for holding on to the audio connection and handling commands
                            // interaction
                            //     .edit_original_interaction_response(&ctx.http, |response| response.content("Joining voice channel"))
                            //     .await
                            //     .unwrap();
                            // send new message in channel
                            let msg = interaction
                                .channel_id
                                .send_message(&ctx.http, |m| {
                                    m.content("Joining voice channel").flags(
                                        serenity::model::channel::MessageFlags::from_bits(
                                            1u64 << 12,
                                        )
                                        .expect("Failed to create message flags"),
                                    )
                                })
                                .await
                                .unwrap();
                            let messageref = MessageReference::new(
                                ctx.http.clone(),
                                ctx.cache.clone(),
                                guild_id,
                                msg.channel_id,
                                msg,
                            );
                            let cfg = crate::Config::get();
                            let mut nothing_path = cfg.data_path.clone();
                            nothing_path.push("override.mp3");
                            // check if the override file exists
                            let nothing_path = if nothing_path.exists() {
                                Some(nothing_path)
                            } else {
                                None
                            };

                            let guild_id = match interaction.guild_id {
                                Some(guild) => guild,
                                None => return,
                            };

                            drop(data_read);
                            let em = {
                                let mut data_write = ctx.data.write().await;
                                let mut f = data_write
                                    .get_mut::<super::transcribe::TranscribeData>()
                                    .expect("Expected TranscribeData in TypeMap.")
                                    .lock()
                                    .await;
                                let mut entry = f.entry(guild_id);
                                match entry {
                                    std::collections::hash_map::Entry::Occupied(ref mut e) => {
                                        e.get_mut()
                                    }
                                    std::collections::hash_map::Entry::Vacant(e) => {
                                        e.insert(Arc::new(Mutex::new(
                                            super::transcribe::TranscribeChannelHandler::new(),
                                        )))
                                    }
                                }
                                .clone()
                            };
                            let data_read = ctx.data.read().await;

                            let handle = tokio::task::spawn(async move {
                                the_lüüp(
                                    call,
                                    &mut rx,
                                    messageref,
                                    cfg.looptime,
                                    nothing_path,
                                    em,
                                )
                                .await;
                            });
                            // let (handle, producer) = self.begin_joinback(ctx, guild_id).await;
                            // e.insert(handle);
                            audio_handler.insert(guild_id.to_string(), handle);
                            let audio_command_handler = data_read
                                .get::<AudioCommandHandler>()
                                .expect("Expected AudioCommandHandler in TypeMap")
                                .clone();
                            let mut audio_command_handler = audio_command_handler.lock().await;
                            audio_command_handler.insert(guild_id.to_string(), tx);

                            if let Err(e) = interaction
                                .delete_original_interaction_response(&ctx.http)
                                .await
                            {
                                println!("Error deleting interaction: {:?}", e);
                            }
                        }
                    }
                }
            } else {
                interaction
                    .edit_original_interaction_response(&ctx.http, |response| {
                        response.content("You must be in a voice channel to use this command")
                    })
                    .await
                    .unwrap();
            }
        } else {
            interaction
                .edit_original_interaction_response(&ctx.http, |response| {
                    response.content("This command can only be used in a guild")
                })
                .await
                .unwrap();
        }
    }
    fn name(&self) -> &str {
        "join"
    }
    // allow unused code when youtube-search feature is not enabled
    #[allow(unused)]
    async fn autocomplete(
        &self,
        ctx: &Context,
        auto: &AutocompleteInteraction,
    ) -> Result<(), Error> {
        for op in auto.data.options.clone() {
            if op.focused {
                // get the search term
                if op.name == "url" {
                    #[cfg(feature = "youtube-search")]
                    {
                        let v = op.value.as_ref().unwrap().as_str().unwrap().to_owned();
                        // let title: Option<String> = None; // = crate::youtube::get_url_title(v.clone()).await;
                        // if let Some(title) = title {
                        //     // auto.create_autocomplete_response(&ctx.http, |c| {
                        //     //     c.add_string_choice(title, v)
                        //     // })
                        //     // .await?;
                        // } else {

                        let query = crate::youtube::youtube_search(v).await;
                        if let Ok(query) = query {
                            if query.is_empty() {
                                auto.create_autocomplete_response(&ctx.http, |c| {
                                    c.add_string_choice("Invalid url", "")
                                })
                                .await?;
                            } else {
                                auto.create_autocomplete_response(&ctx.http, |c| {
                                    let mut c = c;
                                    for (i, q) in query.iter().enumerate() {
                                        if i > 25 {
                                            break;
                                        }
                                        c = c.add_string_choice(q.title.clone(), q.url.clone());
                                    }
                                    c
                                })
                                .await?;
                            }
                        } else {
                            auto.create_autocomplete_response(&ctx.http, |c| {
                                c.add_string_choice("Invalid url", "")
                            })
                            .await?;
                        }
                        // }
                    }
                    #[cfg(not(feature = "youtube-search"))]
                    {
                        auto.create_autocomplete_response(&ctx.http, |c| {
                            c.add_string_choice("Live search functionality not enabled.", "")
                        })
                        .await?;
                    }
                }
            }
        }
        Ok(())
    }
}
