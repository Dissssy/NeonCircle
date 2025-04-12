#![feature(duration_millis_float, if_let_guard, try_blocks)]
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
pub use nanoid;
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

lazy_static::lazy_static!(
    pub static ref TEMP_PATH: std::path::PathBuf = {
        let mut path = <std::path::PathBuf as std::str::FromStr>::from_str("/tmp/").expect("Failed to get /tmp/");
        assert!(path.exists(), "temp path not found");
        path.push("neon_circle");
        if !path.exists() {
            std::fs::create_dir(&path).expect("Failed to create temp directory");
        }
        assert!(path.exists(), "could not create temp directory");
        // attempt to delete all files in the directory, since this is startup, just log if any deletion fails
        for entry in std::fs::read_dir(&path).expect("Failed to read temp directory") {
            log::warn!("Found entry: {:?}", entry);
            // let res: anyhow::Result<()> = try {
            //     let entry = entry?;
            //     if entry.file_type()?.is_file() {
            //         if let Err(e) = std::fs::remove_file(entry.path()) {
            //             log::warn!("Failed to delete file: {}", e);
            //         }
            //     }
            // };
            // if let Err(e) = res {
            //     log::warn!("Failed to delete file: {}", e);
            // }
        }
        path
    };
);
