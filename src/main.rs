// clippy deny unwraps and expects
// #![deny(clippy::unwrap_used)]

mod commands;

mod radio;
mod video;
mod youtube;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
mod context_menu;

mod bigwetsloppybowser;

use anyhow::Error;
// use chrono::Timelike;
use commands::music::transcribe::{TranscribeChannelHandler, TranscribeData};
use commands::music::VoiceData;
use serde::{Deserialize, Serialize};
// use hyper;
// use hyper_rustls;
use serenity::async_trait;
use serenity::builder::CreateApplicationCommand;

use serenity::model::application::interaction::autocomplete::AutocompleteInteraction;
use serenity::model::application::interaction::Interaction;
use serenity::model::gateway::Ready;
use serenity::model::prelude::{GuildId, Member, Message, ResumedEvent};
use serenity::model::user::User;
// use serenity::model::webhook::Webhook;
// use tokio::io::AsyncWriteExt;
// use serenity::model::id::GuildId;
// use crate::bigwetsloppybowser::ShitGPT;
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

// lazy_static::lazy_static! {
//     static ref SHITGPT: Arc<Mutex<HashMap<String, ShitGPT>>> = Arc::new(Mutex::new(serde_json::from_reader(std::fs::File::open(Config::get().shitgpt_path).unwrap()).unwrap()));
// }

