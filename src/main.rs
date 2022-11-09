// clippy deny unwraps and expects
// #![deny(clippy::unwrap_used)]

mod commands;

mod video;
mod youtube;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Error;
use commands::music::VoiceData;
// use hyper;
// use hyper_rustls;
use serenity::async_trait;
use serenity::builder::CreateApplicationCommand;

use serenity::model::application::interaction::autocomplete::AutocompleteInteraction;
use serenity::model::application::interaction::Interaction;
use serenity::model::gateway::Ready;
// use serenity::model::id::GuildId;
use serenity::model::prelude::command::Command;
use serenity::model::voice::VoiceState;
use serenity::prelude::*;
use songbird::SerenityInit;

struct Handler {
    commands: Vec<Box<dyn CommandTrait>>,
}

impl Handler {
    fn new(commands: Vec<Box<dyn CommandTrait>>) -> Self {
        Self { commands }
    }
}

#[async_trait]
pub trait CommandTrait
where
    Self: Send + Sync,
{
    fn register(&self, command: &mut CreateApplicationCommand);
    async fn run(&self, ctx: &Context, interaction: Interaction);
    fn name(&self) -> &str;
    async fn autocomplete(
        &self,
        ctx: &Context,
        interaction: &AutocompleteInteraction,
    ) -> Result<(), Error>;
}

#[async_trait]
impl EventHandler for Handler {
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match &interaction {
            Interaction::ApplicationCommand(command) => {
                let command_name = command.data.name.clone();
                let command = self.commands.iter().find(|c| c.name() == command_name);
                if let Some(command) = command {
                    command.run(&ctx, interaction).await;
                } else {
                    println!("Command not found: {}", command_name);
                }
            }

            Interaction::Autocomplete(autocomplete) => {
                let commandn = autocomplete.data.name.clone();
                let command = self.commands.iter().find(|c| c.name() == commandn);
                if let Some(command) = command {
                    let r = command.autocomplete(&ctx, autocomplete).await;
                    if r.is_err() {}
                } else {
                    println!("Command not found: {}", commandn);
                }
            }
            _ => {}
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);

        // let guild_id = GuildId(
        //     Config::get()
        //         .guild_id
        //         .parse::<u64>()
        //         .expect("Invalid guild id"),
        // );

        // GuildId::set_application_commands(&guild_id, &ctx.http, |commands| {
        //     for command in self.commands.iter() {
        //         println!("Registering command: {}", command.name());
        //         commands.create_application_command(|thiscommand| {
        //             command.register(thiscommand);
        //             thiscommand
        //         });
        //     }
        //     commands
        // })
        // .await
        // .expect("Failed to register commands");
        // register all commands globally

