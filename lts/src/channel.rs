// CREATE TABLE IF NOT EXISTS channels (
//     -- The voice channel ID
//     voice_id BIGINT PRIMARY KEY,
//     -- The text channel IDs
//     text_ids BIGINT[] NOT NULL DEFAULT '{}'
// );

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use common::{
    anyhow::{anyhow, Result},
    lazy_static::lazy_static,
    log,
    serenity::{
        all::{ChannelId, Message},
        futures::{stream::FuturesUnordered, StreamExt as _},
    },
    tokio::sync::{broadcast, RwLock},
};

pub struct Channel {
    pub voice_id: ChannelId,
    pub text_ids: HashSet<ChannelId>,
}

impl Channel {
    pub async fn load_opt(voice_id: ChannelId) -> Result<Option<Self>> {
        let mut conn = crate::get_connection().await?;
        get::full(voice_id, &mut conn).await
    }
    pub async fn load(voice_id: ChannelId) -> Result<Self> {
        let mut conn = crate::get_connection().await?;
        match get::full(voice_id, &mut conn).await? {
            Some(channel) => Ok(channel),
            None => {
                set::full(
                    Self {
                        voice_id,
                        text_ids: HashSet::new(),
                    },
                    &mut conn,
                )
                .await?;
                match get::full(voice_id, &mut conn).await? {
                    Some(channel) => {
                        conn.commit().await?;
                        Ok(channel)
                    }
                    None => Err(anyhow!("Failed to write default channel")),
                }
            }
        }
    }
    pub async fn save(self) -> Result<()> {
        let mut conn = crate::get_connection().await?;
        set::full(self, &mut conn).await?;
        conn.commit().await?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct RawChannel {
    voice_id: i64,
    text_ids: Vec<i64>,
}

impl From<RawChannel> for Channel {
    fn from(raw: RawChannel) -> Self {
        Self {
            voice_id: ChannelId::new(raw.voice_id as u64),
            text_ids: raw
                .text_ids
                .into_iter()
                .map(|i| ChannelId::new(i as u64))
                .collect(),
        }
    }
}

mod get {
    use super::{Channel, ChannelId, RawChannel, Result};
    use sqlx::query_as;

    #[derive(sqlx::FromRow)]
    struct Id {
        voice_id: i64,
    }

    pub async fn all_ids_that_contain(
        text_id: ChannelId,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Vec<ChannelId>> {
        Ok(query_as!(
            Id,
            "SELECT voice_id FROM channels WHERE $1 = ANY(text_ids)",
            text_id.get() as i64
        )
        .fetch_all(&mut **conn)
        .await?
        .into_iter()
        .map(|i| ChannelId::new(i.voice_id as u64))
        .collect())
    }

    pub async fn full(
        voice_id: ChannelId,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Option<Channel>> {
        Ok(query_as!(
            RawChannel,
            "SELECT voice_id, text_ids FROM channels WHERE voice_id = $1",
            voice_id.get() as i64
        )
        .fetch_optional(&mut **conn)
        .await?
        .map(Into::into))
    }
}

mod set {
    use super::{Channel, Result};
    use sqlx::query;

    pub async fn full(
        channel: Channel,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<()> {
        query!(
            "INSERT INTO channels (voice_id, text_ids) VALUES ($1, $2)
             ON CONFLICT (voice_id) DO UPDATE SET text_ids = $2",
            channel.voice_id.get() as i64,
            &channel
                .text_ids
                .iter()
                .map(|id| id.get() as i64)
                .collect::<Vec<_>>()
        )
        .execute(&mut **conn)
        .await?;
        Ok(())
    }
}

pub async fn migrate_data_from_json(
    conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<()> {
    let channels = common::global_data::extract::transcribe().await;
    for (id, channels) in channels {
        set::full(
            Channel {
                voice_id: id,
                text_ids: channels.into_iter().collect(),
            },
            conn,
        )
        .await?;
    }
    Ok(())
}

lazy_static! {
    static ref CHANNELS: RwLock<HashMap<ChannelId, Arc<RwLock<MessageBroadcast>>>> =
        RwLock::new(HashMap::new());
}
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
pub async fn send_message(message: Message) {
    let message = Arc::new(message);
    // we want to get the sender for every voice channel that contains the text channel.
    // this is okay being a read lock because we're not going to modify the map, we will just trace for debugging and silently ignore if the channel is not found
    // let mut send_to = Vec::new();
    // {
    //     let map =
    //     for (voice_channel, text_channels) in map.iter() {
    //         if text_channels.contains(&message.channel_id) {
    //             send_to.push(*voice_channel);
    //         }
    //     }
    // }
    let mut conn = match crate::get_connection().await {
        Ok(conn) => conn,
        Err(e) => {
            log::error!("Failed to get connection: {}", e);
            return;
        }
    };
    let send_to = get::all_ids_that_contain(message.channel_id, &mut conn)
        .await
        .unwrap_or_default();
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
pub async fn get_receiver(channel: ChannelId) -> Result<broadcast::Receiver<Arc<Message>>> {
    // ensure the voice channel exists in the voice channel to text channel map, if not default it to a list only containing the voice channel
    {
        let mut conn = crate::get_connection().await?;
        if get::full(channel, &mut conn).await?.is_none() {
            set::full(
                Channel {
                    voice_id: channel,
                    text_ids: vec![channel].into_iter().collect(),
                },
                &mut conn,
            )
            .await?;
        }
    }
    let broadcaster = {
        let mut channels = CHANNELS.write().await;
        Arc::clone(channels.entry(channel).or_default())
    };
    let broadcaster = broadcaster.read().await;
    Ok(broadcaster.receiver.resubscribe())
}
