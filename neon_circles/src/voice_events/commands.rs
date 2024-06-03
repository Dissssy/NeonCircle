use crate::{
    commands::music::{AudioPromiseCommand, OrToggle},
    video::Video,
};
use anyhow::Result;
use common::log;
use common::serenity::all::*;
use std::{pin::Pin, sync::Arc};
fn filter_input(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .collect::<Vec<&str>>()
        .join(" ")
}
async fn parse_commands(s: &str, u: UserId, http: Arc<Http>) -> WithFeedback {
    if s.is_empty() {
        return WithFeedback::new_without_feedback(Box::pin(
            async move { Ok(ParsedCommand::None) },
        ))
        .await;
    }
    let filtered = filter_input(s);
    if filtered.is_empty() {
        return WithFeedback::new_without_feedback(Box::pin(
            async move { Ok(ParsedCommand::None) },
        ))
        .await;
    }
    if filtered.contains("i do not consent to being recorded") {
        return WithFeedback::new_without_feedback(Box::pin(async move {
            Ok(ParsedCommand::MetaCommand(Command::NoConsent))
        }))
        .await;
    }
    let with_aliases = ALERT_PHRASES.filter(filtered);
    let (command, args): (&str, Vec<&str>) = {
        if let Some(alert) = ALERT_PHRASES.get_alert(&with_aliases) {
            let mut split = with_aliases.split(&alert.main);
            split.next();
            let rest = split.next().unwrap_or("");
            let mut split = rest.split_whitespace();
            let command = match split.next() {
                Some(command) => command,
                None => {
                    return WithFeedback::new_with_feedback(
                        Box::pin(async move { Ok(ParsedCommand::None) }),
                        "You need to say a command",
                    )
                    .await;
                }
            };
            let args = split.collect();
            (command, args)
        } else {
            return WithFeedback::new_without_feedback(Box::pin(
                async move { Ok(ParsedCommand::None) },
            ))
            .await;
        }
    };
    match command {
        t if ["play", "add", "queue", "played"].contains(&t) => {
            let query = args.join(" ");
            let http = Arc::clone(&http);
            if query.replace(' ', "").contains("wonderwall") {
                WithFeedback::new_with_feedback(
                    Box::pin(async move {
                        Ok(ParsedCommand::Command(AudioPromiseCommand::Play(
                            get_videos(query, http, u).await?,
                        )))
                    }),
                    "Anyway, here's wonderwall",
                )
                .await
            } else {
                let response = format!("Adding {} to the queue", query);
                WithFeedback::new_with_feedback(
                    Box::pin(async move {
                        Ok(ParsedCommand::Command(AudioPromiseCommand::Play(
                            get_videos(query, http, u).await?,
                        )))
                    }),
                    &response,
                )
                .await
            }
        }
        t if ["stop", "leave", "disconnect"].contains(&t) => {
            WithFeedback::new_with_feedback(
                Box::pin(async move {
                    Ok(ParsedCommand::Command(AudioPromiseCommand::Stop(Some(
                        tokio::time::Duration::from_millis(2500),
                    ))))
                }),
                "Goodbuy, my friend",
            )
            .await
        }
        t if ["skip", "next"].contains(&t) => {
            WithFeedback::new_with_feedback(
                Box::pin(async move { Ok(ParsedCommand::Command(AudioPromiseCommand::Skip)) }),
                "Skipping",
            )
            .await
        }
        t if ["pause"].contains(&t) => {
            WithFeedback::new_with_feedback(
                Box::pin(async move {
                    Ok(ParsedCommand::Command(AudioPromiseCommand::Paused(
                        OrToggle::Specific(true),
                    )))
                }),
                "Pausing",
            )
            .await
        }
        t if ["resume", "unpause"].contains(&t) => {
            WithFeedback::new_with_feedback(
                Box::pin(async move {
                    Ok(ParsedCommand::Command(AudioPromiseCommand::Paused(
                        OrToggle::Specific(false),
                    )))
                }),
                "Resuming",
            )
            .await
        }
        t if ["volume", "vol"].contains(&t) => {
            if let Some(vol) = attempt_to_parse_number(&args) {
                if vol <= 100 {
                    WithFeedback::new_with_feedback(
                        Box::pin(async move {
                            Ok(ParsedCommand::Command(AudioPromiseCommand::Volume(
                                crate::commands::music::SpecificVolume::Current(
                                    vol.clamp(0, 100) as f32 / 100.0,
                                ),
                            )))
                        }),
                        &format!("Setting volyume to {}%", humanize_number(vol)),
                    )
                    .await
                } else {
                    WithFeedback::new_with_feedback(
                        Box::pin(async move { Ok(ParsedCommand::None) }),
                        "Volyume must be between zero and one hundred",
                    )
                    .await
                }
            } else {
                WithFeedback::new_with_feedback(
                    Box::pin(async move { Ok(ParsedCommand::None) }),
                    "You need to say a number for the volyume",
                )
                .await
            }
        }
        t if ["remove", "delete"].contains(&t) => {
            if let Some(index) = attempt_to_parse_number(&args) {
                WithFeedback::new_with_feedback(
                    Box::pin(async move {
                        Ok(ParsedCommand::Command(AudioPromiseCommand::Remove(index)))
                    }),
                    &format!("Removing song {} from queue", index),
                )
                .await
            } else {
                WithFeedback::new_with_feedback(
                    Box::pin(async move { Ok(ParsedCommand::None) }),
                    "You need to say a number for the index",
                )
                .await
            }
        }
        t if ["say", "echo"].contains(&t) => {
            WithFeedback::new_with_feedback(
                Box::pin(async move { Ok(ParsedCommand::None) }),
                &args.join(" "),
            )
            .await
        }
        unknown => {
            WithFeedback::new_with_feedback(
                Box::pin(async move { Ok(ParsedCommand::None) }),
                &format!("Unknown command. {}", unknown),
            )
            .await
        }
    }
}
#[derive(Debug)]
pub enum ParsedCommand {
    None,
    MetaCommand(Command),
    Command(AudioPromiseCommand),
}
pub struct WithFeedback {
    command: Pin<Box<dyn std::future::Future<Output = Result<ParsedCommand>> + Send>>,
    feedback: Option<Video>,
}
impl std::fmt::Debug for WithFeedback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WithFeedback")
            .field("command", &"Future")
            .field("feedback", &self.feedback)
            .finish()
    }
}
impl WithFeedback {
    async fn new_with_feedback(
        command: Pin<Box<dyn std::future::Future<Output = Result<ParsedCommand>> + Send>>,
        feedback: &str,
    ) -> Self {
        Self {
            command,
            feedback: get_speech(feedback).await,
        }
    }
    async fn new_without_feedback(
        command: Pin<Box<dyn std::future::Future<Output = Result<ParsedCommand>> + Send>>,
    ) -> Self {
        Self {
            command,
            feedback: None,
        }
    }
}
#[derive(Debug)]
pub enum Command {
    NoConsent,
}
fn attempt_to_parse_number(args: &[&str]) -> Option<usize> {
    let mut num = 0;
    for word in args {
        match *word {
            "zero" => num += 0,
            "one" => num += 1,
            "two" => num += 2,
            "three" => num += 3,
            "four" => num += 4,
            "five" => num += 5,
            "six" => num += 6,
            "seven" => num += 7,
            "eight" => num += 8,
            "nine" => num += 9,
            "ten" => num += 10,
            "eleven" => num += 11,
            "twelve" => num += 12,
            "thirteen" => num += 13,
            "fourteen" => num += 14,
            "fifteen" => num += 15,
            "sixteen" => num += 16,
            "seventeen" => num += 17,
            "eighteen" => num += 18,
            "nineteen" => num += 19,
            "twenty" => num += 20,
            "thirty" => num += 30,
            "forty" => num += 40,
            "fifty" => num += 50,
            "sixty" => num += 60,
            "seventy" => num += 70,
            "eighty" => num += 80,
            "ninety" => num += 90,
            "hundred" => num *= 100,
            "thousand" => num *= 1000,
            "million" => num *= 1000000,
            "billion" => num *= 1000000000,
            "trillion" => num *= 1000000000000,
            "quadrillion" => num *= 1000000000000000,
            "quintillion" => num *= 1000000000000000000,
            n if let Ok(n) = n.parse::<usize>() => num += n,
            _ => {
                log::error!("Error parsing number: {:?} from {}", word, args.join(" "));
                return None;
            }
        }
    }
    Some(num)
}
pub fn humanize_number(n: usize) -> String {
    if n == 0 {
        return "zero".to_owned();
    }
    let mut n = n;
    let mut words = Vec::new();
    if n >= 1000 {
        words.push(humanize_number(n / 1000));
        words.push("thousand".to_owned());
        n %= 1000;
    }
    if n >= 100 {
        words.push(humanize_number(n / 100));
        words.push("hundred".to_owned());
        n %= 100;
    }
    if !words.is_empty() && n > 0 {
        words.push("and".to_owned());
    }
    if n >= 20 {
        match n / 10 {
            2 => words.push("twenty".to_owned()),
            3 => words.push("thirty".to_owned()),
            4 => words.push("forty".to_owned()),
            5 => words.push("fifty".to_owned()),
            6 => words.push("sixty".to_owned()),
            7 => words.push("seventy".to_owned()),
            8 => words.push("eighty".to_owned()),
            9 => words.push("ninety".to_owned()),
            _ => unreachable!(),
        }
        n %= 10;
    }
    if n >= 10 {
        match n {
            10 => words.push("ten".to_owned()),
            11 => words.push("eleven".to_owned()),
            12 => words.push("twelve".to_owned()),
            13 => words.push("thirteen".to_owned()),
            14 => words.push("fourteen".to_owned()),
            15 => words.push("fifteen".to_owned()),
            16 => words.push("sixteen".to_owned()),
            17 => words.push("seventeen".to_owned()),
            18 => words.push("eighteen".to_owned()),
            19 => words.push("nineteen".to_owned()),
            _ => unreachable!(),
        }
        n = 0;
    }
    if n > 0 {
        match n {
            1 => words.push("one".to_owned()),
            2 => words.push("two".to_owned()),
            3 => words.push("three".to_owned()),
            4 => words.push("four".to_owned()),
            5 => words.push("five".to_owned()),
            6 => words.push("six".to_owned()),
            7 => words.push("seven".to_owned()),
            8 => words.push("eight".to_owned()),
            9 => words.push("nine".to_owned()),
            _ => unreachable!(),
        }
    }
    words.join(" ")
}
async fn get_speech(text: &str) -> Option<Video> {
    let text = if text.ends_with('.') {
        text.to_owned()
    } else {
        format!("{}.", text)
    };
    match crate::sam::get_speech(&text) {
        Ok(vid) => Some(vid),
        Err(e) => {
            log::error!("Error getting speech: {:?}", e);
            None
        }
    }
}
async fn get_videos(
    query: String,
    http: Arc<Http>,
    u: UserId,
) -> Result<Vec<crate::commands::music::MetaVideo>> {
    let vids = crate::video::Video::get_video(&query, true, true).await;
    match vids {
        Ok(vids) => {
            let mut truevideos = Vec::new();
            #[cfg(feature = "tts")]
            let key = crate::youtube::get_access_token().await;
            for v in vids {
                let title = match &v {
                    crate::commands::music::VideoType::Disk(v) => v.title(),
                    crate::commands::music::VideoType::Url(v) => v.title(),
                };
                #[cfg(feature = "tts")]
                if let Ok(key) = key.as_ref() {
                    truevideos.push(crate::commands::music::MetaVideo {
                        video: v,
                        ttsmsg: Some(crate::commands::music::LazyLoadedVideo::new(tokio::spawn(
                            crate::youtube::get_tts(Arc::clone(&title), key.clone(), None),
                        ))),
                        // title,
                        author: http.get_user(u).await.ok().map(|u| {
                            crate::commands::music::Author {
                                name: u.name.clone(),
                                pfp_url: u
                                    .avatar_url()
                                    .clone()
                                    .unwrap_or(u.default_avatar_url().clone()),
                            }
                        }),
                    })
                } else {
                    truevideos.push(crate::commands::music::MetaVideo {
                        video: v,
                        ttsmsg: None,
                        // title,
                        author: http.get_user(u).await.ok().map(|u| {
                            crate::commands::music::Author {
                                name: u.name.clone(),
                                pfp_url: u.avatar_url().unwrap_or(u.default_avatar_url().clone()),
                            }
                        }),
                    });
                }
                #[cfg(not(feature = "tts"))]
                truevideos.push(MetaVideo { video: v, title });
            }
            Ok(truevideos)
        }
        Err(e) => {
            log::error!("Error getting video: {:?}", e);
            Err(anyhow::anyhow!("Error getting audio."))
        }
    }
}
fn human_readable_size(size: usize) -> String {
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let mut size = size as f64;
    let mut i = 0;
    while size >= 1024.0 {
        size /= 1024.0;
        i += 1;
    }
    format!("{:.2} {}", size, units.get(i).unwrap_or(&"??"))
}
lazy_static::lazy_static!(
    pub static ref ALERT_PHRASES: Alerts = {
        let file = crate::config::get_config().alert_phrases_path;
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) => {
                log::error!("Error reading alert phrases: {:?}", e);
                panic!("Error reading alert phrases: {:?}", e);
            }
        };
        let mut the = match serde_json::from_str::<Alerts>(&text) {
            Ok(the) => the,
            Err(e) => {
                log::error!("Error deserializing alert phrases: {:?}", e);
                panic!("Error deserializing alert phrases: {:?}", e);
            }
        };
        for alert in &mut the.phrases {
            alert.main.push(' ');
            for alias in &mut alert.aliases {
                alias.push(' ');
            }
        }
        the
    };
);
#[derive(Debug, serde::Deserialize)]
pub struct Alerts {
    phrases: Vec<Alert>,
}
impl std::fmt::Display for Alerts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for alert in &self.phrases {
            writeln!(f, "{}", alert)?;
        }
        Ok(())
    }
}
impl Alerts {
    fn filter(&self, s: String) -> String {
        let mut s = s;
        for alert in &self.phrases {
            for alias in &alert.aliases {
                s = s.replace(alias, &alert.main);
            }
        }
        s
    }
    fn get_alert(&'static self, s: &str) -> Option<&'static Alert> {
        self.phrases.iter().find(|a| s.contains(&a.main))
    }
}
#[derive(Debug, serde::Deserialize)]
pub struct Alert {
    main: String,
    aliases: Vec<String>,
}
impl std::fmt::Display for Alert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: [{}]", self.main, self.aliases.join(", "))
    }
}
