use super::{AudioCommandHandler, AudioPromiseCommand};
use anyhow::Error;
use serenity::all::*;
use std::time::Duration;
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub struct Volume;

#[async_trait]
impl crate::CommandTrait for Volume {
    fn register(&self) -> CreateCommand {
        CreateCommand::new(self.name())
            .description("Change the volume of the bot for this session")
            .set_options(vec![CreateCommandOption::new(
                CommandOptionType::Number,
                "volume",
                "Volume",
            )
            .max_number_value(100.0)
            .min_number_value(0.0)
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
            eprintln!("Failed to create interaction response: {:?}", e);
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
                    eprintln!("Failed to edit original interaction response: {:?}", e);
                }
                return;
            }
        };
        let options = interaction.data.options();

        let option = match options.iter().find_map(|o| match o.name {
            "volume" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Number(f)) => *f,
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
        } as f64
            / 100.0;

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
                        eprintln!("Failed to edit original interaction response: {:?}", e);
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
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                    return;
                }
                super::VoiceAction::Join(_channel) => {
                    if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new().content(
                                "I'm not in a channel, if you want me to join use /join or /play",
                            ),
                        )
                        .await
                    {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
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
                        let (rtx, rrx) = oneshot::channel::<String>();
                        if tx.send((rtx, AudioPromiseCommand::Volume(option))).is_err() {
                            if let Err(e) = interaction
                                .edit_response(
                                    &ctx.http,
                                    EditInteractionResponse::new()
                                        .content("Failed to send volume change"),
                                )
                                .await
                            {
                                eprintln!("Failed to edit original interaction response: {:?}", e);
                            }
                            return;
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
                                eprintln!("Failed to edit original interaction response: {:?}", e);
                            }
                        } else if let Err(e) = interaction
                            .edit_response(
                                &ctx.http,
                                EditInteractionResponse::new().content("Failed to change volume"),
                            )
                            .await
                        {
                            eprintln!("Failed to edit original interaction response: {:?}", e);
                        }
                    } else if let Err(e) = interaction
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new()
                                .content("Couldnt find the channel handler :( im broken."),
                        )
                        .await
                    {
                        eprintln!("Failed to edit original interaction response: {:?}", e);
                    }
                }
            }
        } else if let Err(e) = interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content("TELL ETHAN THIS SHOULD NEVER HAPPEN :("),
            )
            .await
        {
            eprintln!("Failed to edit original interaction response: {:?}", e);
        }
    }
    fn name(&self) -> &str {
        "volume"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &CommandInteraction) -> Result<(), Error> {
        Ok(())
    }
}
