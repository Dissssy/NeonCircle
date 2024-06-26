#![feature(duration_millis_float, if_let_guard)]
pub mod audio;
mod config;
pub mod global_data;
pub mod radio;
pub mod sam;
mod statics;
mod traits;
pub mod video;
pub mod youtube;
use std::sync::Arc;

pub use anyhow;
// pub mod voice_events;
pub use chrono;
pub use chrono_tz;
pub use config::{get_config, Config};
pub use lazy_static;
pub use log;
pub use rand;
pub use reqwest;
pub use serenity;
pub use songbird;
pub use statics::*;
pub use tokio;
pub use traits::{CommandTrait, SubCommandTrait};
pub mod utils;
pub enum PostSomething {
    Attachment { name: Arc<str>, data: Vec<u8> },
    Text(Arc<str>),
}
