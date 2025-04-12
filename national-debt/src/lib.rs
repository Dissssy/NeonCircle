#![feature(try_blocks)]

const URL: &str = "https://api.fiscaldata.treasury.gov/services/api/fiscal_service/v2/accounting/od/debt_to_penny?fields=tot_pub_debt_out_amt,record_calendar_year,record_calendar_month,record_calendar_day&filter=record_date:lte:<DATEFORMATTEDLIKEBELOW>&sort=-record_calendar_year,-record_calendar_month,-record_calendar_day";
// 2024-06-04

use anyhow::Result;
use common::{
    anyhow,
    chrono::{self},
    reqwest,
    serenity::{all::*, async_trait},
    CommandTrait,
};
use serde::Deserialize;

async fn get_debt() -> Result<NationalDebt> {
    reqwest::get(URL.replace("<DATEFORMATTEDLIKEBELOW>", &chrono::Utc::now().format("%Y-%m-%d").to_string()))
        .await?
        .json::<RawNationalDebt>()
        .await?
        .try_into()
}

pub struct Command;

#[async_trait]
impl CommandTrait for Command {
    fn register_command(&self) -> Option<CreateCommand> {
        Some(
            CreateCommand::new(self.command_name())
                .contexts(vec![InteractionContext::Guild, InteractionContext::BotDm])
                .description("Get the US national debt as of (at most) a few days ago"),
        )
    }
    fn command_name(&self) -> &str {
        "debt"
    }
    async fn run(&self, ctx: &Context, event: &CommandInteraction) -> Result<()> {
        event.defer_ephemeral(ctx).await?;
        let debt = get_debt().await?;

        // replace repeating spaces with a single space
        // let mut date = match debt.date.day() {
        //     1 | 21 | 31 => debt.date.format("%B %est %Y").to_string(),
        //     2 | 22 => debt.date.format("%B %end %Y").to_string(),
        //     3 | 23 => debt.date.format("%B %erd %Y").to_string(),
        //     _ => debt.date.format("%B %eth %Y").to_string(),
        // };
        // date = date.replace("  ", " ");
        let amount = debt.amount;
        event
            .create_followup(
                ctx,
                CreateInteractionResponseFollowup::new().content(format!(
                    "The US national debt as of <t:{}:F> (most recent data available) is:```\n{}\n```",
                    debt.date.and_time(chrono::Utc::now().time()).and_utc().timestamp(),
                    get_fancy_currency(amount)
                )),
            )
            .await?;

        Ok(())
    }
}

const V: &[&str] = &[
    "thousand",
    "million",
    "billion",
    "trillion",
    "quadrillion",
    "quintillion",
    "sextillion",
    "septillion",
    "octillion",
    "nonillion",
    "decillion",
    "undecillion",
    "duodecillion",
    "tredecillion",
    "quattuordecillion",
    "quindecillion",
    "sexdecillion",
    "septendecillion",
    "octodecillion",
    "novemdecillion",
    "vigintillion",
];

fn get_fancy_currency(c: f64) -> String {
    // for emphasis we want the WHOLE number, every single digit
    let cents = (c * 100.0).round() as i64 % 100;
    let mut dollars = c as i64;
    let is_singular = dollars == 1;
    // 100,010,000,001 -> 100 billion 10 million and 1 dollar
    let mut parts: Vec<String> = vec![];
    for i in 0.. {
        if dollars == 0 {
            break;
        }
        let part = dollars % 1000;
        dollars /= 1000;
        if part == 0 {
            continue;
        }
        let part_str = if i == 0 {
            part.to_string()
        } else {
            format!("{} {}", part, V[i - 1])
        };
        parts.push(part_str);
    }
    parts.reverse();
    let mut result = parts.join(" ");
    result.push_str(" dollar");
    if !is_singular {
        result.push('s');
    }
    if cents != 0 {
        result.push_str(&format!(
            " and {} cent{}",
            cents,
            if cents == 1 { "" } else { "s" }
        ));
    }
    result
}

#[derive(Debug, Deserialize)]
struct RawNationalDebt {
    data: Vec<RawNationalDebtEntry>,
}

#[derive(Debug, Deserialize)]
struct RawNationalDebtEntry {
    tot_pub_debt_out_amt: String,
    record_calendar_year: String,
    record_calendar_month: String,
    record_calendar_day: String,
}

struct NationalDebt {
    amount: f64,
    date: chrono::NaiveDate,
}

impl TryFrom<RawNationalDebt> for NationalDebt {
    type Error = anyhow::Error;

    fn try_from(raw: RawNationalDebt) -> Result<Self> {
        raw.data
            .iter()
            .flat_map(|entry: &RawNationalDebtEntry| -> Result<NationalDebt> {
                try {
                    let amount = entry.tot_pub_debt_out_amt.parse()?;

                    let date = chrono::NaiveDate::from_ymd_opt(
                        entry.record_calendar_year.parse()?,
                        entry.record_calendar_month.parse()?,
                        entry.record_calendar_day.parse()?,
                    )
                    .ok_or(anyhow::anyhow!("Invalid date"))?;

                    Self { amount, date }
                }
            })
            .max_by(|a: &NationalDebt, b: &NationalDebt| a.date.cmp(&b.date))
            .ok_or(anyhow::anyhow!("No entries in response"))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_fancy_currency() {
        // create a tokio runtime and print the current national debt with the fancy currency
        let rt = common::tokio::runtime::Runtime::new().unwrap();
        let debt = rt.block_on(get_debt()).unwrap();
        panic!("{}", get_fancy_currency(debt.amount));
    }
}
