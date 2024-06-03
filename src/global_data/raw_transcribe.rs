use serenity::{
    all::{ChannelId, Message},
    futures::{stream::FuturesUnordered, StreamExt as _},
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{broadcast, RwLock};
// we're gonna keep track of transcription data on a VC by VC basis
// so we want to store a map from guilds to a map from (voice) channels to a list of (text) channels
// discord won't re-use id's so we're going to trust the command to validate that the voice and text channels are the right way around
type VoiceChannel = ChannelId;
type TextChannel = ChannelId;
lazy_static::lazy_static!(
    // map to effectively work backwards from the text channel to the voice channel so we can get the broadcaster for the voice channel (using broadcast so we dont have to manage returning the receiver)
    static ref RWLOCK: RwLock<HashMap<VoiceChannel, Vec<TextChannel>>> = {
        let file = match std::fs::File::open(crate::config::get_config().transcription_map_path) {
            Ok(f) => f,
            Err(_) => {
                let f = match std::fs::File::create(crate::config::get_config().transcription_map_path) {
                    Ok(f) => f,
                    Err(e) => panic!("Failed to create guild config file: {}", e),
                };
                if let Err(e) = serde_json::to_writer(f, &HashMap::<VoiceChannel, Vec<TextChannel>>::new()) {
                    panic!("Failed to write default guild config file: {}", e);
                }
                match std::fs::File::open(crate::config::get_config().transcription_map_path) {
                    Ok(f) => f,
                    Err(e) => panic!("Failed to open guild config file: {}", e),
                }
            }
        };
        RwLock::new(
            match serde_json::from_reader(file) {
                Ok(r) => r,
                Err(e) => panic!("Failed to read guild config file: {}", e),
            }
        )
    };
    // map to store the broadcast channels for each voice channel
    static ref CHANNELS: RwLock<HashMap<VoiceChannel, Arc<RwLock<MessageBroadcast>>>> = RwLock::new(HashMap::new());
);
struct MessageBroadcast {
    sender: broadcast::Sender<Arc<Message>>,
    receiver: broadcast::Receiver<Arc<Message>>,
}
impl Default for MessageBroadcast {
    fn default() -> Self {
        // limit the size of the channel to 1mb of memory
        let (sender, receiver) = broadcast::channel(
            (1024usize * 1024usize).saturating_div(std::mem::size_of::<Arc<Message>>()),
        );
        Self { sender, receiver }
    }
}
pub async fn list_all_channels(channel: VoiceChannel) -> Vec<TextChannel> {
    let map = RWLOCK.read().await;
    map.get(&channel).cloned().unwrap_or_default()
}
pub async fn send_message(message: Message) {
    let message = Arc::new(message);
    // we want to get the sender for every voice channel that contains the text channel.
    // this is okay being a read lock because we're not going to modify the map, we will just trace for debugging and silently ignore if the channel is not found
    let mut send_to = Vec::new();
    {
        let map = RWLOCK.read().await;
        for (voice_channel, text_channels) in map.iter() {
            if text_channels.contains(&message.channel_id) {
                send_to.push(*voice_channel);
            }
        }
    }
    let mut broadcasters = Vec::new();
    {
        let channels = CHANNELS.read().await;
        for voice_channel in send_to {
            if let Some(broadcast) = channels.get(&voice_channel) {
                broadcasters.push(Arc::clone(broadcast));
            }
        }
    }
    let mut futures = broadcasters
        .into_iter()
        .map(|broadcaster| {
            let message = Arc::clone(&message);
            async move {
                let broadcaster = broadcaster.read().await;
                broadcaster.sender.send(message)
            }
        })
        .collect::<FuturesUnordered<_>>();
    while let Some(res) = futures.next().await {
        if let Err(e) = res {
            log::error!(
                "Failed to send message: {} to voice channel: {}",
                e,
                message.channel_id
            );
        }
    }
}
pub async fn get_receiver(channel: VoiceChannel) -> broadcast::Receiver<Arc<Message>> {
    // ensure the voice channel exists in the voice channel to text channel map, if not default it to a list only containing the voice channel
    {
        let mut map = RWLOCK.write().await;
        map.entry(channel).or_insert_with(|| vec![channel]);
    }
    let broadcaster = {
        let mut channels = CHANNELS.write().await;
        Arc::clone(channels.entry(channel).or_default())
    };
    let broadcaster = broadcaster.read().await;
    broadcaster.receiver.resubscribe()
}
pub async fn set_channel(
    voice_channel: VoiceChannel,
    text_channel: TextChannel,
    keep_or_put: bool,
) {
    {
        let mut map = RWLOCK.write().await;
        {
            let entry = map.entry(voice_channel).or_insert(vec![voice_channel]);
            if keep_or_put {
                entry.push(text_channel);
                entry.sort();
                entry.dedup();
            } else {
                entry.retain(|&c| c != text_channel);
            }
        }
    }
    save().await;
}
pub async fn clear_channel(voice_channel: VoiceChannel) {
    {
        let mut map = RWLOCK.write().await;
        let entry = map.entry(voice_channel).or_insert(vec![]);
        entry.clear();
    }
    save().await;
}
pub async fn save() {
    let map = RWLOCK.read().await;
    let file = match std::fs::File::create(crate::config::get_config().transcription_map_path) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Failed to create guild config file: {}", e);
            return;
        }
    };
    if let Err(e) = serde_json::to_writer(file, &*map) {
        log::error!("Failed to write guild config file: {}", e);
    }
}
