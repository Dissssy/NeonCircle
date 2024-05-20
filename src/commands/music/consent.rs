use anyhow::Result;
use serenity::all::*;
#[derive(Debug, Clone)]
pub struct Consent;
#[async_trait]
impl crate::CommandTrait for Consent {
    fn register(&self) -> CreateCommand {
        CreateCommand::new(self.name()).description("Grant consent for Neon Circle to process audio data sent from your microphone. (OFF BY DEFAULT)").set_options(vec![CreateCommandOption::new(CommandOptionType::Boolean, "consent", "I consent to Neon Circle processing audio data sent from my microphone.").required(true)])
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
                return;
            }
        };
        {
            // log::trace!("LOCKING CONSENT DATABASE");
            // let mut db = crate::CONSENT_DATABASE.write().await;
            // log::trace!("LOCKED CONSENT DATABASE");
            // let _ = db.insert(interaction.user.id, option);
            // log::trace!("UNLOCKING CONSENT DATABASE");
            crate::consent::set_consent(interaction.user.id, option);
        }
        // let ungus = {
        //     let bingus = ctx.data.read().await;
        //     let bungly = bingus.get::<super::VoiceData>();
        //     bungly.map(Arc::clone)
        // };
        // if let (Some(v), Some(member), Some(guild_id)) =
        //     (ungus, interaction.member.as_ref(), interaction.guild_id)
        // {
        // let next_step = {
        //     v.write()
        //         .await
        //         .mutual_channel(ctx, &guild_id, &member.user.id)
        // };
        // if let Ok(msg) = next_step
        //     .send_command_or_err(
        //         ctx,
        //         AudioPromiseCommand::Consent {
        //             user_id: member.user.id,
        //             consent: option,
        //         },
        //     )
        //     .await
        // {
        //     if let Err(e) = interaction
        //         .edit_response(&ctx.http, EditInteractionResponse::new().content(msg))
        //         .await
        //     {
        //         log::error!("Failed to edit original interaction response: {:?}", e);
        //     }
        //     return;
        // }
        // }
        if let Err(e) = interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new().content(&format!("You have {} consented to neon circle processing audio data sent from your microphone.", if option { "" } else { "not" })),
            )
            .await
        {
            log::error!("Failed to edit original interaction response: {:?}", e);
        }
    }
    fn name(&self) -> &str {
        "consent"
    }
    async fn autocomplete(&self, _ctx: &Context, _auto: &CommandInteraction) -> Result<()> {
        Ok(())
    }
}
