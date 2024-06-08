// CREATE TABLE IF NOT EXISTS reminders (
//     -- uuid for the reminder
//     id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
//     -- discord user id
//     user_id BIGINT NOT NULL,
//     -- discord channel id (optional, as a user can create a reminder in the bot's DMs)
//     channel_id BIGINT,
//     -- discord guild id (optional, as a user can create a reminder in the bot's DMs)
//     guild_id BIGINT,
//     -- reminder message
//     message TEXT NOT NULL,
//     -- reminder time
//     remind_at TIMESTAMP NOT NULL,
//     -- created at
//     created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
//     -- updated at
//     updated_at TIMESTAMP,
//     -- send attempt count
//     send_attempt_count INT NOT NULL DEFAULT 0
// );

use common::{
    anyhow::{anyhow, Result},
    chrono,
    chrono_tz::Tz,
    log,
    serenity::all::{
        Channel, ChannelId, Context, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter,
        CreateMessage, GuildId, Timestamp, UserId,
    },
};
use sqlx::types::Uuid;

const MAX_FAILED_SEND_ATTEMPTS: i32 = 3;

pub struct Reminder {
    id: Uuid,
    pub user_id: UserId,
    pub channel_id: Option<ChannelId>,
    pub guild_id: Option<GuildId>,
    pub message: String,
    pub remind_at: chrono::DateTime<Tz>,
    pub created_at: chrono::DateTime<Tz>,
    pub updated_at: Option<chrono::DateTime<Tz>>,
    send_attempt_count: i32,
}

impl Reminder {
    pub async fn new(
        user_id: UserId,
        channel_id: Option<ChannelId>,
        guild_id: Option<GuildId>,
        message: &str,
        remind_at: chrono::DateTime<Tz>,
    ) -> Result<Self> {
        let mut conn = crate::get_connection().await?;
        let raw = set::new(user_id, channel_id, guild_id, message, remind_at, &mut conn).await?;
        let user = crate::User::load(user_id).await?;
        let reminder = Reminder::from_raw(raw, &user.timezone);
        conn.commit().await?;
        Ok(reminder)
    }

    pub async fn failed(mut self) -> Result<()> {
        self.send_attempt_count += 1;
        if self.send_attempt_count >= MAX_FAILED_SEND_ATTEMPTS {
            self.delete().await?;
        } else {
            self.save().await?;
        }
        Ok(())
    }

    pub fn id(&self) -> Uuid {
        self.id
    }
    pub async fn from_id(raw_uuid: &str) -> Result<Self> {
        let uuid = Uuid::parse_str(raw_uuid)?;
        let mut conn = crate::get_connection().await?;
        let reminder = get::specific(uuid, &mut conn).await?;
        if let Some(reminder) = reminder {
            Ok(reminder)
        } else {
            Err(anyhow!("Reminder not found"))
        }
    }
    pub async fn nudge_forward(raw_uuid: &str) -> Result<Reminder> {
        // nudge this reminder forward by 1 hour
        let uuid = Uuid::parse_str(raw_uuid)?;
        let mut conn = crate::get_connection().await?;
        let reminder = get::specific(uuid, &mut conn).await?;
        if let Some(mut reminder) = reminder {
            reminder.remind_at += chrono::Duration::hours(1);
            set::full(&reminder, &mut conn).await?;
            conn.commit().await?;
            Ok(reminder)
        } else {
            Err(anyhow!("Reminder not found"))
        }
    }
    pub async fn nudge_backward(raw_uuid: &str) -> Result<Reminder> {
        // nudge this reminder backward by 1 hour
        let uuid = Uuid::parse_str(raw_uuid)?;
        let mut conn = crate::get_connection().await?;
        let reminder = get::specific(uuid, &mut conn).await?;
        if let Some(mut reminder) = reminder {
            reminder.remind_at -= chrono::Duration::hours(1);
            set::full(&reminder, &mut conn).await?;
            conn.commit().await?;
            Ok(reminder)
        } else {
            Err(anyhow!("Reminder not found"))
        }
    }

