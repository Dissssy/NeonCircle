use super::{AudioCommandHandler, AudioHandler, AudioPromiseCommand};
use crate::commands::music::{Author, LazyLoadedVideo, MetaVideo};
use anyhow::Error;
use serenity::all::*;
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot, Mutex};
#[derive(Debug, Clone)]
pub struct Add;
#[async_trait]
impl crate::CommandTrait for Add {
    fn register(&self) -> CreateCommand {
        CreateCommand::new(self.name())
            .description("Add a song to the queue")
            .set_options(vec![CreateCommandOption::new(
                CommandOptionType::String,
                "search",
                "Search youtube or provide a url (non youtube works as well)",
            )
            .required(true)])
    }
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) {
        if let Err(e) = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Defer(
                    CreateInteractionResponseMessage::new().ephemeral(true),
                ),
            )
            .await
        {
            println!("Failed to create interaction response: {:?}", e);
        }
        let guild_id = match interaction.guild_id {
            Some(id) => id,
            None => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new()
                            .content("This command can only be used in a server"),
                    )
                    .await
                {
                    println!("Failed to edit original interaction response: {:?}", e);
                }
                return;
            }
        };
        let options = interaction.data.options();
        let option = match options.iter().find_map(|o| match o.name {
            "search" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::String(s)) => s,
            _ => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content("This command requires an option"),
                    )
                    .await
                {
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
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content("You're not in a voice channel"),
                        )
                        .await
                    {
                        println!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::InDifferent(_channel) => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new()
                                .content("I'm in a different voice channel"),
                        )
                        .await
                    {
                        println!("Failed to edit original interaction response: {:?}", e);
                    }
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
                        match manager.join(guild_id, channel).await {
                            Ok(call) => {
                                let (tx, rx) = mpsc::unbounded_channel::<(
                                    oneshot::Sender<String>,
                                    AudioPromiseCommand,
                                )>();
                                let msg = match interaction
                                    .channel_id
                                    .send_message(
                                        &ctx.http,
                                        CreateMessage::new()
                                            .content("Joining voice channel")
                                            .flags(MessageFlags::SUPPRESS_NOTIFICATIONS),
                                    )
                                    .await
                                {
                                    Ok(msg) => msg,
                                    Err(e) => {
                                        println!("Failed to send message: {:?}", e);
                                        if let Err(e) = interaction
                                            .edit_response(
                                                &ctx.http,
                                                EditInteractionResponse::new()
                                                    .content("Failed to send message"),
                                            )
                                            .await
                                        {
                                            println!("Failed to edit original interaction response: {:?}", e);
                                        }
                                        return;
                                    }
                                };
                                let messageref = super::MessageReference::new(
                                    ctx.http.clone(),
                                    ctx.cache.clone(),
                                    guild_id,
                                    msg.channel_id,
                                    msg,
                                );
                                let cfg = crate::Config::get();
                                let mut nothing_path = cfg.data_path.clone();
                                nothing_path.push("override.mp3");
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
                                    std::collections::hash_map::Entry::Vacant(e) => {
                                        e.insert(Arc::new(Mutex::new(
                                            super::transcribe::TranscribeChannelHandler::new(),
                                        )))
                                    }
                                }
                                .clone();
                                if let Err(e) = em.lock().await.register(channel).await {
                                    println!("Error registering channel: {:?}", e);
                                }
                                let http = Arc::clone(&ctx.http);
                                let handle = {
                                    let tx = tx.clone();
                                    tokio::task::spawn(async move {
                                        super::mainloop::the_lüüp(
                                            call,
                                            rx,
                                            tx,
                                            messageref,
                                            cfg.looptime,
                                            nothing_path,
                                            em,
                                            http,
                                        )
                                        .await;
                                    })
                                };
                                audio_handler.insert(guild_id.to_string(), handle);
                                let audio_command_handler = {
                                    let read_lock = ctx.data.read().await;
                                    read_lock
                                        .get::<super::AudioCommandHandler>()
                                        .expect("Expected AudioCommandHandler in TypeMap")
                                        .clone()
                                };
                                let mut audio_command_handler = audio_command_handler.lock().await;
                                audio_command_handler.insert(guild_id.to_string(), tx);
                                if let Err(e) = interaction.delete_response(&ctx.http).await {
                                    println!("Error deleting interaction: {:?}", e);
                                }
                            }
                            Err(e) => {
                                println!("Failed to join channel: {:?}", e);
                                if let Err(e) = interaction
                                    .edit_response(
                                        &ctx.http,
                                        EditInteractionResponse::new()
                                            .content("Failed to join voice channel"),
                                    )
                                    .await
                                {
                                    println!(
                                        "Failed to edit original interaction response: {:?}",
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
            };
            let res = {
                let option = option.to_string();
                tokio::task::spawn(async move {
                    crate::video::Video::get_video(&option, true, true).await
                })
                .await
            };
            let t = match res {
                Ok(Ok(t)) => Ok(t),
                Ok(Err(_e)) => {
                    let t = {
                        let option = option.to_string();
                        match tokio::task::spawn(
                            async move { crate::youtube::search(option, 1).await },
                        )
                        .await
                        {
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
                            match tokio::task::spawn(async move {
                                crate::video::Video::get_video(&url, true, false).await
                            })
                            .await
                            {
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
                            Err(anyhow::anyhow!("Could not get video info"))
                        }
                    } else {
                        Err(anyhow::anyhow!("No videos found for that query"))
                    }
                }
                Err(e) => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content(format!("Error: {:?}", e)),
                        )
                        .await
                    {
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
                            super::VideoType::Disk(v) => v.title,
                            super::VideoType::Url(v) => v.title,
                        };
                        #[cfg(feature = "tts")]
                        if let Ok(key) = key.as_ref() {
                            println!("Getting tts for {}", title);
                            truevideos.push(MetaVideo {
                                video: v,
                                ttsmsg: Some(LazyLoadedVideo::new(tokio::spawn(
                                    crate::youtube::get_tts(title.clone(), key.clone(), None),
                                ))),
                                title,
                                author: Author::from_user(
                                    ctx,
                                    &interaction.user,
                                    interaction.guild_id,
                                )
                                .await,
                            })
                        } else {
                            truevideos.push(MetaVideo {
                                video: v,
                                ttsmsg: None,
                                title,
                                author: Author::from_user(
                                    ctx,
                                    &interaction.user,
                                    interaction.guild_id,
                                )
                                .await,
                            });
                        }
                        #[cfg(not(feature = "tts"))]
                        truevideos.push(MetaVideo { video: v, title });
                    }
                    let data_read = ctx.data.read().await;
                    let audio_command_handler = data_read
                        .get::<AudioCommandHandler>()
                        .expect("Expected AudioCommandHandler in TypeMap")
                        .clone();
                    let mut audio_command_handler = audio_command_handler.lock().await;
                    let tx = audio_command_handler.get_mut(&guild_id.to_string());
                    if let Some(tx) = tx {
                        let (rtx, rrx) = oneshot::channel::<String>();
                        if tx
                            .send((rtx, AudioPromiseCommand::Play(truevideos)))
                            .is_err()
                        {
                            if let Err(e) = interaction
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new()
                                        .content("Failed to send message to audio handler"),
                                )
                                .await
                            {
                                println!("Failed to edit original interaction response: {:?}", e);
                            }
                        }
                        let timeout = tokio::time::timeout(Duration::from_secs(10), rrx).await;
                        if let Ok(Ok(msg)) = timeout {
                            if let Err(e) = interaction
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new().content(msg),
                                )
                                .await
                            {
                                println!("Failed to edit original interaction response: {:?}", e);
                            }
                        } else if let Err(e) = interaction
                            .edit_response(
                                &ctx.http,
                                EditInteractionResponse::new()
                                    .content("Timed out waiting for song to start playing"),
                            )
                            .await
                        {
                            println!("Failed to edit original interaction response: {:?}", e);
                        }
                    } else {
                        audio_command_handler.remove(&guild_id.to_string());
                    }
                }
                Err(e) => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content(format!("Error: {:?}", e)),
                        )
                        .await
                    {
                        println!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
            }
        } else if let Err(e) = interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("TELL ETHAN THIS SHOULD NEVER HAPPEN :("),
            )
            .await
        {
            println!("Failed to edit original interaction response: {:?}", e);
        }
    }
    fn name(&self) -> &str {
        "play"
    }
    #[allow(unused)]
    async fn autocomplete(&self, ctx: &Context, auto: &CommandInteraction) -> Result<(), Error> {
        // gonna have to fix this...
        println!("{:?}", auto);
        // for op in auto.data.options.clone() {
        //     if op.focused && op.name == "url" {
        //         #[cfg(feature = "youtube-search")]
        //         {
        //             let v = match op.value.as_ref().and_then(|v| v.as_str()) {
        //                 Some(v) => v,
        //                 None => {
        //                     continue;
        //                 }
        //             };
        //             if v.starts_with("http://") || v.starts_with("https://") {
        //                 let video = crate::video::Video::get_video(v, false, true).await?;
        //                 if let Some(vid) = video.first() {
        //                     auto.create_autocomplete_response(&ctx.http, |c| {
        //                         c.add_string_choice(vid.get_title(), v)
        //                     })
        //                     .await?;
        //                 } else {
        //                     auto.create_autocomplete_response(&ctx.http,
        // |c| c.add_string_choice("Could not retrieve title, select this option to use url anyways.", v)).await?;
        //                 }
        //             } else {
        //                 let query = crate::youtube::youtube_search(
        //                     v,
        //                     crate::Config::get().autocomplete_limit,
        //                 )
        //                 .await;
        //                 if let Ok(query) = query {
        //                     if query.is_empty() {
        //                         auto.create_autocomplete_response(&ctx.http, |c| {
        //                             c.add_string_choice("Invalid url", "")
        //                         })
        //                         .await?;
        //                     } else {
        //                         auto.create_autocomplete_response(&ctx.http, |c| {
        //                             let mut c = c;
        //                             for (i, q) in query.iter().enumerate() {
        //                                 if i > 25 {
        //                                     break;
        //                                 }
        //                                 c = c.add_string_choice(
        //                                     format!(
        //                                         "{} {}{}",
        //                                         if q.duration.is_some() { "🎵" } else { "📼" },
        //                                         q.title,
        //                                         match q.uploader.as_ref() {
        //                                             Some(u) => format!(" - {}", u),
        //                                             None => "".to_string(),
        //                                         }
        //                                     ),
        //                                     q.url.clone(),
        //                                 );
        //                             }
        //                             c
        //                         })
        //                         .await?;
        //                     }
        //                 } else {
        //                     auto.create_autocomplete_response(&ctx.http, |c| {
        //                         c.add_string_choice("Invalid url", "")
        //                     })
        //                     .await?;
        //                 }
        //             }
        //         }
        //         #[cfg(not(feature = "youtube-search"))]
        //         {
        //             auto.create_autocomplete_response(&ctx.http, |c| {
        //                 c.add_string_choice("Live search functionality not enabled.", "")
        //             })
        //             .await?;
        //         }
        //     }
        // }
        Ok(())
    }
}
