use serenity::futures::StreamExt;

use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;
use tokio::sync::Mutex;
// use songbird::driver::Bitrate;

use std::sync::Arc;
use std::time::Duration;

use crate::commands::music::MetaVideo;
use anyhow::{anyhow, Error};
use serenity::builder::CreateApplicationCommand;
use serenity::futures::channel::mpsc;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::prelude::command::CommandOptionType;

use serenity::prelude::Context;

use super::mainloop::the_lüüp;

use super::{AudioCommandHandler, AudioHandler, AudioPromiseCommand, MessageReference, VideoType};

#[derive(Debug, Clone)]
pub struct Play;

#[serenity::async_trait]
impl crate::CommandTrait for Play {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command
            .name(self.name())
            .description("Play a song")
            .create_option(|option| {
                option
                    .set_autocomplete(true)
                    .name("url")
                    .description("The url of the song to play")
                    .kind(CommandOptionType::String)
                    .required(true)
            });
    }
    async fn run(&self, ctx: &Context, rawinteraction: Interaction) {
        let interaction = rawinteraction.application_command().unwrap();
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

        let option = match interaction.data.options.iter().find(|o| o.name == "url") {
            Some(o) => match o.value.as_ref() {
                Some(v) => {
                    if let Some(v) = v.as_str() {
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

        if let (Some(v), Some(member)) = (
            ctx.data.read().await.get::<super::VoiceData>(),
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
                super::VoiceAction::InSame(_channel) => {}
                super::VoiceAction::Join(channel) => {
                    let manager = songbird::get(ctx)
                        .await
                        .expect("Songbird Voice client placed in at initialisation.")
                        .clone();
                    {
                        let audio_handler = {
                            ctx.data
                                .read()
                                .await
                                .get::<AudioHandler>()
                                .expect("Expected AudioHandler in TypeMap")
                                .clone()
                        };
                        let mut audio_handler = audio_handler.lock().await;
                        let (call, result) = manager.join(guild_id, channel).await;
                        if result.is_ok() {
                            let (tx, mut rx) = mpsc::unbounded::<(
                                mpsc::UnboundedSender<String>,
                                AudioPromiseCommand,
                            )>();
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

                            let em = match ctx
                                .data
                                .read()
                                .await
                                .get::<super::transcribe::TranscribeData>()
                                .expect("Expected TranscribeData in TypeMap.")
                                .lock()
                                .await
                                .entry(guild_id)
                            {
                                std::collections::hash_map::Entry::Occupied(ref mut e) => {
                                    e.get_mut()
                                }
                                std::collections::hash_map::Entry::Vacant(e) => e.insert(Arc::new(
                                    Mutex::new(super::transcribe::TranscribeChannelHandler::new()),
                                )),
                            }
                            .clone();

                            if let Err(e) = em.lock().await.register(channel).await {
                                println!("Error registering channel: {:?}", e);
                            }

                            // let em = match write_lock
                            //     .get_mut::<super::transcribe::TranscribeData>()
                            //     .expect("Expected TranscribeData in TypeMap.")
                            //     .lock()
                            //     .await
                            //     .entry(guild_id)
                            // {
                            //     std::collections::hash_map::Entry::Occupied(ref mut e) => {
                            //         e.get_mut()
                            //     }
                            //     std::collections::hash_map::Entry::Vacant(e) => e.insert(Arc::new(
                            //         Mutex::new(super::transcribe::TranscribeChannelHandler::new()),
                            //     )),
                            // }
                            // .clone();

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
                            let audio_command_handler = {
                                let read_lock = ctx.data.read().await;
                                read_lock
                                    .get::<AudioCommandHandler>()
                                    .expect("Expected AudioCommandHandler in TypeMap")
                                    .clone()
                            };
                            let mut audio_command_handler = audio_command_handler.lock().await;
                            audio_command_handler.insert(guild_id.to_string(), tx);
                        }
                    }
                }
            };

            let t =
                match tokio::task::spawn(crate::video::Video::get_video(option.to_owned(), true))
                    .await
                {
                    Ok(Ok(t)) => Ok(t),
                    Ok(Err(_e)) => {
                        // search youtube for a video
                        let t = tokio::task::spawn(crate::youtube::search(option.to_owned(), 1))
                            .await
                            .unwrap();
                        // get the first video
                        if !t.is_empty() {
                            let vid = t[0].clone();
                            let th = tokio::task::spawn(crate::video::Video::get_video(
                                vid.url.clone(),
                                true,
                            ))
                            .await
                            .unwrap();
                            if let Ok(vids) = th {
                                Ok(vids)
                            } else {
                                Err(anyhow!("Could not get video info"))
                            }
                        } else {
                            Err(anyhow!("No videos found for that query"))
                        }
                    }
                    Err(e) => {
                        interaction
                            .edit_original_interaction_response(&ctx.http, |response| {
                                response.content(format!("Error: {:?}", e))
                            })
                            .await
                            .unwrap();
                        return;
                    }
                };

            match t {
                Ok(rawvids) => {
                    let mut truevideos = Vec::new();
                    #[cfg(feature = "tts")]
                    let key = crate::youtube::get_access_token().await;
                    for v in rawvids {
                        let title = match v.clone() {
                            VideoType::Disk(v) => v.title,
                            VideoType::Url(v) => v.title,
                        };
                        #[cfg(feature = "tts")]
                        if let Ok(key) = key.as_ref() {
                            let t = tokio::task::spawn(crate::youtube::get_tts(
                                title.clone(),
                                key.clone(),
                                None,
                            ))
                            .await
                            .unwrap();
                            if let Ok(tts) = t {
                                match tts {
                                    VideoType::Disk(tts) => {
                                        truevideos.push(MetaVideo {
                                            video: v,
                                            ttsmsg: Some(tts),
                                            title,
                                        });
                                    }
                                    VideoType::Url(_) => {
                                        unreachable!("TTS should always be a disk file");
                                    }
                                }
                            } else {
                                println!("Error {:?}", t);
                                truevideos.push(MetaVideo {
                                    video: v,
                                    ttsmsg: None,
                                    title,
                                });
                            }
                        } else {
                            truevideos.push(MetaVideo {
                                video: v,
                                ttsmsg: None,
                                title,
                            });
                        }
                        #[cfg(not(feature = "tts"))]
                        truevideos.push(MetaVideo { video: v, title });
                    }

                    // interaction.edit_original_interaction_response(&ctx.http, |response| response.content("Playing song")).await.unwrap();
                    let data_read = ctx.data.read().await;
                    let audio_command_handler = data_read
                        .get::<AudioCommandHandler>()
                        .expect("Expected AudioCommandHandler in TypeMap")
                        .clone();
                    let mut audio_command_handler = audio_command_handler.lock().await;
                    let tx = audio_command_handler.get_mut(&guild_id.to_string());
                    if let Some(tx) = tx {
                        let (rtx, mut rrx) = mpsc::unbounded::<String>();
                        tx.unbounded_send((rtx, AudioPromiseCommand::Play(truevideos)))
                            .unwrap();
                        // wait for up to 10 seconds for the rrx to receive a message
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
                                    response.content("Timed out waiting for song to start playing")
                                })
                                .await
                                .unwrap();
                        }
                    } else {
                        // delete the tx
                        audio_command_handler.remove(&guild_id.to_string());
                    }
                }
                Err(e) => {
                    interaction
                        .edit_original_interaction_response(&ctx.http, |response| {
                            response.content(format!("Error: {:?}", e))
                        })
                        .await
                        .unwrap();
                    return;
                }
            }

            //logic to add the video to the queue
        } else {
            interaction
                .edit_original_interaction_response(&ctx.http, |response| {
                    response.content("TELL ETHAN THIS SHOULD NEVER HAPPEN :(")
                })
                .await
                .unwrap();
        }

        // let interaction = rawinteraction.application_command().unwrap();
        // // check if the promise for this guild exists
        // interaction
        //     .create_interaction_response(&ctx.http, |response| {
        //         response
        //             .interaction_response_data(|f| f.ephemeral(true))
        //             .kind(InteractionResponseType::DeferredChannelMessageWithSource)
        //     })
        //     .await
        //     .unwrap();
        // let guild_id = interaction.guild_id;
        // if let Some(guild_id) = guild_id {
        //     let mutual = get_mutual_voice_channel(ctx, &interaction).await;
        //     // get the voice state for the user that issued the command

        //     if let Some((joins, channel_id)) = mutual {
        //         let manager = songbird::get(ctx)
        //             .await
        //             .expect("Songbird Voice client placed in at initialisation.")
        //             .clone();
        //         {
        //             // if let std::collections::hash_map::Entry::Vacant(e) = audio_handler.entry(guild_id.to_string()) {
        //             if joins {
        //                 let (call, result) = manager.join(guild_id, channel_id).await;
        //                 if result.is_ok() {
        //                     let (tx, mut rx) = mpsc::unbounded::<(
        //                         mpsc::UnboundedSender<String>,
        //                         AudioPromiseCommand,
        //                     )>();
        //                     // create the promise. this will be used for holding on to the audio connection and handling commands
        //                     // interaction
        //                     //     .edit_original_interaction_response(&ctx.http, |response| response.content("Joining voice channel"))
        //                     //     .await
        //                     //     .unwrap();
        //                     // send new message in channel
        //                     let msg = interaction
        //                         .channel_id
        //                         .send_message(&ctx.http, |m| {
        //                             m.content("Joining voice channel").flags(
        //                                 serenity::model::channel::MessageFlags::from_bits(
        //                                     1u64 << 12,
        //                                 )
        //                                 .expect("Failed to create message flags"),
        //                             )
        //                         })
        //                         .await
        //                         .unwrap();
        //                     let messageref = MessageReference::new(
        //                         ctx.http.clone(),
        //                         ctx.cache.clone(),
        //                         guild_id,
        //                         msg.channel_id,
        //                         msg,
        //                     );
        //                     let cfg = crate::Config::get();
        //                     let mut nothing_path = cfg.data_path.clone();
        //                     nothing_path.push("override.mp3");
        //                     // check if the override file exists
        //                     let nothing_path = if nothing_path.exists() {
        //                         Some(nothing_path)
        //                     } else {
        //                         None
        //                     };

        //                     let guild_id = match interaction.guild_id {
        //                         Some(guild) => guild,
        //                         None => return,
        //                     };
        //                     let em = match ctx
        //                         .data
        //                         .write()
        //                         .await
        //                         .get_mut::<super::transcribe::TranscribeData>()
        //                         .expect("Expected TranscribeData in TypeMap.")
        //                         .lock()
        //                         .await
        //                         .entry(guild_id)
        //                     {
        //                         std::collections::hash_map::Entry::Occupied(ref mut e) => {
        //                             e.get_mut()
        //                         }
        //                         std::collections::hash_map::Entry::Vacant(e) => e.insert(Arc::new(
        //                             Mutex::new(super::transcribe::TranscribeChannelHandler::new()),
        //                         )),
        //                     }
        //                     .clone();

        //                     let handle = tokio::task::spawn(async move {
        //                         the_lüüp(
        //                             call,
        //                             &mut rx,
        //                             messageref,
        //                             cfg.looptime,
        //                             nothing_path,
        //                             em,
        //                         )
        //                         .await;
        //                     });
        //                     // let (handle, producer) = self.begin_playback(ctx, guild_id).await;
        //                     // e.insert(handle);
        //                     let audio_handler = {
        //                         ctx.data
        //                             .read()
        //                             .await
        //                             .get::<AudioHandler>()
        //                             .expect("Expected AudioHandler in TypeMap")
        //                             .clone()
        //                     };
        //                     let mut audio_handler = audio_handler.lock().await;
        //                     audio_handler.insert(guild_id.to_string(), handle);
        //                     let audio_command_handler = ctx
        //                         .data
        //                         .read()
        //                         .await
        //                         .get::<AudioCommandHandler>()
        //                         .expect("Expected AudioCommandHandler in TypeMap")
        //                         .clone();
        //                     let mut audio_command_handler = audio_command_handler.lock().await;
        //                     audio_command_handler.insert(guild_id.to_string(), tx);
        //                 }
        //             }
        //             let options = interaction.data.options.clone();
        //             let urloption = options[0]
        //                 .value
        //                 .as_ref()
        //                 .unwrap()
        //                 .as_str()
        //                 .unwrap()
        //                 .to_owned();
        //             // #[cfg(feature = "download")]
        //             let t =
        //                 tokio::task::spawn(crate::video::Video::get_video(urloption.clone(), true))
        //                     .await
        //                     .unwrap();

        //             // #[cfg(not(feature = "download"))]
        //             // let t = tokio::task::spawn(crate::youtube::get_video_info(
        //             //     options[0]
        //             //         .value
        //             //         .as_ref()
        //             //         .unwrap()
        //             //         .as_str()
        //             //         .unwrap()
        //             //         .to_owned(),
        //             // ))
        //             // .await
        //             // .unwrap();
        //             let videos: Result<Vec<VideoType>, Error> = if let Ok(videos) = t {
        //                 Ok(videos)
        //             } else {
        //                 // search youtube for a video
        //                 let t = tokio::task::spawn(crate::youtube::search(urloption, 1))
        //                     .await
        //                     .unwrap();
        //                 // get the first video
        //                 if !t.is_empty() {
        //                     let vid = t[0].clone();
        //                     let th = tokio::task::spawn(crate::video::Video::get_video(
        //                         vid.url.clone(),
        //                         true,
        //                     ))
        //                     .await
        //                     .unwrap();
        //                     if let Ok(vids) = th {
        //                         Ok(vids)
        //                     } else {
        //                         Err(anyhow!("Could not get video info"))
        //                     }
        //                 } else {
        //                     Err(anyhow!("No videos found for that query"))
        //                 }
        //             };

        //
        //
        //
        //

        //             let mut truevideos = Vec::new();
        //             #[cfg(feature = "tts")]
        //             let key = crate::youtube::get_access_token().await;
        //             if let Ok(videos) = videos {
        //                 for v in videos {
        //                     let title = match v.clone() {
        //                         VideoType::Disk(v) => v.title,
        //                         VideoType::Url(v) => v.title,
        //                     };
        //                     #[cfg(feature = "tts")]
        //                     if let Ok(key) = key.as_ref() {
        //                         let t = tokio::task::spawn(crate::youtube::get_tts(
        //                             title.clone(),
        //                             key.clone(),
        //                             None,
        //                         ))
        //                         .await
        //                         .unwrap();
        //                         if let Ok(tts) = t {
        //                             match tts {
        //                                 VideoType::Disk(tts) => {
        //                                     truevideos.push(MetaVideo {
        //                                         video: v,
        //                                         ttsmsg: Some(tts),
        //                                         title,
        //                                     });
        //                                 }
        //                                 VideoType::Url(_) => {
        //                                     unreachable!("TTS should always be a disk file");
        //                                 }
        //                             }
        //                         } else {
        //                             println!("Error {:?}", t);
        //                             truevideos.push(MetaVideo {
        //                                 video: v,
        //                                 ttsmsg: None,
        //                                 title,
        //                             });
        //                         }
        //                     } else {
        //                         truevideos.push(MetaVideo {
        //                             video: v,
        //                             ttsmsg: None,
        //                             title,
        //                         });
        //                     }
        //                     #[cfg(not(feature = "tts"))]
        //                     truevideos.push(MetaVideo { video: v, title });
        //                 }

        //                 // interaction.edit_original_interaction_response(&ctx.http, |response| response.content("Playing song")).await.unwrap();
        //                 let data_read = ctx.data.read().await;
        //                 let audio_command_handler = data_read
        //                     .get::<AudioCommandHandler>()
        //                     .expect("Expected AudioCommandHandler in TypeMap")
        //                     .clone();
        //                 let mut audio_command_handler = audio_command_handler.lock().await;
        //                 let tx = audio_command_handler.get_mut(&guild_id.to_string());
        //                 if let Some(tx) = tx {
        //                     let (rtx, mut rrx) = mpsc::unbounded::<String>();
        //                     tx.unbounded_send((rtx, AudioPromiseCommand::Play(truevideos)))
        //                         .unwrap();
        //                     // wait for up to 10 seconds for the rrx to receive a message
        //                     let timeout =
        //                         tokio::time::timeout(Duration::from_secs(10), rrx.next()).await;
        //                     if let Ok(Some(msg)) = timeout {
        //                         interaction
        //                             .edit_original_interaction_response(&ctx.http, |response| {
        //                                 response.content(msg)
        //                             })
        //                             .await
        //                             .unwrap();
        //                     } else {
        //                         interaction
        //                             .edit_original_interaction_response(&ctx.http, |response| {
        //                                 response
        //                                     .content("Timed out waiting for song to start playing")
        //                             })
        //                             .await
        //                             .unwrap();
        //                     }
        //                 } else {
        //                     // delete the tx
        //                     audio_command_handler.remove(&guild_id.to_string());
        //                 }
        //             } else {
        //                 interaction
        //                     .edit_original_interaction_response(&ctx.http, |response| {
        //                         response.content(videos.unwrap_err())
        //                     })
        //                     .await
        //                     .unwrap();
        //             }
        //         }
        //     } else {
        //         interaction
        //             .edit_original_interaction_response(&ctx.http, |response| {
        //                 response.content("You must be in a voice channel to use this command")
        //             })
        //             .await
        //             .unwrap();
        //     }
        // } else {
        //     interaction
        //         .edit_original_interaction_response(&ctx.http, |response| {
        //             response.content("This command can only be used in a guild")
        //         })
        //         .await
        //         .unwrap();
        // }
    }
    fn name(&self) -> &str {
        "play"
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
