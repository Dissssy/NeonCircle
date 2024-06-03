#[cfg(feature = "transcribe")]
use super::AudioPromiseCommand;
// use crate::global_data::voice_data::VoiceAction;
#[cfg(feature = "transcribe")]
use crate::voice_events::PostSomething;
use anyhow::Result;
use serenity::all::*;
#[cfg(feature = "transcribe")]
use songbird::Call;
use std::sync::Arc;
#[cfg(feature = "transcribe")]
use tokio::sync::{mpsc, oneshot, Mutex};
// #[derive(Debug, Clone)]
// pub struct Command;
// #[async_trait]
// impl crate::traits::CommandTrait for Command {
//     fn register_command(&self) -> Option<CreateCommand> {
//         Some(
//             CreateCommand::new(self.command_name())
//                 .description("Set transcribe status for a channel")
//                 // .set_options(vec![CreateCommandOption::new(
//                 //     CommandOptionType::Boolean,
//                 //     "value",
//                 //     "Specific value, otherwise toggle",
//                 // )]),
//                 .set_options(vec![
//                     CreateCommandOption::new(
//                         CommandOptionType::
//                     )
//                 ])
//         )
//     }
//     async fn run(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
//         if let Err(e) = interaction
//             .create_response(
//                 &ctx.http,
//                 CreateInteractionResponse::Defer(
//                     CreateInteractionResponseMessage::new().ephemeral(true),
//                 ),
//             )
//             .await
//         {
//             log::error!("Failed to create interaction response: {:?}", e);
//         }
//         let guild_id = match interaction.guild_id {
//             Some(id) => id,
//             None => {
//                 if let Err(e) = interaction
//                     .edit_response(
//                         &ctx.http,
//                         EditInteractionResponse::new()
//                             .content("This command can only be used in a server"),
//                     )
//                     .await
//                 {
//                     log::error!("Failed to edit original interaction response: {:?}", e);
//                 }
//                 return Ok(());
//             }
//         };
//         let options = interaction.data.options();
//         let option = match options.iter().find_map(|o| match o.name {
//             "value" => Some(&o.value),
//             _ => None,
//         }) {
//             Some(ResolvedValue::Boolean(o)) => super::OrToggle::Specific(*o),
//             None => super::OrToggle::Toggle,
//             _ => {
//                 if let Err(e) = interaction
//                     .edit_response(
//                         &ctx.http,
//                         EditInteractionResponse::new().content("This command requires an option"),
//                     )
//                     .await
//                 {
//                     log::error!("Failed to edit original interaction response: {:?}", e);
//                 }
//                 return Ok(());
//             }
//         };
//         if let Some(member) = interaction.member.as_ref() {
//             let next_step =
//                 match crate::global_data::voice_data::mutual_channel(&guild_id, &member.user.id)
//                     .await
//                 {
//                     Ok(v) => v,
//                     Err(e) => {
//                         log::error!("Failed to get mutual channel: {:?}", e);
//                         if let Err(e) = interaction
//                             .edit_response(
//                                 &ctx.http,
//                                 EditInteractionResponse::new()
//                                     .content("Failed to get mutual channel"),
//                             )
//                             .await
//                         {
//                             log::error!("Failed to edit original interaction response: {:?}", e);
//                         }
//                         return Ok(());
//                     }
//                 };
//             match next_step.action {
//                 VoiceAction::NoRemaining => {
//                     if let Err(e) = interaction
//                         .edit_response(
//                             &ctx.http,
//                             EditInteractionResponse::new().content("No satellites available to join, use /feedback to request more (and dont forget to donate if you can! :D)"),
//                         )
//                         .await
//                     {
//                         log::error!("Failed to edit original interaction response: {:?}", e);
//                     }
//                     return Ok(());
//                 }
//                 VoiceAction::InviteSatellite(invite) => {
//                     if let Err(e) = interaction
//                         .edit_response(
//                             &ctx.http,
//                             EditInteractionResponse::new().content(format!(
//                                 "There are no satellites available, [use this link to invite one]({})\nPlease ensure that all satellites have permission to view the voice channel you're in.",
//                                 invite
//                             )),
//                         )
//                         .await
//                     {
//                         log::error!("Failed to edit original interaction response: {:?}", e);
//                     }
//                     return Ok(());
//                 }
//                 VoiceAction::UserNotConnected => {
//                     if let Err(e) = interaction
//                         .edit_response(
//                             &ctx.http,
//                             EditInteractionResponse::new().content("You're not in a voice channel"),
//                         )
//                         .await
//                     {
//                         log::error!("Failed to edit original interaction response: {:?}", e);
//                     }
//                     return Ok(());
//                 }
//                 VoiceAction::SatelliteShouldJoin(_channel, _ctx) => {
//                     if let Err(e) = interaction
//                         .edit_response(
//                             &ctx.http,
//                             EditInteractionResponse::new().content(
//                                 "I'm not in a channel, if you want me to join use /join or /add",
//                             ),
//                         )
//                         .await
//                     {
//                         log::error!("Failed to edit original interaction response: {:?}", e);
//                     }
//                     return Ok(());
//                 }
//                 VoiceAction::SatelliteInVcWithUser(channel, _ctx) => {
//                     crate::global_data::transcribe::change_transcription(
//                         channel,
//                         interaction.channel_id,
//                         option,
//                     )
//                     .await;
//                     if let Err(e) = interaction
//                         .edit_response(
//                             &ctx.http,
//                             EditInteractionResponse::new().content(match option {
//                                 super::OrToggle::Specific(option) => {
//                                     if option {
//                                         "Registered"
//                                     } else {
//                                         "Unregistered"
//                                     }
//                                 }
//                                 super::OrToggle::Toggle => "Toggled",
//                             }),
//                         )
//                         .await
//                     {
//                         log::error!("Failed to edit original interaction response: {:?}", e);
//                     }
//                 }
//             }
//         } else if let Err(e) = interaction
//             .edit_response(
//                 &ctx.http,
//                 EditInteractionResponse::new().content("TELL ETHAN THIS SHOULD NEVER HAPPEN :("),
//             )
//             .await
//         {
//             log::error!("Failed to edit original interaction response: {:?}", e);
//         }
//         Ok(())
//     }
//     fn command_name(&self) -> &str {
//         "transcribe"
//     }
// }
#[cfg(feature = "transcribe")]
pub struct TranscriptionThread {
    pub thread: tokio::task::JoinHandle<()>,
    pub message: mpsc::UnboundedSender<TranscriptionMessage>,
    pub receiver: mpsc::UnboundedReceiver<(PostSomething, UserId)>,
}
#[cfg(feature = "transcribe")]
impl TranscriptionThread {
    pub async fn new(
        call: Arc<Mutex<Call>>,
        http: Arc<http::Http>,
        otx: mpsc::UnboundedSender<(oneshot::Sender<Arc<str>>, AudioPromiseCommand)>,
    ) -> Self {
        let (message, messagerx) = mpsc::unbounded_channel();
        let (tx, receiver) = mpsc::unbounded_channel::<(PostSomething, UserId)>();
        let thread = tokio::task::spawn(crate::voice_events::transcription_thread(
            call, http, otx, messagerx, tx,
        ));
        Self {
            thread,
            message,
            receiver,
        }
    }
    pub async fn stop(self) -> Result<()> {
        self.message.send(TranscriptionMessage::Stop)?;
        match tokio::time::timeout(tokio::time::Duration::from_secs(5), self.thread).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err(anyhow::anyhow!("Timeout")),
        }
    }
}
#[derive(Debug)]
pub enum TranscriptionMessage {
    Stop,
}