        for command in self.commands.iter() {
            println!("Registering command: {}", command.name());
            Command::create_global_application_command(&ctx.http, |com| {
                command.register(com);
                com
            })
            .await
            .expect("Failed to register command");
        }
    }

    async fn voice_state_update(&self, ctx: Context, _old: Option<VoiceState>, new: VoiceState) {
        if let Some(guild_id) = new.guild_id {
            let data_lock = ctx.data.read().await;
            let data = data_lock
                .get::<VoiceData>()
                .expect("Expected VoiceData in TypeMap.")
                .clone();
            let mut data = data.lock().await;

            let guild = data.get_mut(&guild_id);
            if let Some(guild) = guild {
                let state = guild.iter_mut().find(|user| user.user_id == new.user_id);
                if let Some(state) = state {
                    *state = new;
                } else {
                    guild.push(new);
                }
            } else {
                data.insert(guild_id, vec![new]);
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let cfg = Config::get();

    let mut tmp = cfg.data_path.clone();
    tmp.push("tmp");

    let r = std::fs::remove_dir_all(&tmp);
    if r.is_err() {}
    std::fs::create_dir_all(&tmp).expect("Failed to create tmp folder");

    let token = cfg.token;

    let handler = Handler::new(vec![
        Box::new(commands::music::loopit::Loop),
        Box::new(commands::music::pause::Pause),
        Box::new(commands::music::play::Play),
        Box::new(commands::music::remove::Remove),
        Box::new(commands::music::resume::Resume),
        Box::new(commands::music::shuffle::Shuffle),
        Box::new(commands::music::skip::Skip),
        Box::new(commands::music::stop::Stop),
        Box::new(commands::music::volume::Volume),
        Box::new(commands::embed::Video),
        Box::new(commands::embed::Audio),
    ]);
    let mut client = Client::builder(token, GatewayIntents::all())
        .register_songbird()
        .event_handler(handler)
        .await
        .expect("Error creating client");
    {
        let mut data = client.data.write().await;
        data.insert::<commands::music::AudioHandler>(Arc::new(serenity::prelude::Mutex::new(
            HashMap::new(),
        )));
        data.insert::<commands::music::AudioCommandHandler>(Arc::new(
            serenity::prelude::Mutex::new(HashMap::new()),
        ));
        data.insert::<commands::music::VoiceData>(Arc::new(serenity::prelude::Mutex::new(
            HashMap::new(),
        )));
    }

    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Config {
    token: String,
    guild_id: String,
    app_name: String,
    looptime: u64,
    data_path: PathBuf,
    gcloud_script: String,
    youtube_api_key: String,
}

impl Config {
    pub fn get() -> Self {
        let path = dirs::data_dir();
        let mut path = if let Some(path) = path {
            path
        } else {
            PathBuf::from(".")
        };
        path.push("RmbConfig.json");
        Self::get_from_path(path)
    }
    fn onboarding(config_path: &PathBuf, recovered_config: Option<RecoverConfig>) {
        let config = if let Some(rec) = recovered_config {
            println!("Welcome back to my shitty Rust Music Bot!");
            println!("It appears that you have run the bot before, but the config got biffed up.");
            println!("I will take you through a short onboarding process to get you back up and running.");
            let app_name = if let Some(app_name) = rec.app_name {
                app_name
            } else {
                Self::safe_read("\nPlease enter your application name:")
            };
            let mut data_path = config_path.parent().unwrap().to_path_buf();
            data_path.push(app_name.clone());
            Config {
                token: if let Some(token) = rec.token {
                    token
                } else {
                    Self::safe_read("\nPlease enter your bot token:")
                },
                guild_id: if let Some(guild_id) = rec.guild_id {
                    guild_id
                } else {
                    Self::safe_read("\nPlease enter your guild id:")
                },
                app_name,
                looptime: if let Some(looptime) = rec.looptime {
                    looptime
                } else {
                    Self::safe_read("\nPlease enter your loop time in ms\nlower time means faster response but higher utilization:")
                },
                gcloud_script: if let Some(gcloud_script) = rec.gcloud_script {
                    gcloud_script
                } else {
                    Self::safe_read("\nPlease enter your gcloud script location (teehee):")
                },
                youtube_api_key: if let Some(youtube_api_key) = rec.youtube_api_key {
                    youtube_api_key
                } else {
                    Self::safe_read("\nPlease enter your youtube api key:")
                },
                data_path,
            }
        } else {
            println!("Welcome to my shitty Rust Music Bot!");
            println!("It appears that this may be the first time you are running the bot.");
            println!("I will take you through a short onboarding process to get you started.");
            let app_name: String = Self::safe_read("\nPlease enter your application name:");
            let mut data_path = config_path.parent().unwrap().to_path_buf();
            data_path.push(app_name.clone());
            Config {
                token: Self::safe_read("\nPlease enter your bot token:"),
                guild_id: Self::safe_read("\nPlease enter your guild id:"),
                app_name,
                looptime: Self::safe_read("\nPlease enter your loop time in ms\nlower time means faster response but higher utilization:"),
                gcloud_script: Self::safe_read("\nPlease enter your gcloud script location (teehee):"),
                data_path,
                youtube_api_key: Self::safe_read("\nPlease enter your youtube api key:"),
            }
        };
        std::fs::write(
            config_path.clone(),
            serde_json::to_string_pretty(&config)
                .unwrap_or_else(|_| panic!("Failed to write\n{:?}", config_path)),
        )
        .expect("Failed to write config.json");
        println!("Config written to {:?}", config_path);
    }
    fn safe_read<T: std::str::FromStr>(prompt: &str) -> T {
        loop {
            println!("{}", prompt);
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .expect("Failed to read line");
            let input = input.trim();
            match input.parse::<T>() {
                Ok(input) => return input,
                Err(_) => println!("Invalid input"),
            }
        }
    }
    fn get_from_path(path: std::path::PathBuf) -> Self {
        if !path.exists() {
            Self::onboarding(&path, None);
        }
        let config = std::fs::read_to_string(&path);
        if let Ok(config) = config {
            let x: Result<Config, serde_json::Error> = serde_json::from_str(&config);
            if let Ok(x) = x {
                x
            } else {
                println!("Failed to parse config.json, Attempting recovery");
                let recovered = serde_json::from_str(&config);
                if let Ok(recovered) = recovered {
                    Self::onboarding(&path, Some(recovered));
                } else {
                    Self::onboarding(&path, None);
                }
                Self::get()
            }
        } else {
            println!("Failed to read config.json");
            Self::onboarding(&path, None);
            Self::get_from_path(path)
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RecoverConfig {
    token: Option<String>,
    guild_id: Option<String>,
    app_name: Option<String>,
    looptime: Option<u64>,
    data_path: Option<PathBuf>,
    gcloud_script: Option<String>,
    youtube_api_key: Option<String>,
}
