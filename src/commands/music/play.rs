use serenity::futures::StreamExt;

use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;

use std::time::Duration;

use crate::commands::music::MetaVideo;
use anyhow::{anyhow, Error};
use serenity::builder::CreateApplicationCommand;
use serenity::futures::channel::mpsc;
use serenity::model::application::interaction::{Interaction, InteractionResponseType};
use serenity::model::prelude::command::CommandOptionType;

use serenity::prelude::Context;

use super::mainloop::the_lüüp;

use super::{
    get_mutual_voice_channel, AudioCommandHandler, AudioHandler, AudioPromiseCommand,
    MessageReference, VideoType,
};

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
                                .send_message(&ctx.http, |m| m.content("Joining voice channel"))
                                .await
                                .unwrap();
                            let messageref = MessageReference::new(
                                ctx.http.clone(),
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
                            let handle = tokio::task::spawn(async move {
                                the_lüüp(call, &mut rx, messageref, cfg.looptime, nothing_path)
                                    .await;
                            });
                            // let (handle, producer) = self.begin_playback(ctx, guild_id).await;
                            // e.insert(handle);
                            audio_handler.insert(guild_id.to_string(), handle);
                            let audio_command_handler = data_read
                                .get::<AudioCommandHandler>()
                                .expect("Expected AudioCommandHandler in TypeMap")
                                .clone();
                            let mut audio_command_handler = audio_command_handler.lock().await;
                            audio_command_handler.insert(guild_id.to_string(), tx);
                        }
                    }
                    let options = interaction.data.options.clone();
                    let urloption = options[0]
                        .value
                        .as_ref()
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_owned();
                    // #[cfg(feature = "download")]
                    let t = tokio::task::spawn(crate::video::Video::get_video(
                        urloption.clone(),
                        true,
                        true,
                    ))
                    .await
                    .unwrap();
                    // #[cfg(not(feature = "download"))]
                    // let t = tokio::task::spawn(crate::youtube::get_video_info(options[0].value.as_ref().unwrap().as_str().unwrap().to_owned()))
                    //     .await
                    //     .unwrap();
                    let videos: Result<Vec<VideoType>, Error> = if let Ok(videos) = t {
                        Ok(videos)
                    } else {
                        // search youtube for a video
                        let t = tokio::task::spawn(crate::youtube::search(urloption))
                            .await
                            .unwrap();
                        // get the first video
                        if !t.is_empty() {
                            let vid = t[0].clone();
                            let th = tokio::task::spawn(crate::video::Video::get_video(
                                vid.url.clone(),
                                true,
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
                    };
                    let mut truevideos = Vec::new();
                    #[cfg(feature = "tts")]
                    let key = crate::youtube::get_access_token().await;
                    if let Ok(videos) = videos {
                        for v in videos {
                            let title = match v.clone() {
                                VideoType::Disk(v) => v.title,
                                VideoType::Url(v) => v.title,
                            };
                            #[cfg(feature = "tts")]
                            if let Ok(key) = key.as_ref() {
                                let t = tokio::task::spawn(crate::youtube::get_tts(
                                    title.clone(),
                                    key.clone(),
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
                                        response
                                            .content("Timed out waiting for song to start playing")
                                    })
                                    .await
                                    .unwrap();
                            }
                        } else {
                            // delete the tx
                            audio_command_handler.remove(&guild_id.to_string());
                        }
                    } else {
                        interaction
                            .edit_original_interaction_response(&ctx.http, |response| {
                                response.content(videos.unwrap_err())
                            })
                            .await
                            .unwrap();
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