    pub async fn all_reminders_for(user_id: UserId, page: i64) -> Result<PaginatedReminders> {
        let mut conn = crate::get_connection().await?;
        let (reminders, more) = get::all(user_id, page, &mut conn).await?;
        Ok(PaginatedReminders {
            reminders,
            page,
            more,
        })
    }
    pub async fn all_reminders(before: chrono::DateTime<chrono::Utc>) -> Result<Vec<Self>> {
        let mut conn = crate::get_connection().await?;
        let mut reminders = get::all_before_besides(before, &mut conn).await?;
        reminders.sort_by_key(|r| r.remind_at);
        Ok(reminders)
    }
    pub async fn save(self) -> Result<()> {
        let mut conn = crate::get_connection().await?;
        set::full(&self, &mut conn).await?;
        conn.commit().await?;
        Ok(())
    }
    pub async fn remind(&mut self, ctx: &Context) -> Result<()> {
        // first we want to ensure the reminder has expired
        if self.remind_at > chrono::Utc::now() {
            return Err(anyhow!("Reminder is not ready to be sent"));
        }
        // we want to check again to ensure the reminder has not been updated or deleted
        let mut conn = crate::get_connection().await?;

        let reminder = get::specific(self.id, &mut conn).await?;
        if let Some(reminder) = reminder {
            // ensure the reminder time was not updated, if it was, ensure the time is LATER than the original time, otherwise we can continue
            if reminder.remind_at != self.remind_at {
                return Err(anyhow!("Reminder was updated"));
            } else if reminder.remind_at > chrono::Utc::now() {
                return Err(anyhow!("Reminder was updated and is not ready to be sent"));
            }
            if reminder.message != self.message {
                self.message = reminder.message;
            }
        } else {
            return Err(anyhow!("Reminder was deleted"));
        }

        // then we want to send the reminder
        let user = self.user_id.to_user(ctx).await?;
        let (author_name, author_url, author_icon): (String, Option<String>, String) = {
            let channel_id = self.channel_id.as_ref().copied();
            let guild_id = self.guild_id.as_ref().copied();
            async move {
                let fallback = {
                    let current_avatar = {
                        let current_user = ctx.cache.current_user();
                        current_user
                            .avatar_url()
                            .unwrap_or_else(|| current_user.default_avatar_url())
                    };
                    ("Bot DMs".to_owned(), None, current_avatar)
                };
                let guild = match guild_id {
                    Some(id) => match id.to_partial_guild(ctx).await {
                        Ok(guild) => guild,
                        Err(e) => {
                            log::error!("Failed to fetch guild: {:?}", e);
                            return fallback;
                        }
                    },
                    None => return fallback,
                };

                let channel = match channel_id {
                    Some(id) => match id.to_channel(ctx).await {
                        Ok(channel) => channel,
                        Err(e) => {
                            log::error!("Failed to fetch channel: {:?}", e);
                            return fallback;
                        }
                    },
                    None => return fallback,
                };

                let channel_name = match channel {
                    Channel::Guild(ref channel) => &channel.name,
                    _ => return fallback,
                };

                let guild_icon = match guild.icon_url() {
                    Some(url) => url,
                    None => fallback.2,
                };

                (
                    format!("{} #{}", guild.name, channel_name),
                    Some(format!(
                        "https://discord.com/channels/{}/{}",
                        guild.id.get(),
                        channel.id().get()
                    )),
                    guild_icon,
                )
            }
            .await
        };
        user.direct_message(
            ctx,
            // CreateMessage::new().content(&format!(
            //     "On <t:{}:F> you asked me to remind you about:\n`{}`",
            //     self.remind_at.timestamp(),
            //     self.message
            // )),
            CreateMessage::new().embed({
                let mut embed = CreateEmbed::new()
                    .author({
                        let mut author = CreateEmbedAuthor::new(format!("In: {}", author_name));
                        if let Some(url) = author_url {
                            author = author.url(url);
                        }
                        author.icon_url(author_icon)
                    })
                    .title("You asked me to remind you about")
                    .description(&self.message);

                if let Ok(timestamp) = Timestamp::from_unix_timestamp(self.remind_at.timestamp()) {
                    embed = embed
                        .footer(CreateEmbedFooter::new("You requested this reminder on"))
                        .timestamp(timestamp);
                } else {
                    embed = embed.title(format!(
                        // "On <t:{}:F> you asked me to remind you about",
                        "On {} you asked me to remind you about",
                        common::utils::full_datetime_format(&self.remind_at, true)
                    ));
                }
                embed
            }),
        )
        .await?;

        // then delete the reminder
        set::delete(self.id, &mut conn).await?;
        conn.commit().await?;
        Ok(())
    }
    pub async fn delete(self) -> Result<()> {
        let mut conn = crate::get_connection().await?;
        set::delete(self.id, &mut conn).await?;
        conn.commit().await?;
        Ok(())
    }
}

pub struct PaginatedReminders {
    pub reminders: Vec<Reminder>,
    pub page: i64,
    pub more: bool,
}

#[derive(sqlx::FromRow)]
struct RawReminder {
    id: Uuid,
    user_id: i64,
    channel_id: Option<i64>,
    guild_id: Option<i64>,
    message: String,
    remind_at: chrono::NaiveDateTime,
    created_at: chrono::NaiveDateTime,
    updated_at: Option<chrono::NaiveDateTime>,
    send_attempt_count: i32,
}

impl Reminder {
    fn from_raw(raw: RawReminder, timezone: &Tz) -> Self {
        Self {
            id: raw.id,
            user_id: UserId::new(raw.user_id as u64),
            channel_id: raw.channel_id.map(|id| ChannelId::new(id as u64)),
            guild_id: raw.guild_id.map(|id| GuildId::new(id as u64)),
            message: raw.message,
            remind_at: raw.remind_at.and_utc().with_timezone(timezone),
            created_at: raw.created_at.and_utc().with_timezone(timezone),
            updated_at: raw.updated_at.map(|t| t.and_utc().with_timezone(timezone)),
            send_attempt_count: raw.send_attempt_count,
        }
    }
}

