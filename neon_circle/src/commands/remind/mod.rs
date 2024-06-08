use anyhow::Result;
use common::chrono::{Datelike as _, Timelike as _};
use common::serenity::all::*;
use common::{chrono, log, SubCommandTrait};
use long_term_storage::Reminder;
pub struct Command {
    subcommands: Vec<Box<dyn SubCommandTrait>>,
}
impl Command {
    pub fn new() -> Self {
        Self {
            subcommands: vec![Box::new(Add), Box::new(List)],
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
}

struct Add;
#[async_trait]
impl SubCommandTrait for Add {
    fn register_command(&self) -> CreateCommandOption {
        let now = chrono::Utc::now();
        CreateCommandOption::new(
            CommandOptionType::SubCommand,
            self.command_name(),
            "Add a reminder (don't forget to include at least one date/time specifier)",
        )
        .set_sub_options(vec![
            CreateCommandOption::new(
                CommandOptionType::String,
                "message",
                "What to remind you of",
            )
            .required(true),
            // year
            CreateCommandOption::new(
                CommandOptionType::Integer,
                "year",
                "Year to remind you, defaults to current year",
            )
            .required(false)
            .min_int_value((now.year() - 1) as u64),
            // month
            CreateCommandOption::new(
                CommandOptionType::Integer,
                "month",
                "Month to remind you, defaults to current month",
            )
            .required(false)
            .min_int_value(1)
            .max_int_value(12),
            // day
            CreateCommandOption::new(
                CommandOptionType::Integer,
                "day",
                "Day to remind you, defaults to current day",
            )
            .required(false)
            .min_int_value(1)
            .max_int_value(31),
            // hour
            CreateCommandOption::new(
                CommandOptionType::Integer,
                "hour",
                "Hour to remind you (24 hour time, 17 is 5pm), defaults to current hour",
            )
            .required(false)
            .min_int_value(0)
            .max_int_value(23),
            // minute
            CreateCommandOption::new(
                CommandOptionType::Integer,
                "minute",
                "Minute to remind you (give or take a minute), defaults to current minute",
            )
            .required(false)
            .min_int_value(0)
            .max_int_value(59),
        ])
    }
    async fn run(
        &self,
        ctx: &Context,
        interaction: &CommandInteraction,
        options: &[ResolvedOption],
    ) -> Result<()> {
        let message = options
            .iter()
            .find_map(|o| match o.name {
                "message" => match &o.value {
                    ResolvedValue::String(s) => Some(s),
                    _ => {
                        log::error!("Invalid message type");
                        None
                    }
                },
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("No message provided"))?;

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

        let now = chrono::Utc::now().with_timezone(&user.timezone);
        let mut time = now;
        if let Some(year) = options.iter().find_map(|o| match o.name {
            "year" => match &o.value {
                ResolvedValue::Integer(i) => Some(*i as i32),
                _ => {
                    log::error!("Invalid year type");
                    None
                }
            },
            _ => None,
        }) {
            time = time
                .with_year(year)
                .ok_or_else(|| anyhow::anyhow!("Invalid year"))?;
        }
        if let Some(month) = options.iter().find_map(|o| match o.name {
            "month" => match &o.value {
                ResolvedValue::Integer(i) => Some(*i as u32),
                _ => {
                    log::error!("Invalid month type");
                    None
                }
            },
            _ => None,
        }) {
            time = time
                .with_month(month)
                .ok_or_else(|| anyhow::anyhow!("Invalid month"))?;
        }
        if let Some(day) = options.iter().find_map(|o| match o.name {
            "day" => match &o.value {
                ResolvedValue::Integer(i) => Some(*i as u32),
                _ => {
                    log::error!("Invalid day type");
                    None
                }
            },
            _ => None,
        }) {
            time = time
                .with_day(day)
                .ok_or_else(|| anyhow::anyhow!("Invalid day"))?;
        }
        if let Some(hour) = options.iter().find_map(|o| match o.name {
            "hour" => match &o.value {
                ResolvedValue::Integer(i) => Some(*i as u32),
                _ => {
                    log::error!("Invalid hour type");
                    None
                }
            },
            _ => None,
        }) {
            time = time
                .with_hour(hour)
                .ok_or_else(|| anyhow::anyhow!("Invalid hour"))?;
        }
        if let Some(minute) = options.iter().find_map(|o| match o.name {
            "minute" => match &o.value {
                ResolvedValue::Integer(i) => Some(*i as u32),
                _ => {
                    log::error!("Invalid minute type");
                    None
                }
            },
            _ => None,
        }) {
            time = time
                .with_minute(minute)
                .ok_or_else(|| anyhow::anyhow!("Invalid minute"))?;
        }
        if time < now {
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
            message,
            time,
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
                        "Reminder set for <t:{}:F> with timezone `{}`",
                        reminder.remind_at.timestamp(),
                        user.timezone,
                    ))
                    .button(
                        CreateButton::new(ReminderCustomId::TimeWrong)
                            .style(ButtonStyle::Primary)
                            .label("Time wrong?"),
                    )
                    .button(
                        CreateButton::new(ReminderCustomId::NudgeForward(
                            reminder.id().to_string(),
                        ))
                        .style(ButtonStyle::Primary)
                        .label("Nudge forward"),
                    )
                    .button(
                        CreateButton::new(ReminderCustomId::NudgeBackward(
                            reminder.id().to_string(),
                        ))
                        .style(ButtonStyle::Primary)
                        .label("Nudge backward"),
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
            Ok(Some((menu, button1, button2))) => CreateInteractionResponseFollowup::new()
                .ephemeral(true)
                .content(String::new())
                .select_menu(menu)
                .button(button1)
                .button(button2),
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

pub async fn list_reminders(
    user_id: UserId,
    page: i64,
) -> Result<Option<(CreateSelectMenu, CreateButton, CreateButton)>> {
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

    Ok(Some((select_menu, previous_button, next_button)))
}

fn select_menu_element(
    reminder: &Reminder,
    now: chrono::DateTime<chrono::Utc>,
) -> CreateSelectMenuOption {
    let mut label = format!(
        "{} | {}",
        {
            let mut v = match (
                reminder.remind_at.year() == now.year(),
                reminder.remind_at.month() == now.month(),
                reminder.remind_at.day() == now.day(),
            ) {
                (false, _, _) => reminder.remind_at.format("%Y %m %dth").to_string(),
                (true, false, _) => reminder.remind_at.format("%m %dth").to_string(),
                (true, true, false) => reminder
                    .remind_at
                    .format("on the %dth of this month")
                    .to_string(),
                (true, true, true) => "Today".to_string(),
            };
            match reminder.remind_at.day() {
                1 | 21 | 31 => v = v.replace("1th", "1st"),
                2 | 22 => v = v.replace("2th", "2nd"),
                3 | 23 => v = v.replace("3th", "3rd"),
                _ => {} // do nothing
            }
            v
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
