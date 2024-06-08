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
                    .dm(
                        &ctx.http,
                        CreateMessage::default()
                            .content(format!("Anonymous Feedback:\n```{}```", feedback))
                            .button(
                                CreateButton::new(FeedbackCustomId::new(interaction.user.id))
                                    .style(ButtonStyle::Success)
                                    .label("Respond"),
                            ),
                    )
                    .await
                {
                    log::error!("Failed to send feedback to developer: {}", e);
                    content = format!(
                        "Unfortunately, I failed to send your feedback to the developer.\n\
                        If you're able to, be sure to send it to him yourself.\n\
                        He's <@156533151198478336> (@monkey_d._issy)\n\
                        Here's a copy if you need it.\n\
                        ```\n{}\n```",
                        feedback
                    );
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

pub struct FeedbackCustomId {
    pub user_id: UserId,
}

impl FeedbackCustomId {
    pub fn new(user_id: UserId) -> Self {
        Self { user_id }
    }
    // pub fn from_id(id: &str) -> Option<Self> {
    //     // id is in the format "respond:123456789012345678"
    //     let mut split = id.split(':');
    //     match (split.next(), split.next(), split.next()) {
    //         (Some("respond"), Some(user_id), None) => match user_id.parse::<u64>() {
    //             Ok(user_id) => Some(Self::new(UserId::new(user_id))),
    //             Err(e) => {
    //                 log::error!("Failed to parse user id: {}", e);
    //                 None
    //             }
    //         },
    //         _ => None,
    //     }
    // }
}

impl From<FeedbackCustomId> for String {
    fn from(val: FeedbackCustomId) -> Self {
        format!("respond:{}", val.user_id.get())
    }
}

impl TryFrom<&str> for FeedbackCustomId {
    type Error = anyhow::Error;
    fn try_from(id: &str) -> Result<Self> {
        let mut split = id.split(':');
        match (split.next(), split.next(), split.next()) {
            (Some("respond"), Some(user_id), None) => match user_id.parse::<u64>() {
                Ok(user_id) => Ok(Self::new(UserId::new(user_id))),
                Err(e) => {
                    log::error!("Failed to parse user id: {}", e);
                    Err(e.into())
                }
            },
            _ => Err(anyhow::anyhow!("Invalid feedback custom id")),
        }
    }
}
