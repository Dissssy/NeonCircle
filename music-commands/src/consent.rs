use common::anyhow::Result;
use common::serenity::all::*;
use common::{log, CommandTrait};
use long_term_storage::User;
#[derive(Debug, Clone)]
pub struct Command;
#[async_trait]
impl CommandTrait for Command {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .contexts(vec![InteractionContext::Guild])
                .description(
                    "Grant consent for Neon Circle to process audio data \
                    sent from your microphone. (OFF BY DEFAULT)",
                )
                .set_options(vec![CreateCommandOption::new(
                    CommandOptionType::Boolean,
                    "consent",
                    "I consent to Neon Circle processing audio data \
                    sent from my microphone.",
                )
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
            "consent" => Some(&o.value),
            _ => None,
        }) {
            Some(ResolvedValue::Boolean(b)) => *b,
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
        if user_conf.mic_consent == option {
            if let Err(e) = interaction
                .edit_response(
                    &ctx.http,
                    EditInteractionResponse::new()
                        .content("This is already your current consent status."),
                )
                .await
            {
                log::error!("Failed to edit original interaction response: {:?}", e);
            }
            return Ok(());
        }

        user_conf.mic_consent = option;
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
                    if option {
                        "You have granted consent for Neon Circle to process audio data sent from your microphone."
                    } else {
                        "You have revoked consent for Neon Circle to process audio data sent from your microphone."
                    },
                ))
            .await
        {
            log::error!("Failed to edit original interaction response: {:?}", e);
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "consent"
    }
}
