use anyhow::Result;
use common::log;
use common::serenity::all::*;
#[derive(Debug, Clone)]
pub struct Feedback;
#[async_trait]
impl crate::traits::CommandTrait for Feedback {
    fn register_command(&self) -> Option<CreateCommand> {
        // no options because this is going to open a modal for the user to type the feedback into, with a dropdown for the type of feedback and a dropdown at the end for whether or not to make the feedback anonymous
        Some(CreateCommand::new(self.command_name()).description("Send feedback to the developers"))
    }
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        if let Err(e) = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Modal(
                    CreateModal::new("feedback", "Feedback").components(vec![
                        CreateActionRow::InputText(
                            CreateInputText::new(InputTextStyle::Paragraph, "feedback", "Feedback")
                                .placeholder("Type your feedback here")
                                .required(true),
                        ),
                    ]),
                ),
            )
            .await
        {
            log::error!("Failed to create interaction response: {:?}", e);
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "feedback"
    }
    fn modal_names(&self) -> &'static [&'static str] {
        &["feedback"]
    }
    async fn run_modal(&self, ctx: &Context, interaction: &ModalInteraction) -> Result<()> {
        let i = match interaction
            .data
            .components
            .first()
            .and_then(|ar| ar.components.first())
        {
            Some(ActionRowComponent::InputText(feedback)) => feedback,
            Some(_) => {
                log::error!("Invalid components in feedback modal");
                return Ok(());
            }
            None => {
                log::error!("No components in feedback modal");
                return Ok(());
            }
        };
        let mut content = "Thanks for the feedback!".to_owned();
        let feedback = match i.value {
            Some(ref value) => value,
            None => {
                log::error!("No value in feedback modal");
                return Ok(());
            }
        };
        match ctx.http.get_user(UserId::new(156533151198478336)).await {
            Ok(user) => {
                if let Err(e) = user
                    .dm(&ctx.http, CreateMessage::default().content(feedback))
                    .await
                {
                    log::error!("Failed to send feedback to developer: {}", e);
                    content = format!("{}{}\n{}\n{}\n{}", content, "Unfortunately, I failed to send your feedback to the developer.", "If you're able to, be sure to send it to him yourself!", "He's <@156533151198478336> (monkey_d._issy)\n\nHere's a copy if you need it.", feedback);
                }
            }
            Err(e) => {
                log::error!("Failed to get user: {}", e);
                content = format!(
                    "{}{}\n{}\n{}\n{}",
                    content,
                    "Unfortunately, I failed to send your feedback to the developer.",
                    "If you're able to, be sure to send it to him yourself!",
                    "He's <@156533151198478336> (monkey_d._issy)\n\nHere's a copy if you need it.",
                    feedback
                );
            }
        }
        if let Err(e) = interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(content)
                        .ephemeral(true),
                ),
            )
            .await
        {
            log::error!("Failed to send response: {}", e);
        }
        Ok(())
    }
}