lazy_static::lazy_static! {
    static ref WHITELIST: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(serde_json::from_reader(std::fs::File::open(Config::get().whitelist_path).unwrap()).unwrap()));
}
// lazy_static::lazy_static! {
//     static ref WEBHOOKS: Arc<Mutex<HashMap<u64, Webhook>>> = Arc::new(Mutex::new(HashMap::new()));
// }

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSafe {
    pub id: String,
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
                    println!("Command not found: {command_name}");
                }
            }

            Interaction::Autocomplete(autocomplete) => {
                let commandn = autocomplete.data.name.clone();
                let command = self.commands.iter().find(|c| c.name() == commandn);
                if let Some(command) = command {
                    let r = command.autocomplete(&ctx, autocomplete).await;
                    if r.is_err() {}
                } else {
                    println!("Command not found: {commandn}");
                }
            }
            _ => {}
        }
    }

    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
        let mut users = Vec::new();
        for guild in ready.guilds {
            match ctx.http.get_guild(guild.id.0).await {
                Ok(guild) => {
                    for member in match guild.members(&ctx.http, None, None).await {
                        Ok(members) => members,
                        Err(e) => {
                            println!("Error getting members: {e}");
                            Vec::new()
                        }
                    } {
                        // check if user is not in users yet
                        let id = member.user.id.0.to_string();
                        if !users.contains(&id) {
                            users.push(id);
                        }
                    }
                }
                Err(e) => {
                    println!("Error getting guild: {e}");
                }
            }
        }
        let mut finalusers = Vec::new();
        for id in users {
            // let hashed_id = format!("{:x}", {
            //     let mut hasher = sha2::Sha512::new();
            //     hasher.update(id);
            //     hasher.finalize()
            // });
            finalusers.push(UserSafe { id });
        }

        let mut req = reqwest::Client::new()
            .post("http://localhost:16834/api/set/user")
            .json(&finalusers);
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to send users to api {e}. Users might be out of date");
        }

        let mut req = reqwest::Client::new()
            .post("http://localhost:16835/api/set/user")
            .json(&finalusers);
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to send users to api {e}. Users might be out of date");
        }

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
        // let commands = Command::get_global_application_commands(&ctx.http)
        //     .await
        //     .expect("Failed to get commands");
        // // delete all commands
        // for command in commands {
        //     Command::delete_global_application_command(&ctx.http, command.id)
        //         .await
        //         .expect("Failed to delete command");
        // }

        // enable when need to update commands
        println!("Register commands? (y/n)");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        if input.trim() == "y" {
            for command in self.commands.iter() {
                println!("Register command: {}? (y/n)", command.name());
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).unwrap();
                if input.trim() != "y" {
                    continue;
                }
                println!("Registering command: {}", command.name());
                Command::create_global_application_command(&ctx.http, |com| {
                    command.register(com);
                    com
                })
                .await
                .expect("Failed to register command");
            }
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

    async fn message(&self, ctx: Context, new_message: Message) {
        if new_message.author.bot {
            return;
        }
        if new_message.content.trim().is_empty() {
            return;
        }

        let guild_id = match new_message.guild_id {
            Some(guild) => guild,
            None => return,
        };

        let mut g = ctx.data.write().await;
        let mut f = g
            .get_mut::<TranscribeData>()
            .expect("Expected TranscribeData in TypeMap.")
            .lock()
            .await;
        let mut entry = f.entry(guild_id);
        let em = match entry {
            std::collections::hash_map::Entry::Occupied(ref mut e) => e.get_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let uh = TranscribeChannelHandler::new();
                // testing thread that just reads from rx and prints
                // let mut rx = uh.lock().unwrap();
                // tokio::spawn(async move {
                //     loop {
                //         let v = rx.next().await;
                //         if let Some(v) = v {
                //             println!("{:?}", v);
                //         }
                //     }
                // });
                e.insert(Arc::new(Mutex::new(uh)))
            }
        };

        let mut e = em.lock().await;

        let v = e.get_tts(&ctx, &new_message).await;

        for raw in v {
            if let Err(ugh) = e.send(raw).await {
                if let Some(ughh) = ugh.tts_audio_handle {
                    ughh.abort();
                }
            }
        }

        //     // let mut g = SHITGPT.lock().await;
        //     // let s = g
        //     //     .entry(new_message.author.id.as_u64().to_string())
        //     //     .or_insert(ShitGPT::new(7));
        //     // s.train(new_message.content_safe(&ctx));
        //     // // save shitgpt with serde_json
        //     // tokio::fs::write(
        //     //     Config::get().shitgpt_path,
        //     //     serde_json::to_string(&*g).unwrap(),
        //     // )
        //     // .await
        //     // .unwrap();

        //     // get current unix timestamp
        //     //
        //     // -------------------------------
        //     //
        //     // let validchars = "abcdefghijklmnopqrstuvwxyz";
        //     let t = std::time::SystemTime::now()
        //         .duration_since(std::time::UNIX_EPOCH)
        //         .unwrap()
        //         .as_secs();
        //     let string = new_message.content_safe(&ctx);
        //     //     .split_ascii_whitespace()
        //     //     .map(|s| TimedString {
        //     //         string: s
        //     //             .to_lowercase()
        //     //             .chars()
        //     //             .filter(|c| c.is_ascii())
        //     //             .collect::<String>(),
        //     //         time: t,
        //     //     })
        //     //     .filter(|s| !s.string.is_empty())
        //     //     .map(|mut s| {
        //     //         s.string = s
        //     //             .string
        //     //             .chars()
        //     //             .filter(|c| validchars.contains(*c))
        //     //             .collect::<String>();
        //     //         s
        //     //     })
        //     //     .collect::<Vec<TimedString>>();
        //     // make a request to localhost:16834
        //     if !string.is_empty() {
        //         let mut req = reqwest::Client::new()
        //             .post("http://localhost:16834/api/add/string")
        //             .json(&Timed {
        //                 thing: string,
        //                 time: t,
        //             });
        //         if let Some(token) = Config::get().string_api_token {
        //             req = req.bearer_auth(token);
        //         }
        //         if let Err(e) = req.send().await {
        //             println!("Failed to send strings to api {e}");
        //         }
        //     }
    }

    async fn resume(&self, ctx: Context, _: ResumedEvent) {
        // resync all users
        let mut users = Vec::new();
        for guild in match ctx.http.get_guilds(None, None).await {
            Ok(guilds) => guilds,
            Err(e) => {
                println!("Error getting guilds: {e}");
                return;
            }
        } {
            match ctx.http.get_guild(guild.id.0).await {
                Ok(guild) => {
                    for member in match guild.members(&ctx.http, None, None).await {
                        Ok(members) => members,
                        Err(e) => {
                            println!("Error getting members: {e}");
                            continue;
                        }
                    } {
                        // check if user is not in users yet
                        let id = member.user.id.0.to_string();
                        if !users.contains(&id) {
                            users.push(id);
                        }
                    }
                }
                Err(e) => {
                    println!("Error getting guild: {e}");
                }
            }
        }
        let mut finalusers = Vec::new();
        for id in users {
            // let hashed_id = format!("{:x}", {
            //     let mut hasher = sha2::Sha512::new();
            //     hasher.update(id);
            //     hasher.finalize()
            // });
            finalusers.push(UserSafe { id });
        }

        let mut req = reqwest::Client::new()
            .post("http://localhost:16834/api/set/user")
            .json(&finalusers);
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to send users to api {e}. Users might be out of date");
        }

        let mut req = reqwest::Client::new()
            .post("http://localhost:16835/api/set/user")
            .json(&finalusers);
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to send users to api {e}. Users might be out of date");
        }
    }

    async fn guild_member_addition(&self, _ctx: Context, new_member: Member) {
        // get hashed id
        // let id = format!("{:x}", {
        //     let mut hasher = sha2::Sha512::new();
        //     hasher.update(new_member.user.id.0.to_string());
        //     hasher.finalize()
        // });
        let id = new_member.user.id.0.to_string();

        let mut req = reqwest::Client::new()
            .post("http://localhost:16834/api/add/user")
            .json(&UserSafe { id: id.clone() });
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to add user to api {e}. Users might be out of date");
        }

        let mut req = reqwest::Client::new()
            .post("http://localhost:16835/api/add/user")
            .json(&UserSafe { id });
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to add user to api {e}. Users might be out of date");
        }
    }

    async fn guild_member_removal(
        &self,
        _ctx: Context,
        _guild_id: GuildId,
        user: User,
        _member_data_if_available: Option<Member>,
    ) {
        // get hashed id
        // let id = format!("{:x}", {
        //     let mut hasher = sha2::Sha512::new();
        //     hasher.update(user.id.0.to_string());
        //     hasher.finalize()
        // });
        let id = user.id.0.to_string();

        let mut req = reqwest::Client::new()
            .post("http://localhost:16834/api/remove/user")
            .json(&UserSafe { id: id.clone() });
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to remove user from api {e}. Users might be out of date");
        }

        let mut req = reqwest::Client::new()
            .post("http://localhost:16835/api/remove/user")
            .json(&UserSafe { id });
        if let Some(token) = Config::get().string_api_token {
            req = req.bearer_auth(token);
        }
        if let Err(e) = req.send().await {
            println!("Failed to remove user from api {e}. Users might be out of date");
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Timed<T> {
    thing: T,
    time: u64,
}

#[tokio::main]
async fn main() {
    // let v = crate::youtube::youtube_search("KIKUOWORLD2".to_owned())
    //     .await
    //     .unwrap();
    // let spoofytitle =
    //     "https://open.spotify.com/album/3WNehG6cwmM6dy37lXn70Z?si=mQJ-D1YcTxiOANF9DjiOYQ";

    // let v = crate::video::get_spotify_shiz(spoofytitle.to_string())
    //     .await
    //     .unwrap();
    // println!("{:?}", v);
    // panic!("test");

    let cfg = Config::get();

    let mut tmp = cfg.data_path.clone();
    tmp.push("tmp");

    let r = std::fs::remove_dir_all(&tmp);
    if r.is_err() {
        println!("Failed to remove tmp folder");
    }
    std::fs::create_dir_all(&tmp).expect("Failed to create tmp folder");

    let token = cfg.token;

    let handler = Handler::new(vec![
        Box::new(commands::music::transcribe::Transcribe),
        Box::new(commands::music::repeat::Repeat),
        Box::new(commands::music::loopit::Loop),
        Box::new(commands::music::pause::Pause),
        Box::new(commands::music::play::Play),
        Box::new(commands::music::join::Join),
        Box::new(commands::music::setbitrate::SetBitrate),
        Box::new(commands::music::remove::Remove),
        Box::new(commands::music::resume::Resume),
        Box::new(commands::music::shuffle::Shuffle),
        Box::new(commands::music::skip::Skip),
        Box::new(commands::music::stop::Stop),
        Box::new(commands::music::volume::Volume),
        Box::new(commands::music::autoplay::Autoplay),
        Box::new(commands::embed::Video),
        Box::new(commands::embed::Audio),
        Box::new(commands::embed::John),
        Box::new(commands::emulate::EmulateCommand),
    ]);

    let config = songbird::Config::default()
        .preallocated_tracks(2)
        .decode_mode(songbird::driver::DecodeMode::Pass)
        .crypto_mode(songbird::driver::CryptoMode::Lite);

    let mut client = Client::builder(token, GatewayIntents::all())
        .register_songbird_from_config(config)
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
        data.insert::<commands::music::transcribe::TranscribeData>(Arc::new(
            serenity::prelude::Mutex::new(HashMap::new()),
        ));
    }

    // tokio interval until the next six am
    let mut tick = tokio::time::interval({
        let now = chrono::Local::now();
        let mut next = chrono::Local::now()
            .date_naive()
            .and_hms_opt(8, 0, 0)
            .unwrap()
            .and_utc();
        if next < now {
            next += chrono::Duration::days(1);
        }
        let next = next - now.naive_utc().and_utc();
        tokio::time::Duration::from_secs(next.num_seconds() as u64)
    });

    // testing, wait 10 seconds
    // let mut tick = tokio::time::interval(tokio::time::Duration::from_secs(10));

    tick.tick().await;

    tokio::select! {
        _ = tick.tick() => {
            println!("Restarting at {}", chrono::Local::now());
            client.shard_manager.lock().await.shutdown_all().await;
            println!("Exit code 3 {}", chrono::Local::now());
            std::process::exit(3);
        }
        Err(why) = client.start() => {
            println!("Client error: {:?}", why);
        }
    }
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Config {
    token: String,
    guild_id: String,
    app_name: String,
    looptime: u64,
    data_path: PathBuf,
    shitgpt_path: PathBuf,
    whitelist_path: PathBuf,
    string_api_token: Option<String>,
    idle_url: String,
    api_url: Option<String>,
    #[cfg(feature = "tts")]
    gcloud_script: String,
    #[cfg(feature = "youtube-search")]
    youtube_api_key: String,
    #[cfg(feature = "spotify")]
    spotify_api_key: String,
    bumper_url: String,
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
                    Self::safe_read("\nPlease enter your loop time in ms\nlower time means faster response but potentially higher cpu utilization (50 is a good compromise):")
                },
                #[cfg(feature = "tts")]
                gcloud_script: if let Some(gcloud_script) = rec.gcloud_script {
                    gcloud_script
                } else {
                    Self::safe_read("\nPlease enter your gcloud script location (teehee):")
                },
                #[cfg(feature = "youtube-search")]
                youtube_api_key: if let Some(youtube_api_key) = rec.youtube_api_key {
                    youtube_api_key
                } else {
                    Self::safe_read("\nPlease enter your youtube api key:")
                },
                #[cfg(feature = "spotify")]
                spotify_api_key: if let Some(spotify_api_key) = rec.spotify_api_key {
                    spotify_api_key
                } else {
                    Self::safe_read("\nPlease enter your spotify api key:")
                },
                idle_url: if let Some(idle_audio) = rec.idle_url {
                    idle_audio
                } else {
                    Self::safe_read("\nPlease enter your idle audio URL (NOT A FILE PATH)\nif you wish to use a file on disk, set this to something as a fallback, and name the file override.mp3 inside the bot directory)\n(appdata/local/ for windows users and ~/.local/share/ for linux users):")
                },
                api_url: rec.api_url,
                bumper_url: if let Some(bumper_url) = rec.bumper_url {
                    bumper_url
                } else {
                    Self::safe_read("\nPlease enter your bumper audio URL (NOT A FILE PATH) (for silence put \"https://www.youtube.com/watch?v=Vbks4abvLEw\"):")
                },
                data_path,
                shitgpt_path: Self::safe_read("\nPlease enter your shitgpt path (teehee):"),
                whitelist_path: Self::safe_read("\nPlease enter your whitelist path (teehee):"),
                string_api_token: Some(Self::safe_read(
                    "\nPlease enter your string api token (teehee):",
                )),
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
                #[cfg(feature = "tts")]
                gcloud_script: Self::safe_read("\nPlease enter your gcloud script location (teehee):"),
                data_path,
                #[cfg(feature = "youtube-search")]
                youtube_api_key: Self::safe_read("\nPlease enter your youtube api key:"),
                #[cfg(feature = "spotify")]
                spotify_api_key: Self::safe_read("\nPlease enter your spotify api key:"),
                idle_url: Self::safe_read("\nPlease enter your idle audio URL (NOT A FILE PATH):"),
                api_url: None,
                bumper_url: Self::safe_read("\nPlease enter your bumper audio URL (NOT A FILE PATH) (for silence put \"https://www.youtube.com/watch?v=Vbks4abvLEw\"):"),
                shitgpt_path: Self::safe_read("\nPlease enter your shitgpt path (teehee):"),
                whitelist_path: Self::safe_read("\nPlease enter your whitelist path (teehee):"),
                string_api_token: Some(Self::safe_read("\nPlease enter your string api token (teehee):")),
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
    #[cfg(feature = "tts")]
    gcloud_script: Option<String>,
    #[cfg(feature = "youtube-search")]
    youtube_api_key: Option<String>,
    #[cfg(feature = "spotify")]
    spotify_api_key: Option<String>,
    idle_url: Option<String>,
    api_url: Option<String>,
    bumper_url: Option<String>,
}
