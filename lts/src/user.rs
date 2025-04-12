// CREATE TABLE IF NOT EXISTS users (
//     -- Discord user ID for querying
//     id BIGINT PRIMARY KEY,
//     -- Whether or not the user consents to their microphone data being processed
//     mic_consent BOOLEAN NOT NULL DEFAULT FALSE
// );

use common::{
    anyhow::{anyhow, Result},
    chrono_tz::Tz,
    log,
    serenity::{all::UserId, futures::StreamExt as _},
};
use std::{cell::OnceCell, collections::HashMap, str::FromStr};

static mut CONSENT_CACHE: OnceCell<ConsentCache> = OnceCell::new();

struct ConsentCache(HashMap<UserId, bool>);

impl ConsentCache {
    fn set_consent(&mut self, user_id: UserId, consent: bool) {
        self.0.insert(user_id, consent);
    }
    fn get_consent(&self, user_id: UserId) -> bool {
        self.0.get(&user_id).copied().unwrap_or(false)
    }
    async fn sync() -> Result<Self> {
        let mut transaction = crate::get_connection().await?;
        let mut cache = HashMap::new();
        let mut stream = sqlx::query_as!(RawUser, "SELECT * FROM users").fetch(&mut *transaction);
        while let Some(user) = stream.next().await {
            match user {
                Err(e) => common::log::error!("Failed to fetch user: {:?}", e),
                Ok(user) => {
                    let _ = cache.insert(UserId::new(user.id as u64), user.mic_consent);
                }
            }
        }
        Ok(ConsentCache(cache))
    }
}

pub(crate) async fn init() {
    let _ = get_cache_mut().await;
}

fn get_cache_ref() -> Result<&'static ConsentCache> {
    unsafe {
        CONSENT_CACHE
            .get()
            .ok_or_else(|| anyhow!("cache not initialized, call `get_cache_mut` first"))
    }
}

async fn get_cache_mut() -> Result<&'static mut ConsentCache> {
    unsafe {
        match CONSENT_CACHE.get_mut() {
            Some(cache) => Ok(cache),
            None => Ok({
                let cache = ConsentCache::sync().await?;
                CONSENT_CACHE.get_mut_or_init(|| cache)
            }),
        }
    }
}

#[derive(Debug)]
pub struct User {
    pub id: UserId,
    pub mic_consent: bool,
    pub voice_preference: VoicePreference,
    pub timezone: Tz,
}

impl User {
    pub async fn load_opt(user_id: UserId) -> Result<Option<Self>> {
        let mut conn = crate::get_connection().await?;
        get::full(user_id, &mut conn).await
    }
    pub async fn load(user_id: UserId) -> Result<Self> {
        let mut conn = crate::get_connection().await?;
        match get::full(user_id, &mut conn).await? {
            Some(user) => Ok(user),
            None => {
                set::default(user_id, &mut conn).await?;
                match get::full(user_id, &mut conn).await? {
                    Some(user) => {
                        conn.commit().await?;
                        Ok(user)
                    }
                    None => Err(anyhow!("Failed to write default user")),
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
    pub fn mic_consent(user_id: UserId) -> Result<bool> {
        get::mic_consent(user_id)
    }
}

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub enum VoicePreference {
    #[default]
    NoPreference,
    Male,
    Female,
}

struct RawUser {
    id: i64,
    mic_consent: bool,
    voice_preference: Option<bool>, // none = no preference, true = female, false = male
    timezone: String,
}

impl From<RawUser> for User {
    fn from(val: RawUser) -> Self {
        User {
            id: UserId::new(val.id as u64),
            mic_consent: val.mic_consent,
            timezone: Tz::from_str(&val.timezone).unwrap_or_else(|_| {
                log::warn!("Failed to parse timezone: {}", val.timezone);
                Tz::EST
            }),
            voice_preference: match val.voice_preference {
                Some(true) => VoicePreference::Female,
                Some(false) => VoicePreference::Male,
                None => VoicePreference::NoPreference,
            },
        }
    }
}

mod get {
    use super::{get_cache_mut, get_cache_ref, RawUser, User};
    use common::{anyhow::Result, serenity::all::UserId};

    pub async fn full(
        user_id: UserId,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Option<User>> {
        // get the user data direct from the DB
        let o: Option<User> = sqlx::query_as!(
            RawUser,
            "SELECT * FROM users WHERE id = $1",
            user_id.get() as i64
        )
        .fetch_optional(&mut **conn)
        .await?
        .map(Into::into);
        if let Some(ref user) = o {
            // update the cache
            let cache = get_cache_mut().await?;
            cache.set_consent(user_id, user.mic_consent);
        }
        Ok(o)
    }

    pub fn mic_consent(user_id: UserId) -> Result<bool> {
        // get specifically only from cache
        let cache = get_cache_ref()?;
        Ok(cache.get_consent(user_id))
    }
}

mod set {
    use common::anyhow::Result;

    pub async fn full(
        user: super::User,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<()> {
        let super::User {
            id,
            mic_consent,
            voice_preference,
            timezone,
        } = user;

        // set the cache
        {
            let cache = super::get_cache_mut().await?;
            cache.set_consent(id, mic_consent);
        }
        // set the user in the DB, either insert or update the user
        sqlx::query!(
            "INSERT INTO users (id, mic_consent, voice_preference, timezone) VALUES ($1, $2, $3, $4) ON CONFLICT (id) DO UPDATE SET mic_consent = $2, voice_preference = $3, timezone = $4",
            id.get() as i64,
            mic_consent,
            match voice_preference {
                super::VoicePreference::NoPreference => None,
                super::VoicePreference::Male => Some(false),
                super::VoicePreference::Female => Some(true),
            },
            timezone.name()
        )
        .execute(&mut **conn)
        .await?;
        Ok(())
    }

    pub async fn default(
        user_id: super::UserId,
        conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<()> {
        // set the user in the DB
        sqlx::query!(
            "INSERT INTO users (id) VALUES ($1) ON CONFLICT (id) DO NOTHING",
            user_id.get() as i64
        )
        .execute(&mut **conn)
        .await?;
        Ok(())
    }
}

// pub(crate) async fn migrate_data_from_json(
//     conn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
// ) -> Result<()> {
//     let data = common::global_data::extract::consent_data();

//     for (user_id, consent) in data {
//         sqlx::query!(
//             "INSERT INTO users (id, mic_consent) VALUES ($1, $2) ON CONFLICT (id) DO UPDATE SET mic_consent = $2",
//             user_id.get() as i64,
//             consent
//         )
//         .execute(&mut **conn)
//         .await?;
//     }
//     Ok(())
// }
