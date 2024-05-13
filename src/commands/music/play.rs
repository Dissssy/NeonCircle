use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;
use tokio::sync::Mutex;

use std::sync::Arc;
use std::time::Duration;

use crate::commands::music::{Author, MetaVideo};
use anyhow::{anyhow, Error};
use serenity::builder::CreateApplicationCommand;
use serenity::futures::channel::mpsc;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::model::prelude::command::CommandOptionType;

use serenity::prelude::Context;

use super::mainloop::the_lüüp;

use super::{AudioCommandHandler, AudioHandler, AudioPromiseCommand, LazyLoadedVideo, MessageReference, VideoType};

#[derive(Debug, Clone)]
pub struct Play;

#[serenity::async_trait]
impl crate::CommandTrait for Play {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command.name(self.name()).description("Play a song").create_option(|option| option.set_autocomplete(true).name("url").description("The url of the song to play").kind(CommandOptionType::String).required(true));
    }
    async fn run(&self, ctx: &Context, interaction: &serenity::model::prelude::application_command::ApplicationCommandInteraction) {
        if let Err(e) = interaction.create_interaction_response(&ctx.http, |response| response.interaction_response_data(|f| f.ephemeral(true)).kind(InteractionResponseType::DeferredChannelMessageWithSource)).await {
            println!("Failed to create interaction response: {:?}", e);
        };
        let guild_id = match interaction.guild_id {
            Some(id) => id,
            None => {
                if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command can only be used in a server")).await {
                    println!("Failed to edit original interaction response: {:?}", e);
                }
                return;
            }
        };

        let option = match interaction.data.options.iter().find(|o| o.name == "url") {
            Some(o) => match o.value.as_ref() {
                Some(v) => {
                    if let Some(v) = v.as_str() {
                        v
                    } else {
                        if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command requires an option")).await {
                            println!("Failed to edit original interaction response: {:?}", e);
                        }
                        return;
                    }
                }
                None => {
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command requires an option")).await {
                        println!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
            },
            None => {
                if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("This command requires an option")).await {
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
                super::VoiceAction::InSame(_channel) => {}
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
                        }
                    }
                }
            };

            let res = {
                let option = option.to_string();
                tokio::task::spawn(async move { crate::video::Video::get_video(&option, true, true).await }).await
            };
            let t = match res {
                Ok(Ok(t)) => Ok(t),
                Ok(Err(_e)) => {
                    let t = {
                        let option = option.to_string();
                        match tokio::task::spawn(async move { crate::youtube::search(option, 1).await }).await {
                            Ok(t) => t,
                            Err(e) => {
                                eprintln!("Error: {:?}", e);
                                return;
                            }
                        }
                    };

                    if let Some(vid) = t.first() {
                        let th = {
                            let url = vid.url.to_owned();
                            match tokio::task::spawn(async move { crate::video::Video::get_video(&url, true, false).await }).await {
                                Ok(t) => t,
                                Err(e) => {
                                    eprintln!("Error: {:?}", e);
                                    return;
                                }
                            }
                        };
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
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content(format!("Error: {:?}", e))).await {
                        println!("Failed to edit original interaction response: {:?}", e);
                    }
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
                            println!("Getting tts for {}", title);
                            truevideos.push(MetaVideo {
                                video: v,
                                ttsmsg: Some(LazyLoadedVideo::new(tokio::spawn(crate::youtube::get_tts(title.clone(), key.clone(), None)))),
                                title,
                                author: Author::from_user(ctx, &interaction.user, interaction.guild_id).await,
                            })
                        } else {
                            truevideos.push(MetaVideo { video: v, ttsmsg: None, title, author: Author::from_user(ctx, &interaction.user, interaction.guild_id).await });
                        }
                        #[cfg(not(feature = "tts"))]
                        truevideos.push(MetaVideo { video: v, title });
                    }

                    let data_read = ctx.data.read().await;
                    let audio_command_handler = data_read.get::<AudioCommandHandler>().expect("Expected AudioCommandHandler in TypeMap").clone();
                    let mut audio_command_handler = audio_command_handler.lock().await;
                    let tx = audio_command_handler.get_mut(&guild_id.to_string());
                    if let Some(tx) = tx {
                        let (rtx, rrx) = serenity::futures::channel::oneshot::channel::<String>();
                        if tx.unbounded_send((rtx, AudioPromiseCommand::Play(truevideos))).is_err() {
                            if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("Failed to send message to audio handler")).await {
                                println!("Failed to edit original interaction response: {:?}", e);
                            }
                        }

                        let timeout = tokio::time::timeout(Duration::from_secs(10), rrx).await;
                        if let Ok(Ok(msg)) = timeout {
                            if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content(msg)).await {
                                println!("Failed to edit original interaction response: {:?}", e);
                            }
                        } else if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("Timed out waiting for song to start playing")).await {
                            println!("Failed to edit original interaction response: {:?}", e);
                        }
                    } else {
                        audio_command_handler.remove(&guild_id.to_string());
                    }
                }
                Err(e) => {
                    if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content(format!("Error: {:?}", e))).await {
                        println!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
            }
        } else if let Err(e) = interaction.edit_original_interaction_response(&ctx.http, |response| response.content("TELL ETHAN THIS SHOULD NEVER HAPPEN :(")).await {
            println!("Failed to edit original interaction response: {:?}", e);
        }
    }
    fn name(&self) -> &str {
        "play"
    }

    #[allow(unused)]
    async fn autocomplete(&self, ctx: &Context, auto: &AutocompleteInteraction) -> Result<(), Error> {
        for op in auto.data.options.clone() {
            if op.focused && op.name == "url" {
                #[cfg(feature = "youtube-search")]
                {
                    let v = match op.value.as_ref().and_then(|v| v.as_str()) {
                        Some(v) => v,
                        None => {
                            continue;
                        }
                    };

                    if v.starts_with("http://") || v.starts_with("https://") {
                        let video = crate::video::Video::get_video(v, false, true).await?;

                        if let Some(vid) = video.first() {
                            auto.create_autocomplete_response(&ctx.http, |c| c.add_string_choice(vid.get_title(), v)).await?;
                        } else {
                            auto.create_autocomplete_response(&ctx.http, |c| c.add_string_choice("Could not retrieve title, select this option to use url anyways.", v)).await?;
                        }
                    } else {
                        let query = crate::youtube::youtube_search(v, crate::Config::get().autocomplete_limit).await;
                        if let Ok(query) = query {
                            if query.is_empty() {
                                auto.create_autocomplete_response(&ctx.http, |c| c.add_string_choice("Invalid url", "")).await?;
                            } else {
                                auto.create_autocomplete_response(&ctx.http, |c| {
                                    let mut c = c;
                                    for (i, q) in query.iter().enumerate() {
                                        if i > 25 {
                                            break;
                                        }
                                        c = c.add_string_choice(
                                            format!(
                                                "{} {}{}",
                                                if q.duration.is_some() { "🎵" } else { "📼" },
                                                q.title,
                                                match q.uploader.as_ref() {
                                                    Some(u) => format!(" - {}", u),
                                                    None => "".to_string(),
                                                }
                                            ),
                                            q.url.clone(),
                                        );
                                    }
                                    c
                                })
                                .await?;
                            }
                        } else {
                            auto.create_autocomplete_response(&ctx.http, |c| c.add_string_choice("Invalid url", "")).await?;
                        }
                    }
                }
                #[cfg(not(feature = "youtube-search"))]
                {
                    auto.create_autocomplete_response(&ctx.http, |c| c.add_string_choice("Live search functionality not enabled.", "")).await?;
                }
            }
        }
        Ok(())
    }
}
