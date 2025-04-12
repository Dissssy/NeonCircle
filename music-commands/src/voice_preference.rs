use common::anyhow::Result;
use common::serenity::all::*;
use common::{log, CommandTrait};
use long_term_storage::{User, VoicePreference};
#[derive(Debug, Clone)]
pub struct Command;
#[async_trait]
impl CommandTrait for Command {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .description(
                    "Allows you to set your TTS voice preference."
                )
                .set_options(vec![CreateCommandOption::new(
                    CommandOptionType::String,
                    "preference",
                    "The voice preference you want to set.",
                )
                .add_string_choice("No Preference", "none")
                .add_string_choice("Male Voice", "male")
                .add_string_choice("Female Voice", "female")
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
        let options = interaction.data.options();
        let option = match options.iter().find_map(|o| match o.name {
            "preference" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::String(b)) => *b,
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
        let mut user_conf = match User::load(interaction.user.id).await {
            Ok(c) => c,
            Err(e) => {
                log::error!("Failed to load user: {:?}", e);
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content("Failed to load user"),
                    )
                    .await
                {
                    log::error!("Failed to edit original interaction response: {:?}", e);
                }
                return Ok(());
            }
        };
        let option = match option {
            "none" => VoicePreference::NoPreference,
            "female" => VoicePreference::Female,
            "male" => VoicePreference::Male,
            _ => {
                if let Err(e) = interaction
                    .edit_response(
                        &ctx.http,
                        EditInteractionResponse::new().content("Invalid option"),
                    )
                    .await
                {
                    log::error!("Failed to edit original interaction response: {:?}", e);
                }
                return Ok(());
            }
        };
        
        if user_conf.voice_preference == option {
            if let Err(e) = interaction
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new()
                        .content("This is already your current voice_preference status."),
                )
                .await
            {
                log::error!("Failed to edit original interaction response: {:?}", e);
            }
            return Ok(());
        }

        user_conf.voice_preference = option;
        if let Err(e) = user_conf.save().await {
            log::error!("Failed to save user: {:?}", e);
            if let Err(e) = interaction
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new().content("Failed to save user"),
                )
                .await
            {
                log::error!("Failed to edit original interaction response: {:?}", e);
            }
            return Ok(());
        }
        if let Err(e) = interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content(
                    match option {
                        VoicePreference::NoPreference => "Your voice preference has been unset.",
                        VoicePreference::Male => "Your voice preference has been set to male.",
                        VoicePreference::Female => "Your voice preference has been set to female.",
                    },
                ))
            .await
        {
            log::error!("Failed to edit original interaction response: {:?}", e);
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "voice_preference"
    }
}