mod get {
    use common::chrono_tz::Tz;

    use super::{chrono, RawReminder, Reminder, Result, UserId, Uuid};

    pub async fn all(
        user_id: UserId,
        page: i64,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(Vec<Reminder>, bool)> {
        let user = crate::User::load(user_id).await?;
        let mut reminders = sqlx::query_as!(
            RawReminder,
            "SELECT * FROM reminders WHERE user_id = $1 ORDER BY remind_at ASC LIMIT 26 OFFSET $2",
            user_id.get() as i64,
            page * 25,
        )
        .fetch_all(&mut **conn)
        .await?
        .into_iter()
        .map(|raw| Reminder::from_raw(raw, &user.timezone))
        .collect::<Vec<_>>();

        reminders.sort_by_key(|r| r.remind_at);

        let twenty_sixth = reminders.len() == 26;
        if twenty_sixth {
            reminders.pop();
        }

        Ok((reminders, twenty_sixth))
    }

    pub async fn all_before_besides(
        before: chrono::DateTime<chrono::Utc>,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Vec<Reminder>> {
        let raw_reminders = sqlx::query_as!(
            RawReminder,
            "SELECT * FROM reminders WHERE remind_at < $1",
            before.naive_utc(),
        )
        .fetch_all(&mut **conn)
        .await?;

        let mut user_map: std::collections::HashMap<UserId, Tz> = std::collections::HashMap::new();

        let mut reminders = Vec::new();

        for raw in raw_reminders.into_iter() {
            let user_id = UserId::new(raw.user_id as u64);
            let tz = match user_map.entry(user_id) {
                std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let user = crate::User::load(user_id).await?;
                    *entry.insert(user.timezone)
                }
            };

            reminders.push(Reminder::from_raw(raw, &tz));
        }

        reminders.sort_by_key(|r| r.remind_at);

        Ok(reminders)
    }

    pub async fn specific(
        id: Uuid,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Option<Reminder>> {
        match sqlx::query_as!(RawReminder, "SELECT * FROM reminders WHERE id = $1", id)
            .fetch_optional(&mut **conn)
            .await?
        {
            Some(raw) => {
                let user_id = UserId::new(raw.user_id as u64);
                let user = crate::User::load(user_id).await?;
                Ok(Some(Reminder::from_raw(raw, &user.timezone)))
            }
            None => Ok(None),
        }
    }
}

mod set {
    use super::{chrono, ChannelId, GuildId, RawReminder, Reminder, Result, Tz, UserId, Uuid};

    pub async fn full(
        reminder: &Reminder,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<()> {
        let Reminder {
            id,
            user_id,
            channel_id,
            guild_id,
            message,
            remind_at,
            created_at,
            updated_at,
            send_attempt_count,
        } = reminder;
        sqlx::query!(
            "INSERT INTO reminders \
            (\
                id, \
                user_id, \
                channel_id, \
                guild_id, \
                message, \
                remind_at, \
                created_at, \
                updated_at, \
                send_attempt_count \
            ) VALUES ( \
                $1, \
                $2, \
                $3, \
                $4, \
                $5, \
                $6, \
                $7, \
                $8, \
                $9 \
            ) ON CONFLICT (id) DO \
            UPDATE SET \
                user_id = $2, \
                channel_id = $3, \
                guild_id = $4, \
                message = $5, \
                remind_at = $6, \
                created_at = $7, \
                updated_at = $8, \
                send_attempt_count = $9",
            id,
            user_id.get() as i64,
            channel_id.map(|id| id.get() as i64),
            guild_id.map(|id| id.get() as i64),
            message,
            remind_at.naive_utc(),
            created_at.naive_utc(),
            updated_at.map(|t| t.naive_utc()),
            send_attempt_count,
        )
        .execute(&mut **conn)
        .await?;
        Ok(())
    }

    pub async fn new(
        user_id: UserId,
        channel_id: Option<ChannelId>,
        guild_id: Option<GuildId>,
        message: &str,
        remind_at: chrono::DateTime<Tz>,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<RawReminder> {
        Ok(sqlx::query_as!(
            RawReminder,
            "INSERT INTO reminders (user_id, channel_id, guild_id, message, remind_at) VALUES ($1, $2, $3, $4, $5) RETURNING *",
            user_id.get() as i64,
            channel_id.map(|id| id.get() as i64),
            guild_id.map(|id| id.get() as i64),
            message,
            remind_at.to_utc().naive_utc()
        )
        .fetch_one(&mut **conn)
        .await?)
    }

    pub async fn delete(id: Uuid, conn: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Result<()> {
        sqlx::query!("DELETE FROM reminders WHERE id = $1", id)
            .execute(&mut **conn)
            .await?;
        Ok(())
    }
}
