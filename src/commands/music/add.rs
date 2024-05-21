use super::{
    mainloop::{ControlData, Log},
    settingsdata::SettingsData,
    transcribe::TranscriptionThread,
    AudioCommandHandler, AudioHandler, AudioPromiseCommand,
};
use crate::{
    commands::music::{Author, LazyLoadedVideo, MetaVideo},
    global_data::VoiceAction,
};
use anyhow::Result;
use serenity::all::*;
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
#[derive(Debug, Clone)]
pub struct Add;
#[async_trait]
impl crate::CommandTrait for Add {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .description("Add a song to the queue")
                .set_options(vec![CreateCommandOption::new(
                    CommandOptionType::String,
                    "search",
                    "Search youtube or provide a url (non youtube works as well)",
                )
                .set_autocomplete(true)
                .required(true)]),
        )
    }
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        if let Err(e) = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Defer(
                    CreateInteractionResponseMessage::new().ephemeral(true),
                ),
            )
            .await
        {
            log::error!("Failed to create interaction response: {:?}", e);
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
                    log::error!("Failed to edit original interaction response: {:?}", e);
                }
                return Ok(());
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
                    log::error!("Failed to edit original interaction response: {:?}", e);
                }
                return Ok(());
            }
        };
        if let Some(member) = interaction.member.as_ref() {
            let next_step = match crate::global_data::mutual_channel(&guild_id, &member.user.id)
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    log::error!("Failed to get mutual channel: {:?}", e);
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content("Failed to get mutual channel"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return Ok(());
                }
            };
            let mut userin = None;
            match next_step.action {
                VoiceAction::UserNotConnected => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content("You're not in a voice channel"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return Ok(());
                }
                VoiceAction::NoRemaining => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content("No satellites available to join, use /feedback to request more (and dont forget to donate if you can! :D)"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return Ok(());
                }
                VoiceAction::InviteSatellite(invite) => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content(format!(
                                "There are no satellites available, [use this link to invite one]({})\nPlease ensure that all satellites have permission to view the voice channel you're in.",
                                invite
                            )),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return Ok(());
                }
                VoiceAction::SatelliteInVcWithUser(channel, _ctx) => {
                    userin = Some(channel);
                }
                VoiceAction::SatelliteShouldJoin(channel, satellite_ctx) => {
                    let manager = match songbird::get(&satellite_ctx).await {
                        Some(v) => v,
                        None => {
                            log::error!("Failed to get songbird manager");
                            if let Err(e) = interaction
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new()
                                        .content("Failed to get songbird manager"),
                                )
                                .await
                            {
                                log::error!(
                                    "Failed to edit original interaction response: {:?}",
                                    e
                                );
                            }
                            return Ok(());
                        }
                    };
                    {
                        let audio_handler = {
                            match ctx.data.read().await.get::<AudioHandler>() {
                                Some(v) => Arc::clone(v),
                                None => {
                                    log::error!("Failed to get audio handler");
                                    if let Err(e) = interaction
                                        .edit_response(
                                            &ctx.http,
                                            EditInteractionResponse::new()
                                                .content("Failed to get audio handler"),
                                        )
                                        .await
                                    {
                                        log::error!(
                                            "Failed to edit original interaction response: {:?}",
                                            e
                                        );
                                    }
                                    return Ok(());
                                }
                            }
                        };
                        match manager.join(guild_id, channel).await {
                            Ok(call) => {
                                let (tx, rx) = mpsc::unbounded_channel::<(
                                    oneshot::Sender<String>,
                                    AudioPromiseCommand,
                                )>();
                                let transcription = TranscriptionThread::new(
                                    Arc::clone(&call),
                                    Arc::clone(&ctx.http),
                                    tx.clone(),
                                )
                                .await;
                                let msg = match channel
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
                                        log::error!("Failed to send message: {:?}", e);
                                        if let Err(e) = interaction
                                            .edit_response(
                                                &ctx.http,
                                                EditInteractionResponse::new()
                                                    .content("Failed to send message"),
                                            )
                                            .await
                                        {
                                            log::error!("Failed to edit original interaction response: {:?}", e);
                                        }
                                        return Ok(());
                                    }
                                };
                                let messageref = super::MessageReference::new(
                                    ctx.http.clone(),
                                    ctx.cache.clone(),
                                    guild_id,
                                    channel,
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
                                    None => return Ok(()),
                                };
                                let em = match super::get_transcribe_channel_handler(ctx, &guild_id)
                                    .await
                                {
                                    Ok(v) => v,
                                    Err(e) => {
                                        log::error!(
                                            "Failed to get transcribe channel handler: {:?}",
                                            e
                                        );
                                        if let Err(e) = interaction
                                            .edit_response(
                                                &ctx.http,
                                                EditInteractionResponse::new()
                                                    .content("Failed to get handler"),
                                            )
                                            .await
                                        {
                                            log::error!(
                                    "Failed to edit original interaction response: {:?}",
                                    e
                                );
                                        }
                                        return Ok(());
                                    }
                                };
                                if let Err(e) = em.write().await.register(channel).await {
                                    log::error!("Error registering channel: {:?}", e);
                                }
                                let handle = tokio::task::spawn(async move {
                                    let control = ControlData {
                                        call,
                                        rx,
                                        msg: messageref,
                                        nothing_uri: nothing_path,
                                        settings: SettingsData::default(),
                                        brk: false,
                                        log: Log::new(format!("{}-{}", guild_id, channel)),
                                        transcribe: em,
                                    };
                                    super::mainloop::the_lüüp(
                                        cfg.looptime,
                                        transcription,
                                        control,
                                    )
                                    .await;
                                });
                                audio_handler
                                    .write()
                                    .await
                                    .insert(guild_id.to_string(), handle);
                                let audio_command_handler = {
                                    let read_lock = ctx.data.read().await;
                                    match read_lock.get::<super::AudioCommandHandler>() {
                                        Some(v) => Arc::clone(v),
                                        None => {
                                            log::error!("Failed to get audio command handler");
                                            if let Err(e) = interaction
                                                .edit_response(
                                                    &ctx.http,
                                                    EditInteractionResponse::new().content(
                                                        "Failed to get audio command handler",
                                                    ),
                                                )
                                                .await
                                            {
                                                log::error!(
                                                    "Failed to edit original interaction response: {:?}",
                                                    e
                                                );
                                            }
                                            return Ok(());
                                        }
                                    }
                                };
                                userin = Some(channel);
                                audio_command_handler.write().await.insert(channel, tx);
                            }
                            Err(e) => {
                                log::error!("Failed to join channel: {:?}", e);
                                if let Err(e) = interaction
                                    .edit_response(
                                        &ctx.http,
                                        EditInteractionResponse::new()
                                            .content("Failed to join voice channel"),
                                    )
                                    .await
                                {
                                    log::error!(
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
                                log::error!("Error: {:?}", e);
                                return Ok(());
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
                                    log::error!("Error: {:?}", e);
                                    return Ok(());
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
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return Ok(());
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
                            log::trace!("Getting tts for {}", title);
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
                    let audio_command_handler = match data_read.get::<AudioCommandHandler>() {
                        Some(v) => Arc::clone(v),
                        None => {
                            log::error!("Failed to get audio command handler");
                            if let Err(e) = interaction
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new()
                                        .content("Failed to get audio command handler"),
                                )
                                .await
                            {
                                log::error!(
                                    "Failed to edit original interaction response: {:?}",
                                    e
                                );
                            }
                            return Ok(());
                        }
                    };
                    let userin = match userin {
                        Some(v) => v,
                        None => {
                            if let Err(e) = interaction
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new()
                                        .content("Failed to get channel???"),
                                )
                                .await
                            {
                                log::error!(
                                    "Failed to edit original interaction response: {:?}",
                                    e
                                );
                            }
                            return Ok(());
                        }
                    };
                    let mut audio_command_handler = audio_command_handler.write().await;
                    let tx = audio_command_handler.get_mut(&userin);
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
                                log::error!(
                                    "Failed to edit original interaction response: {:?}",
                                    e
                                );
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
                                log::error!(
                                    "Failed to edit original interaction response: {:?}",
                                    e
                                );
                            }
                        } else if let Err(e) = interaction
                            .edit_response(
                                &ctx.http,
                                EditInteractionResponse::new()
                                    .content("Timed out waiting for song to start playing"),
                            )
                            .await
                        {
                            log::error!("Failed to edit original interaction response: {:?}", e);
                        }
                    } else {
                        audio_command_handler.remove(&userin);
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
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                    return Ok(());
                }
            }
        } else if let Err(e) = interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("TELL ETHAN THIS SHOULD NEVER HAPPEN :("),
            )
            .await
        {
            log::error!("Failed to edit original interaction response: {:?}", e);
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "add"
    }
    #[allow(unused)]
    async fn autocomplete(&self, ctx: &Context, auto: &CommandInteraction) -> Result<()> {
        let options = auto.data.options();
        let initial_query = match options.iter().find_map(|o| match o.name {
            "search" => Some(o.value.clone()),
            _ => None,
        }) {
            Some(ResolvedValue::Autocomplete { value, .. }) => value,
            _ => {
                return Ok(());
            }
        };
        #[cfg(feature = "youtube-search")]
        {
            let mut completions = CreateAutocompleteResponse::default();
            if initial_query.starts_with("http://") || initial_query.starts_with("https://") {
                let video = crate::video::Video::get_video(initial_query, false, true).await?;
                if let Some(vid) = video.first() {
                    completions = completions.add_string_choice(vid.get_title(), initial_query);
                } else {
                    completions = completions.add_string_choice(
                        "Could not retrieve title. Is the URL valid?",
                        initial_query,
                    );
                }
            } else {
                let query = crate::youtube::youtube_search(
                    initial_query,
                    crate::Config::get().autocomplete_limit,
                )
                .await;
                if let Ok(query) = query {
                    if query.is_empty() {
                        completions =
                            completions.add_string_choice("No results found", initial_query);
                    } else {
                        for (i, q) in query.iter().enumerate() {
                            if i > 25 {
                                break;
                            }
                            let mut title = format!(
                                "{} {}{}",
                                if q.duration.is_some() { "🎵" } else { "📼" },
                                match q.uploader.as_ref() {
                                    Some(u) => format!("{} - ", u),
                                    None => "".to_string(),
                                },
                                q.title,
                            );
                            if title.len() > 100 {
                                title = title.get(..97).unwrap_or_default().to_string() + "...";
                            }
                            completions = completions.add_string_choice(title, q.url.clone());
                        }
                    }
                } else {
                    completions =
                        completions.add_string_choice("Error fetching results", initial_query);
                }
            }
            if let Err(e) = auto
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Autocomplete(completions),
                )
                .await
            {
                log::error!("Failed to create interaction response: {:?}", e);
            }
        }
        Ok(())
    }
}
