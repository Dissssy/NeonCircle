use crate::audio::{AudioCommandHandler, AudioPromiseCommand, GenericInteraction};
use anyhow::Result;
use serenity::all::*;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{oneshot, RwLock};
lazy_static::lazy_static!(
    static ref VOICE_DATA: RwLock<Option<VoiceData>> = RwLock::new(None);
);
pub async fn initialize_planet(planet: Context) -> Result<()> {
    let mut data = VOICE_DATA.write().await;
    if data.is_some() {
        return Err(anyhow::anyhow!("Voice data already initialized"));
    }
    *data = Some(VoiceData::new(planet));
    Ok(())
}
pub async fn lazy_refresh_guild(guild_id: GuildId) -> Result<(GuildId, GuildVc)> {
    let data = VOICE_DATA.read().await;
    match data.as_ref() {
        Some(data) => data.lazy_refresh_guild(guild_id).await,
        None => Err(anyhow::anyhow!("Voice data uninitialized")),
    }
}
pub async fn insert_guild(guild_id: GuildId, voice_data: GuildVc) -> Result<()> {
    let mut data = VOICE_DATA.write().await;
    match data.as_mut() {
        Some(data) => {
            data.write_guild(guild_id, voice_data);
            Ok(())
        }
        None => Err(anyhow::anyhow!("Voice data uninitialized")),
    }
}
pub async fn refresh_guild(guild_id: GuildId) -> Result<()> {
    let mut data = VOICE_DATA.write().await;
    match data.as_mut() {
        Some(data) => {
            let (_, new) = data.lazy_refresh_guild(guild_id).await?;
            data.write_guild(guild_id, new);
            Ok(())
        }
        None => Err(anyhow::anyhow!("Voice data uninitialized")),
    }
}
pub async fn add_satellite_wait(satellite: Context, position: usize) {
    loop {
        if add_satellite(satellite.clone(), position).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
pub async fn add_satellite(satellite: Context, position: usize) -> Result<()> {
    let mut data = VOICE_DATA.write().await;
    match data.as_mut() {
        Some(data) => {
            let id = satellite.cache.current_user().id;
            data.bot_ids.push((position, id, satellite));
            data.bot_ids.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(())
        }
        None => Err(anyhow::anyhow!("Voice data uninitialized")),
    }
}
pub async fn update_voice(old: Option<VoiceState>, new: VoiceState) -> Result<()> {
    let mut data = VOICE_DATA.write().await;
    match data.as_mut() {
        Some(data) => {
            data.update(old, new);
            Ok(())
        }
        None => Err(anyhow::anyhow!("Voice data uninitialized")),
    }
}
pub async fn mutual_channel(guild: &GuildId, member: &UserId) -> Result<VoiceActionWithContext> {
    let mut data = VOICE_DATA.write().await;
    match data.as_mut() {
        Some(data) => Ok(data.mutual_channel(guild, member).await),
        None => Err(anyhow::anyhow!("Voice data uninitialized")),
    }
}
pub async fn bot_connected(guild: &GuildId, bot: &UserId) -> Result<bool> {
    let data = VOICE_DATA.read().await;
    match data.as_ref() {
        Some(data) => Ok(data
            .guilds
            .get(guild)
            .map_or(false, |guild| guild.bots_connected.contains(bot))),
        None => Err(anyhow::anyhow!("Voice data uninitialized")),
    }
}
pub async fn channel_count_besides(
    guild: &GuildId,
    channel: &ChannelId,
    ignore: &UserId,
) -> Result<UserCount> {
    let data = VOICE_DATA.read().await;
    match data.as_ref() {
        Some(data) => Ok(data
            .user_count_besides(guild, channel, ignore)
            .ok_or(anyhow::anyhow!("Channel not found"))?),
        None => Err(anyhow::anyhow!("Voice data uninitialized")),
    }
}
struct VoiceData {
    guilds: HashMap<GuildId, GuildVc>,
    planet_context: Context,
    bot_ids: Vec<(usize, UserId, Context)>,
}
impl VoiceData {
    fn new(planet: Context) -> Self {
        // let id = planet.cache.current_user().id;
        Self {
            guilds: HashMap::new(),
            planet_context: planet,
            bot_ids: Vec::new(),
        }
    }
    fn update(&mut self, old: Option<VoiceState>, new: VoiceState) {
        if let Some(guild_id) = new.guild_id {
            let guild = self.guilds.entry(guild_id).or_default();
            guild.update(old, new.clone());
            if self.bot_ids.iter().any(|(_, id, _)| *id == new.user_id) {
                if new.channel_id.is_some() {
                    guild.bots_connected.insert(new.user_id);
                } else {
                    guild.bots_connected.remove(&new.user_id);
                }
            }
        }
    }
    fn user_count_besides(
        &self,
        guild: &GuildId,
        channel: &ChannelId,
        ignore: &UserId,
    ) -> Option<UserCount> {
        let guild = self.guilds.get(guild)?;
        guild.user_count_besides(
            channel,
            &self
                .bot_ids
                .iter()
                .filter_map(|(_, id, _)| (id != ignore).then_some(*id))
                .collect::<Vec<_>>(),
        )
    }
    async fn mutual_channel(&mut self, guild: &GuildId, member: &UserId) -> VoiceActionWithContext {
        let guildstate = self.guilds.entry(*guild).or_default();
        let memberstate = match guildstate.find_user(*member) {
            Some(channel) => channel,
            None => {
                return VoiceActionWithContext {
                    planet_ctx: self.planet_context.clone(),
                    action: VoiceAction::UserNotConnected,
                }
            }
        };
        if let Some(bot) = guildstate.first_in(memberstate, self.bot_ids.as_slice()) {
            return VoiceActionWithContext {
                planet_ctx: self.planet_context.clone(),
                action: VoiceAction::SatelliteInVcWithUser(memberstate, bot.clone()),
            };
        }
        let mut invite_bot = None;
        let mut join_bot = None;
        for (_, satellite, context) in self.bot_ids.iter() {
            // check if the satellite can see the channel (if not it might not be in the guild! it should be invited!)
            if let Err(e) = context.http.get_channel(memberstate).await {
                log::trace!("Failed to get channel: {:?}", e); // this is normal behavior, so trace
                if invite_bot.is_none() {
                    invite_bot = Some(context.clone());
                }
                continue;
            }
            match guildstate.find_user(*satellite) {
                Some(channel) => {
                    if channel == memberstate {
                        return VoiceActionWithContext {
                            planet_ctx: self.planet_context.clone(),
                            action: VoiceAction::SatelliteInVcWithUser(channel, context.clone()),
                        };
                    }
                }
                None => {
                    // return VoiceActionWithContext {
                    //     planet_ctx: self.planet_context.clone(),
                    //     action: VoiceAction::SatelliteShouldJoin(memberstate, context.clone()),
                    // };
                    if join_bot.is_none() {
                        join_bot = Some(context.clone());
                    }
                }
            }
        }
        if let Some(joinbot) = join_bot {
            return VoiceActionWithContext {
                planet_ctx: self.planet_context.clone(),
                action: VoiceAction::SatelliteShouldJoin(memberstate, joinbot),
            };
        }
        match invite_bot {
            Some(context) => VoiceActionWithContext {
                planet_ctx: self.planet_context.clone(),
                action: VoiceAction::InviteSatellite(format!("https://discord.com/api/oauth2/authorize?client_id={}&permissions=3145728&scope=bot", context.cache.current_user().id.get())),
            },
            None => VoiceActionWithContext {
                planet_ctx: self.planet_context.clone(),
                action: VoiceAction::NoRemaining,
            },
        }
        // let botstate = guildstate.find_user(bot_id);
        // match (botstate, memberstate) {
        //     (Some(botstate), Some(memberstate)) => {
        //         if botstate == memberstate {
        //             VoiceAction::InSame(botstate)
        //         } else {
        //             VoiceAction::InDifferent(botstate)
        //         }
        //     }
        //     (None, Some(memberstate)) => VoiceAction::Join(memberstate),
        //     _ => VoiceAction::UserNotConnected,
        // }
        // todo!("get whichever bot either shares a channel with the user or has access to the channel the user is in");
    }
    // async fn refresh_guild(&mut self, guild_id: GuildId) -> Result<()> {
    //     let (_, new) = self.lazy_refresh_guild(guild_id).await?;
    //     self.guilds.insert(guild_id, new);
    //     Ok(())
    // }
    async fn lazy_refresh_guild(&self, guild_id: GuildId) -> Result<(GuildId, GuildVc)> {
        let mut new = GuildVc::new();
        for (_i, channel) in self
            .planet_context
            .http
            .get_guild(guild_id)
            .await?
            .channels(&self.planet_context.http)
            .await?
        {
            if channel.kind == ChannelType::Voice {
                let mut newchannel = HashMap::new();
                for member in match self.planet_context.http.get_channel(channel.id).await {
                    Ok(Channel::Guild(channel)) => channel,
                    Err(_) => {
                        continue;
                    }
                    _ => return Err(anyhow::anyhow!("Expected Guild channel")),
                }
                .members(&self.planet_context.cache)?
                {
                    newchannel.insert(
                        member.user.id,
                        UserMetadata {
                            member: member.clone(),
                            last_known_state: None,
                        },
                    );
                }
                new.replace_channel(channel.id, newchannel);
            }
        }
        Ok((guild_id, new))
    }
    fn write_guild(&mut self, guild_id: GuildId, voice_data: GuildVc) {
        self.guilds.insert(guild_id, voice_data);
    }
    // fn bot_alone(&mut self, guild: &GuildId) -> bool {
    //     let guild = self.guilds.entry(*guild).or_insert_with(GuildVc::new);
    //     let channel = match guild.find_user(self.bot_id) {
    //         Some(channel) => channel,
    //         None => return false,
    //     };
    //     let channel = guild.channels.entry(channel).or_default();
    //     for user in channel.values() {
    //         if !user.member.user.bot {
    //             return false;
    //         }
    //     }
    //     true
    // }
}
pub struct VoiceActionWithContext {
    planet_ctx: Context,
    pub action: VoiceAction,
}
pub enum VoiceAction {
    SatelliteShouldJoin(ChannelId, Context),
    SatelliteInVcWithUser(ChannelId, Context),
    InviteSatellite(String),
    NoRemaining,
    UserNotConnected,
}
impl<'a> VoiceActionWithContext {
    pub async fn send_command_or_respond(
        self,
        interaction: impl Into<GenericInteraction<'a>>,
        _guild_id: GuildId,
        command: AudioPromiseCommand,
    ) {
        let interaction = interaction.into();
        match self.action {
            VoiceAction::UserNotConnected => {
                if let Err(e) = interaction
                    .edit_response(
                        &self.planet_ctx.http,
                        EditInteractionResponse::new().content("You're not in a voice channel"),
                    )
                    .await
                {
                    log::error!("Failed to edit original interaction response: {:?}", e);
                }
            }
            VoiceAction::InviteSatellite(invite) => {
                if let Err(e) = interaction
                    .edit_response(
                        &self.planet_ctx.http,
                        EditInteractionResponse::new().content(format!("There are no satellites available, [use this link to invite one]({})\nPlease ensure that all satellites have permission to view the voice channel you're in.", invite)),
                    )
                    .await
                {
                    log::error!("Failed to edit original interaction response: {:?}", e);
                }
            }
            VoiceAction::NoRemaining => {
                if let Err(e) = interaction
                    .edit_response(
                        &self.planet_ctx.http,
                        EditInteractionResponse::new().content("No satellites available to join, use /feedback to request more (and dont forget to donate if you can! :D)"),
                    )
                    .await
                {
                    log::error!("Failed to edit original interaction response: {:?}", e);
                }
            }
            VoiceAction::SatelliteShouldJoin(_channel, _ctx) => {
                if let Err(e) = interaction
                    .edit_response(
                        &self.planet_ctx.http,
                        EditInteractionResponse::new().content(
                            "There's no bot in your channel, use /add or /join to summon one",
                        ),
                    )
                    .await
                {
                    log::error!("Failed to edit original interaction response: {:?}", e);
                }
            }
            VoiceAction::SatelliteInVcWithUser(channel, _ctx) => {
                let audio_command_handler = match self
                    .planet_ctx
                    .data
                    .read()
                    .await
                    .get::<AudioCommandHandler>()
                {
                    Some(handler) => Arc::clone(handler),
                    None => {
                        if let Err(e) = interaction
                            .edit_response(
                                &self.planet_ctx.http,
                                EditInteractionResponse::new().content(
                                    "Failed to get audio handler, this shouldn't be possible",
                                ),
                            )
                            .await
                        {
                            log::error!("Failed to edit original interaction response: {:?}", e);
                        }
                        return;
                    }
                };
                let mut audio_command_handler = audio_command_handler.write().await;
                if let Some(tx) = audio_command_handler.get_mut(&channel) {
                    let (rtx, rrx) = oneshot::channel::<Arc<str>>();
                    if let Err(e) = tx.send((rtx, command)) {
                        log::error!("Failed to send volume change: {:?}", e);
                        if let Err(e) = interaction
                            .edit_response(
                                &self.planet_ctx.http,
                                EditInteractionResponse::new()
                                    .content("Failed to send volume change"),
                            )
                            .await
                        {
                            log::error!("Failed to edit original interaction response: {:?}", e);
                        }
                        return;
                    }
                    let timeout = tokio::time::timeout(Duration::from_secs(10), rrx).await;
                    if let Ok(Ok(msg)) = timeout {
                        if let Err(e) = interaction
                            .edit_response(
                                &self.planet_ctx.http,
                                EditInteractionResponse::new().content(msg.as_ref()),
                            )
                            .await
                        {
                            log::error!("Failed to edit original interaction response: {:?}", e);
                        }
                    } else if let Err(e) = interaction
                        .edit_response(
                            &self.planet_ctx.http,
                            EditInteractionResponse::new().content("Failed to send inner command"),
                        )
                        .await
                    {
                        log::error!("Failed to edit original interaction response: {:?}", e);
                    }
                } else if let Err(e) = interaction
                    .edit_response(
                        &self.planet_ctx.http,
                        EditInteractionResponse::new()
                            .content("Couldnt find the channel handler :( im broken."),
                    )
                    .await
                {
                    log::error!("Failed to edit original interaction response: {:?}", e);
                }
            }
        }
    }
}
#[derive(Debug, Clone)]
pub struct GuildVc {
    pub channels: HashMap<ChannelId, HashMap<UserId, UserMetadata>>,
    pub bots_connected: HashSet<UserId>,
}
impl GuildVc {
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
            bots_connected: HashSet::new(),
        }
    }
    fn user_count_besides(&self, channel: &ChannelId, our_bots: &[UserId]) -> Option<UserCount> {
        if let Some(channel) = self.channels.get(channel) {
            let mut users = 0;
            let mut bots = 0;
            for (id, user) in channel {
                if !user.member.user.bot {
                    if our_bots.contains(id) {
                        bots += 1;
                    } else {
                        users += 1;
                    }
                }
            }
            Some(UserCount { users, bots })
        } else {
            None
        }
    }
    pub fn update(&mut self, old: Option<VoiceState>, new: VoiceState) {
        if let Some(old) = old {
            if old.channel_id != new.channel_id {
                if let Some(channel) = old.channel_id {
                    let channel = self.channels.entry(channel).or_default();
                    channel.remove(&old.user_id);
                }
            }
        }
        if let (Some(channel), Some(member)) = (new.channel_id, new.member.clone()) {
            let channel = self.channels.entry(channel).or_default();
            channel.insert(new.user_id, UserMetadata::new(member, new));
        }
    }
    pub fn replace_channel(&mut self, id: ChannelId, members: HashMap<UserId, UserMetadata>) {
        self.channels.insert(id, members);
    }
    pub fn find_user(&self, user: UserId) -> Option<ChannelId> {
        for (channel, members) in self.channels.iter() {
            if members.contains_key(&user) {
                return Some(*channel);
            }
        }
        None
    }
    fn first_in(
        &self,
        memberstate: ChannelId,
        as_slice: &[(usize, UserId, Context)],
    ) -> Option<Context> {
        for (_, id, context) in as_slice {
            if self.bots_connected.contains(id) {
                continue;
            }
            if let Some(channel) = self.find_user(*id) {
                if channel == memberstate {
                    return Some(context.clone());
                }
            }
        }
        None
    }
}
impl Default for GuildVc {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Debug, Clone)]
pub struct UserMetadata {
    pub member: Member,
    #[allow(dead_code)]
    pub last_known_state: Option<VoiceState>,
}
impl UserMetadata {
    pub fn new(member: Member, state: VoiceState) -> Self {
        Self {
            member,
            last_known_state: Some(state),
        }
    }
}
pub struct UserCount {
    pub users: usize,
    pub bots: usize,
}
