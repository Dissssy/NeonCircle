use anyhow::Result;
use common::chrono::{Datelike as _, Timelike as _};
use common::serenity::all::*;
use common::{chrono, chrono_tz, log, SubCommandTrait};
use fuzzy_matcher::FuzzyMatcher as _;
use long_term_storage::Reminder;
pub struct Command {
    subcommands: Vec<Box<dyn SubCommandTrait>>,
}
impl Command {
    pub fn new() -> Self {
        Self {
            subcommands: vec![Box::new(Me), Box::new(List), Box::new(Timezone)],
        }
    }
}
#[async_trait]
impl crate::traits::CommandTrait for Command {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .description("Manage your reminders")
                .set_options(
                    self.subcommands
                        .iter()
                        .map(|sc| sc.register_command())
                        .collect(),
                ),
        )
    }
    async fn run(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        if let Err(e) = interaction.defer_ephemeral(&ctx.http).await {
            log::error!("Failed to send response: {}", e);
        }
        let (subcommand, opts) = match interaction.data.options().into_iter().find_map(|o| match o
            .value
        {
            ResolvedValue::SubCommand(opts) => Some((o.name, opts)),
            ResolvedValue::SubCommandGroup(opts) => Some((o.name, opts)),
            _ => None,
        }) {
            None => {
                unreachable!();
            }
            Some(s) => s,
        };
        for sc in &self.subcommands {
            if sc.command_name() == subcommand {
                return sc.run(ctx, interaction, &opts).await;
            }
        }
        if let Err(e) = interaction
            .create_followup(
                &ctx.http,
                CreateInteractionResponseFollowup::new()
                    .ephemeral(true)
                    .content("Invalid subcommand"),
            )
            .await
        {
            log::error!("Failed to send response: {}", e);
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "remind"
    }
    async fn autocomplete(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        for option in interaction.data.options() {
            for sc in &self.subcommands {
                if sc.command_name() == option.name {
                    match option.value {
                        ResolvedValue::SubCommand(opts) | ResolvedValue::SubCommandGroup(opts) => {
                            return sc.autocomplete(ctx, interaction, &opts).await;
                        }
                        _ => {
                            return Err(anyhow::anyhow!("Invalid option type"));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

struct Me;
#[async_trait]
impl SubCommandTrait for Me {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "Add a reminder (don't forget to include at least one date/time specifier)",
        )
        .set_sub_options(vec![
            CreateCommandOption::new(CommandOptionType::String, "about", "What to remind you of")
                .required(true),
            CreateCommandOption::new(CommandOptionType::String, "at", "When to remind you")
                .required(true)
                .set_autocomplete(true),
            // // year
            // CreateCommandOption::new(
            //     CommandOptionType::Integer,
            //     "year",
            //     "Year to remind you, defaults to current year",
            // )
            // .required(false)
            // .min_int_value(2024), // dont forget to update this!
            // // month
            // CreateCommandOption::new(
            //     CommandOptionType::Integer,
            //     "month",
            //     "Month to remind you, defaults to current month",
            // )
            // .required(false)
            // .min_int_value(1)
            // .max_int_value(12),
            // // day
            // CreateCommandOption::new(
            //     CommandOptionType::Integer,
            //     "day",
            //     "Day to remind you, defaults to current day",
            // )
            // .required(false)
            // .min_int_value(1)
            // .max_int_value(31),
            // // hour
            // CreateCommandOption::new(
            //     CommandOptionType::Integer,
            //     "hour",
            //     "Hour to remind you (24 hour time, 17 is 5pm), defaults to current hour",
            // )
            // .required(false)
            // .min_int_value(0)
            // .max_int_value(23),
            // // minute
            // CreateCommandOption::new(
            //     CommandOptionType::Integer,
            //     "minute",
            //     "Minute to remind you (give or take a minute), defaults to current minute",
            // )
            // .required(false)
            // .min_int_value(0)
            // .max_int_value(59),
        ])
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        // let message = options
        //     .iter()
        //     .find_map(|o| match o.name {
        //         "message" => match &o.value {
        //             ResolvedValue::String(s) => Some(s),
        //             _ => {
        //                 log::error!("Invalid message type");
        //                 None
        //             }
        //         },
        //         _ => None,
        //     })
        //     .ok_or_else(|| anyhow::anyhow!("No message provided"))?;
        let about = options
            .iter()
            .find_map(|o| match o.name {
                "about" => match &o.value {
                    ResolvedValue::String(s) => Some(s),
                    _ => {
                        log::error!("Invalid about type");
                        None
                    }
                },
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("No about provided"))?;

        let user = match long_term_storage::User::load(interaction.user.id).await {
            Ok(user) => user,
            Err(e) => {
                log::error!("Failed to load user: {}", e);
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .ephemeral(true)
                            .content("Failed to load user"),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };

        // let now = chrono::Utc::now().with_timezone(&user.timezone);
        // let mut modified = false;
        // let mut time = now;
        // if let Some(year) = options.iter().find_map(|o| match o.name {
        //     "year" => match &o.value {
        //         ResolvedValue::Integer(i) => Some(*i as i32),
        //         _ => {
        //             log::error!("Invalid year type");
        //             None
        //         }
        //     },
        //     _ => None,
        // }) {
        //     time = time
        //         .with_year(year)
        //         .ok_or_else(|| anyhow::anyhow!("Invalid year"))?;
        //     modified = true;
        // }
        // if let Some(month) = options.iter().find_map(|o| match o.name {
        //     "month" => match &o.value {
        //         ResolvedValue::Integer(i) => Some(*i as u32),
        //         _ => {
        //             log::error!("Invalid month type");
        //             None
        //         }
        //     },
        //     _ => None,
        // }) {
        //     time = time
        //         .with_month(month)
        //         .ok_or_else(|| anyhow::anyhow!("Invalid month"))?;
        //     modified = true;
        // }
        // if let Some(day) = options.iter().find_map(|o| match o.name {
        //     "day" => match &o.value {
        //         ResolvedValue::Integer(i) => Some(*i as u32),
        //         _ => {
        //             log::error!("Invalid day type");
        //             None
        //         }
        //     },
        //     _ => None,
        // }) {
        //     time = time
        //         .with_day(day)
        //         .ok_or_else(|| anyhow::anyhow!("Invalid day"))?;
        //     modified = true;
        // }
        // if let Some(hour) = options.iter().find_map(|o| match o.name {
        //     "hour" => match &o.value {
        //         ResolvedValue::Integer(i) => Some(*i as u32),
        //         _ => {
        //             log::error!("Invalid hour type");
        //             None
        //         }
        //     },
        //     _ => None,
        // }) {
        //     time = time
        //         .with_hour(hour)
        //         .ok_or_else(|| anyhow::anyhow!("Invalid hour"))?;
        //     modified = true;
        // }
        // if let Some(minute) = options.iter().find_map(|o| match o.name {
        //     "minute" => match &o.value {
        //         ResolvedValue::Integer(i) => Some(*i as u32),
        //         _ => {
        //             log::error!("Invalid minute type");
        //             None
        //         }
        //     },
        //     _ => None,
        // }) {
        //     time = time
        //         .with_minute(minute)
        //         .ok_or_else(|| anyhow::anyhow!("Invalid minute"))?;
        //     modified = true;
        // }
        // if !modified {
        //     if let Err(e) = interaction
        //         .create_followup(
        //             &ctx.http,
        //             CreateInteractionResponseFollowup::new()
        //                 .ephemeral(true)
        //                 .content("You must include at least one date/time specifier.\n\
        //                 This reminder would have been sent immediately.\n\
        //                 If you need reminders that frequently, you should consider seeing a doctor."),
        //         )
        //         .await
        //     {
        //         log::error!("Failed to send response: {}", e);
        //     }
        //     return Ok(());
        // }

        let at = options
            .iter()
            .find_map(|o| match o.name {
                "at" => match &o.value {
                    ResolvedValue::String(s) => {
                        if *s == "::INVALID" {
                            None
                        } else {
                            match s.parse::<i64>() {
                                Ok(i) => chrono::DateTime::from_timestamp(i, 0)
                                    .map(|dt| dt.with_timezone(&user.timezone)),
                                Err(e) => {
                                    log::error!("Failed to parse timestamp: {}", e);
                                    None
                                }
                            }
                        }
                    }
                    _ => {
                        log::error!("Invalid at type");
                        None
                    }
                },
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("No at provided"))?;

        if at < chrono::Utc::now() {
            if let Err(e) = interaction
                .create_followup(
                    &ctx.http,
                    CreateInteractionResponseFollowup::new()
                        .ephemeral(true)
                        .content("Reminder must be in the future"),
                )
                .await
            {
                log::error!("Failed to send response: {}", e);
            }
            return Ok(());
        }

        let reminder = match long_term_storage::Reminder::new(
            interaction.user.id,
            interaction
                .guild_id
                .is_some()
                .then_some(interaction.channel_id),
            interaction.guild_id,
            about,
            at,
        )
        .await
        {
            Ok(reminder) => reminder,
            Err(e) => {
                log::error!("Failed to create reminder: {}", e);
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .ephemeral(true)
                            .content("Failed to create reminder"),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };

        if let Err(e) = interaction
            .create_followup(
                &ctx.http,
                CreateInteractionResponseFollowup::new()
                    .ephemeral(true)
                    .content(format!(
                        "Reminder set for <t:{}:F> (your local time)\n\
                        With your configured timezone `{}`",
                        reminder.remind_at.timestamp(),
                        user.timezone,
                    ))
                    .button(
                        CreateButton::new(ReminderCustomId::TimeWrong)
                            .style(ButtonStyle::Primary)
                            .label("Time wrong?"),
                    )
                    .button(
                        CreateButton::new(ReminderCustomId::NudgeBackward(
                            reminder.id().to_string(),
                        ))
                        .style(ButtonStyle::Primary)
                        .label("Nudge backward"),
                    )
                    .button(
                        CreateButton::new(ReminderCustomId::NudgeForward(
                            reminder.id().to_string(),
                        ))
                        .style(ButtonStyle::Primary)
                        .label("Nudge forward"),
                    ),
            )
            .await
        {
            log::error!("Failed to send response: {}", e);
        }
        Ok(())
    }
    fn command_name(&self) -> &str {
        "me"
    }
    fn permissions(&self) -> Permissions {
        Permissions::empty()
    }
    async fn autocomplete(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        for option in options {
            if let ResolvedValue::Autocomplete { kind: _, value } = option.value {
                if value.is_empty() {
                    continue;
                }
                if option.name == "at" {
                    match parse_time_and_date(interaction.user.id, value).await {
                        Ok(time) => {
                            if let Err(e) = interaction
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Autocomplete(
                                        CreateAutocompleteResponse::new().add_string_choice(
                                            // time.format("%Y-%m-%d %l:%M:%S %p").to_string(),
                                            common::utils::full_datetime_format(&time, true),
                                            time.to_utc().timestamp().to_string(),
                                        ),
                                    ),
                                )
                                .await
                            {
                                log::error!("Failed to send response: {}", e);
                            }
                        }
                        Err(e) => {
                            if let Err(e) = interaction
                                .create_response(
                                    &ctx.http,
                                    CreateInteractionResponse::Autocomplete(
                                        CreateAutocompleteResponse::new().add_string_choice(
                                            {
                                                let mut err = e.to_string();
                                                // if err is above 100 characters, truncate it and add "..."
                                                if err.len() > 100 {
                                                    log::error!("Error too long: {}", err);
                                                    err.truncate(97);
                                                    err.push_str("...");
                                                }
                                                err
                                            },
                                            "::INVALID",
                                        ),
                                    ),
                                )
                                .await
                            {
                                log::error!("Failed to send response: {}", e);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

async fn parse_time_and_date(
    user_id: UserId,
    message: &str,
) -> Result<chrono::DateTime<chrono_tz::Tz>> {
    let timezone: chrono_tz::Tz = long_term_storage::User::load(user_id).await?.timezone;
    let now = chrono::Utc::now().with_timezone(&timezone);
    let mut now_mut = now;

    let date_specified = if let Some(date) = date_time_parser::DateParser::parse(message) {
        let _: Option<()> = try {
            now_mut = now_mut
                .with_year(date.year())?
                .with_month(date.month())?
                .with_day(date.day())?;
        };
        true
    } else {
        false
    };

    if let Some(time) = date_time_parser::TimeParser::parse(message) {
        let _: Option<()> = try {
            now_mut = now_mut
                .with_hour(time.hour())?
                .with_minute(time.minute())?
                .with_second(time.second())?;
        };
    }

    if now_mut <= now {
        if date_specified {
            return Err(anyhow::anyhow!("Reminder must be in the future"));
        }
        // add one day
        now_mut += chrono::Duration::days(1);
        if now_mut <= now {
            return Err(anyhow::anyhow!("Reminder must be in the future"));
        }
    }

    Ok(now_mut)
}

struct List;
#[async_trait]
impl SubCommandTrait for List {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "List your reminders",
        )
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        _options: &[ResolvedOption],
    ) -> Result<()> {
        let followup = match list_reminders(interaction.user.id, 0).await {
            Ok(Some(elements)) => CreateInteractionResponseFollowup::new()
                .ephemeral(true)
                .content(String::new())
                .select_menu(elements.list)
                .button(elements.backward)
                .button(elements.forward),
            Ok(None) => CreateInteractionResponseFollowup::new()
                .content("You have no reminders.")
                .components(vec![]),
            Err(e) => {
                log::error!("Failed to list reminders: {}", e);
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .content(format!("Failed to list reminders: {}", e))
                            .ephemeral(true),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };
        if let Err(e) = interaction.create_followup(&ctx.http, followup).await {
            log::error!("Failed to send response: {}", e);
        }

        Ok(())
    }
    fn command_name(&self) -> &str {
        "list"
    }
    fn permissions(&self) -> Permissions {
        Permissions::empty()
    }
}

pub struct ReminderElements {
    pub list: CreateSelectMenu,
    pub backward: CreateButton,
    pub forward: CreateButton,
}

pub async fn list_reminders(user_id: UserId, page: i64) -> Result<Option<ReminderElements>> {
    // will construct a "text" select menu that lists all the reminders as well as whatever contents will fit after we display the date

    // first using our pagination, get 25 reminders, sorted by remind_at such that the earliest reminder is first and offset by page * 25
    // we're gonna query for all of them and do the processing here because i'm too lazy to make a new function for this lol
    let all_reminders = long_term_storage::Reminder::all_reminders_for(user_id, page).await?;
    if all_reminders.reminders.is_empty() {
        return Ok(None);
    }
    let now = chrono::Utc::now();
    let select_menu = CreateSelectMenu::new(
        ReminderCustomId::List,
        CreateSelectMenuKind::String {
            options: all_reminders
                .reminders
                .iter()
                .map(|r| select_menu_element(r, now))
                .collect(),
        },
    )
    .placeholder("Reminders");
    let next_button = CreateButton::new(ReminderCustomId::ToPage(page + 1))
        .style(ButtonStyle::Primary)
        .label("Next page")
        .disabled(!all_reminders.more);

    let previous_button = CreateButton::new(ReminderCustomId::ToPage(page - 1))
        .style(ButtonStyle::Primary)
        .label("Previous page")
        .disabled(page == 0);

    Ok(Some(ReminderElements {
        forward: next_button,
        backward: previous_button,
        list: select_menu,
    }))
}

fn select_menu_element(
    reminder: &Reminder,
    now: chrono::DateTime<chrono::Utc>,
) -> CreateSelectMenuOption {
    let mut label = format!(
        "{} | {}",
        {
            match (
                reminder.remind_at.year() == now.year(),
                reminder.remind_at.month() == now.month(),
                reminder.remind_at.day() == now.day(),
            ) {
                (false, _, _) => reminder.remind_at.format("%B %eth %Y").to_string(),
                (true, false, _) => reminder.remind_at.format("%B %eth").to_string(),
                (true, true, false) => reminder
                    .remind_at
                    .format("On the %eth at %l:%M %p")
                    .to_string(),
                (true, true, true) => reminder.remind_at.format("at %l:%M %p").to_string(),
            }
            .replace("1th", "1st") // this will catch all cases because it will match the {1th} as well as the 2{1th}
            .replace("2th", "2nd")
            .replace("3th", "3rd")
        },
        reminder.message.replace('\n', " "),
    );
    if label.len() > 100 {
        label.truncate(97);
        label.push_str("...");
    }
    CreateSelectMenuOption::new(label, ReminderCustomId::Detail(reminder.id().to_string()))
}
pub enum ReminderCustomId {
    TimeWrong,
    List,
    Return,
    NudgeForward(String),
    NudgeBackward(String),
    Delete(String),
    ToPage(i64),
    Detail(String),
}

impl ReminderCustomId {
    pub fn is_list(&self) -> bool {
        matches!(self, Self::List)
    }
}

impl From<ReminderCustomId> for String {
    fn from(id: ReminderCustomId) -> Self {
        match id {
            ReminderCustomId::TimeWrong => "reminder:time_wrong".to_string(),
            ReminderCustomId::List => "reminder:list".to_string(),
            ReminderCustomId::Return => "reminder:return".to_string(),
            ReminderCustomId::NudgeForward(id) => format!("reminder:nudge_forward:{}", id),
            ReminderCustomId::NudgeBackward(id) => format!("reminder:nudge_backward:{}", id),
            ReminderCustomId::Delete(id) => format!("reminder:delete:{}", id),
            ReminderCustomId::ToPage(page) => format!("reminder:page:{}", page),
            ReminderCustomId::Detail(id) => format!("reminder:detail:{}", id),
        }
    }
}

impl TryFrom<&str> for ReminderCustomId {
    type Error = anyhow::Error;

    fn try_from(id: &str) -> Result<Self> {
        let mut parts = id.split(':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("reminder"), Some("time_wrong"), None) => Ok(ReminderCustomId::TimeWrong),
            (Some("reminder"), Some("list"), None) => Ok(ReminderCustomId::List),
            (Some("reminder"), Some("return"), None) => Ok(ReminderCustomId::Return),
            (Some("reminder"), Some("nudge_forward"), Some(id)) => {
                Ok(ReminderCustomId::NudgeForward(id.to_string()))
            }
            (Some("reminder"), Some("nudge_backward"), Some(id)) => {
                Ok(ReminderCustomId::NudgeBackward(id.to_string()))
            }
            (Some("reminder"), Some("delete"), Some(id)) => {
                Ok(ReminderCustomId::Delete(id.to_string()))
            }
            (Some("reminder"), Some("page"), Some(page)) => {
                Ok(ReminderCustomId::ToPage(page.parse()?))
            }
            (Some("reminder"), Some("detail"), Some(id)) => {
                Ok(ReminderCustomId::Detail(id.to_string()))
            }
            _ => Err(anyhow::anyhow!("Invalid custom id")),
        }
    }
}

struct Timezone;
#[async_trait]
impl SubCommandTrait for Timezone {
    fn register_command(&self) -> CreateCommandOption {
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "Set your timezone",
        )
        .set_sub_options(vec![CreateCommandOption::new(
            CommandOptionType::String,
            "timezone",
            "Your timezone",
        )
        .required(true)
        .set_autocomplete(true)])
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        let timezone = options
            .iter()
            .find_map(|o| match o.name {
                "timezone" => match &o.value {
                    ResolvedValue::String(s) => Some(s),
                    _ => {
                        log::error!("Invalid timezone type");
                        None
                    }
                },
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("No timezone provided"))?;

        let timezone = match timezone.parse::<chrono_tz::Tz>() {
            Ok(tz) => tz,
            Err(e) => {
                log::error!("Failed to parse timezone: {}", e);
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .ephemeral(true)
                            .content("Failed to parse timezone"),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };

        let user = match long_term_storage::User::load(interaction.user.id).await {
            Ok(mut user) => {
                user.timezone = timezone;
                user
            }
            Err(e) => {
                log::error!("Failed to load user: {}", e);
                if let Err(e) = interaction
                    .create_followup(
                        &ctx.http,
                        CreateInteractionResponseFollowup::new()
                            .ephemeral(true)
                            .content("Failed to load user"),
                    )
                    .await
                {
                    log::error!("Failed to send response: {}", e);
                }
                return Ok(());
            }
        };

        if let Err(e) = user.save().await {
            log::error!("Failed to save user: {}", e);
            if let Err(e) = interaction
                .create_followup(
                    &ctx.http,
                    CreateInteractionResponseFollowup::new()
                        .ephemeral(true)
                        .content("Failed to save user"),
                )
                .await
            {
                log::error!("Failed to send response: {}", e);
            }
            return Ok(());
        }

        if let Err(e) = interaction
            .create_followup(
                &ctx.http,
                CreateInteractionResponseFollowup::new()
                    .ephemeral(true)
                    .content(format!(
                        "Timezone set to `{}`\n\
                        Your time: <t:{}:F>\n\
                        Timezone time: `{}`",
                        timezone,
                        chrono::Utc::now().with_timezone(&timezone).timestamp(),
                        common::utils::full_datetime_format(
                            &chrono::Utc::now().with_timezone(&timezone),
                            false,
                        ),
                    )),
            )
            .await
        {
            log::error!("Failed to send response: {}", e);
        }

        Ok(())
    }
    fn command_name(&self) -> &str {
        "timezone"
    }
    fn permissions(&self) -> Permissions {
        Permissions::empty()
    }
    async fn autocomplete(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        for option in options {
            if let ResolvedValue::Autocomplete { kind: _, value } = option.value {
                if value.is_empty() {
                    continue;
                }
                if option.name == "timezone" {
                    let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
                    let mut opts: Vec<(chrono_tz::Tz, i64)> = chrono_tz::TZ_VARIANTS
                        .iter()
                        .filter_map(|tz| {
                            matcher
                                .fuzzy_match(tz.name(), value)
                                .map(|score| (*tz, score))
                        })
                        .collect::<Vec<_>>();
                    opts.sort_by(|a, b| b.1.cmp(&a.1));
                    // only keep the top 25
                    opts.truncate(25);
                    if let Err(e) = interaction
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::Autocomplete({
                                let mut resp = CreateAutocompleteResponse::new();
                                let now = chrono::Utc::now();
                                for (tz, _) in opts {
                                    resp = resp.add_string_choice(
                                        now.with_timezone(&tz)
                                            .format(&format!("{} | %l:%M %p", tz.name()))
                                            .to_string(),
                                        tz.name(),
                                    );
                                }
                                resp
                            }),
                        )
                        .await
                    {
                        log::error!("Failed to send response: {}", e);
                    }
                }
            }
        }
        Ok(())
    }
}
