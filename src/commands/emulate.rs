use anyhow::Error;
use serenity::builder::CreateApplicationCommand;
use serenity::model::application::interaction::InteractionResponseType;
use serenity::model::prelude::command::CommandOptionType;
// use serenity::model::prelude::Embed;

use serenity::model::prelude::interaction::autocomplete::AutocompleteInteraction;
use serenity::prelude::Context;

pub struct EmulateCommand;

#[serenity::async_trait]
impl crate::CommandTrait for EmulateCommand {
    fn register(&self, command: &mut CreateApplicationCommand) {
        command
            .name(self.name())
            .description("Emulate a user")
            .create_option(|option| {
                option
                    .name("user")
                    .description("The user to emulate")
                    .kind(CommandOptionType::User)
                    .required(true)
            });
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &serenity::model::prelude::application_command::ApplicationCommandInteraction,
    ) {
        // let interaction = interaction.application_command().expect("Not a command");
        interaction
            .create_interaction_response(&ctx.http, |response| {
                response.kind(InteractionResponseType::DeferredChannelMessageWithSource)
            })
            .await
            .unwrap();
        // //disabled
        // interaction
        //             .edit_original_interaction_response(&ctx.http, |e| {
        //             e.content("The FCC has temporarily disabled this feature until ETHAN can write a privacy policy (four years of law school)")
        //             }).await.unwrap();
        // //disabled
        // return;
        let options = interaction.data.options.clone();
        let user = match options[0].resolved.as_ref().unwrap() {
            serenity::model::prelude::interaction::application_command::CommandDataOptionValue::User(u, _) => u,
            _ => unreachable!(),
        };
        // {
        //     let mut whitelist = crate::WHITELIST.lock().await;
        //     // if the user is not whitelisted, check if they are the one invoking the command, if not, return
        //     let userstr = format!("{}", user.id.0);
        //     if !whitelist.contains(&userstr) {
        //         if user.id.0 == interaction.user.id.0 {
        //             whitelist.push(userstr);
        //             // save the whitelist
        //             tokio::fs::write(
        //                 crate::Config::get().whitelist_path,
        //                 serde_json::to_string(&*whitelist).unwrap(),
        //             )
        //             .await
        //             .unwrap();
        //         } else {
        //             interaction
        //                 .edit_original_interaction_response(&ctx.http, |e| {
        //                     e.content("The user you are trying to emulate is not whitelisted, complain to them to whitelist themselves")
        //                 })
        //                 .await
        //                 .unwrap();
        //             return;
        //         }
        //     }
        // }

        // let mut b = if user.id.0 == 1035364346471133194 {
        //     "".to_owned()
        // } else {
        //     let g = crate::SHITGPT.lock().await;
        //     g.get(&format!("{}", user.id.0))
        //         .map(|g| g.generate_without_weights())
        //         .unwrap_or("I have yet to say anything in the training data because i am a MASSIVE homosexual!".to_string())
        // };
        let mut b = "Big Wet Sloppy Bowser".to_owned();
        if b.trim().is_empty() {
            let size = std::fs::metadata(crate::Config::get().shitgpt_path)
                .unwrap()
                .len();
            // make the size readable (i.e. 1.2 GB)
            let size = format_bytes(size);
            let potential_messages = [
                "I'm schizophrenic have some numbers",
                "This is how many men i've killed",
                "This is how many men i've sucked off",
                "I eated this many kids",
                "what",
                "you're so white",
                "you're gay?????",
                "WOCKY SLUSH",
            ];
            // select a random potential message
            let thismessage = rand::random::<usize>() % potential_messages.len();
            b = format!("{}: {size}", potential_messages[thismessage]);
        }

        match interaction
            .channel_id
            .create_webhook(&ctx.http, "Emulator")
            .await
        {
            Ok(w) => {
                w.execute(&ctx.http, false, |e| {
                    e.username(user.name.as_str());
                    e.avatar_url(user.avatar_url().unwrap().as_str());
                    e.content(b)
                })
                .await
                .unwrap();
                w.delete(&ctx.http).await.unwrap();
                interaction
                    .delete_original_interaction_response(&ctx.http)
                    .await
                    .unwrap();
            }
            Err(_) => {
                interaction
                    .edit_original_interaction_response(&ctx.http, |e| {
                        // make an embed with the users name and avatar and the text in b
                        e.embed(|e| {
                            e.author(|a| {
                                a.name(user.name.as_str());
                                a.icon_url(user.avatar_url().unwrap().as_str());
                                a
                            });
                            e.description(b);
                            e
                        })
                    })
                    .await
                    .unwrap();
            }
        }
    }
    fn name(&self) -> &str {
        "emulate"
    }
    #[allow(unused_variables)]
    async fn autocomplete(
        &self,
        ctx: &Context,
        interaction: &AutocompleteInteraction,
    ) -> Result<(), Error> {
        Ok(())
    }
}

fn format_bytes(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let exp = (bytes as f64).log(1024f64).floor() as usize;
    let pre = format!("{:.2}", bytes as f64 / 1024f64.powi(exp as i32));
    format!("{} {}", pre, units[exp])
}
