//! Statistics about the messages in a data package: how many, when, and
//! where. Everything is computed locally.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Timelike, Utc};
use serde::Serialize;

use crate::package::Package;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MonthCount {
    /// `YYYY-MM`.
    pub month: String,
    pub messages: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DayCount {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub messages: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Statistics {
    pub messages: u64,
    /// Messages with at least one attachment.
    pub with_attachments: u64,
    pub attachments: u64,
    /// Messages without text, usually attachments only.
    pub without_text: u64,
    pub words: u64,
    pub characters: u64,
    pub first_message: Option<DateTime<Utc>>,
    pub last_message: Option<DateTime<Utc>>,
    /// Every month from the first message to the last, including empty ones.
    pub months: Vec<MonthCount>,
    /// Messages by weekday (Monday first) and hour, in local time.
    pub week: [[u64; 24]; 7],
    pub busiest_day: Option<DayCount>,
    /// Days with at least one message.
    pub active_days: u64,
}

impl Statistics {
    /// `utc_offset_minutes` places the messages in the user's time zone,
    /// e.g. 120 for UTC+2. Daylight saving time is not accounted for.
    pub fn of(package: &Package, utc_offset_minutes: i32) -> Statistics {
        let offset = FixedOffset::east_opt(utc_offset_minutes.clamp(-18 * 60, 18 * 60) * 60)
            .unwrap_or(FixedOffset::east_opt(0).expect("zero offset"));
        let mut stats = Statistics::default();
        let mut months: BTreeMap<(i32, u32), u64> = BTreeMap::new();
        let mut days: BTreeMap<NaiveDate, u64> = BTreeMap::new();
        for message in package.channels.iter().flat_map(|c| &c.messages) {
            let sent = message.id.created_at();
            stats.messages += 1;
            stats.first_message = Some(stats.first_message.map_or(sent, |t| t.min(sent)));
            stats.last_message = Some(stats.last_message.map_or(sent, |t| t.max(sent)));
            if !message.attachments.is_empty() {
                stats.with_attachments += 1;
                stats.attachments += message.attachments.len() as u64;
            }
            let text = message.content.trim();
            if text.is_empty() {
                stats.without_text += 1;
            }
            stats.words += text.split_whitespace().count() as u64;
            stats.characters += text.chars().count() as u64;

            let local = sent.with_timezone(&offset);
            *months.entry((local.year(), local.month())).or_default() += 1;
            *days.entry(local.date_naive()).or_default() += 1;
            stats.week[local.weekday().num_days_from_monday() as usize][local.hour() as usize] += 1;
        }
        stats.months = fill_months(&months);
        stats.active_days = days.len() as u64;
        // The earliest of equally busy days.
        stats.busiest_day = days
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
            .map(|(date, &messages)| DayCount {
                date: date.format("%Y-%m-%d").to_string(),
                messages,
            });
        stats
    }
}

fn fill_months(months: &BTreeMap<(i32, u32), u64>) -> Vec<MonthCount> {
    let (Some(&first), Some(&last)) = (months.keys().next(), months.keys().next_back()) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let (mut year, mut month) = first;
    while (year, month) <= last {
        result.push(MonthCount {
            month: format!("{year:04}-{month:02}"),
            messages: months.get(&(year, month)).copied().unwrap_or(0),
        });
        (year, month) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{PackageChannel, PackageMessage};
    use crate::snowflake::Snowflake;
    use crate::targets::TargetKind;
    use chrono::TimeZone;

    fn message(time: &str, content: &str, attachments: usize) -> PackageMessage {
        let time = Utc.from_utc_datetime(
            &chrono::NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M").unwrap(),
        );
        PackageMessage {
            id: Snowflake::from_datetime(time),
            content: content.into(),
            attachments: vec!["https://cdn.discordapp.com/a.png".into(); attachments],
        }
    }

    fn package(messages: Vec<PackageMessage>) -> Package {
        Package {
            owner: None,
            channels: vec![PackageChannel {
                id: Snowflake(1),
                kind: TargetKind::Dm,
                name: String::new(),
                guild: None,
                messages,
            }],
        }
    }

    #[test]
    fn counts_messages_by_time_and_content() {
        let stats = Statistics::of(
            &package(vec![
                message("2024-01-31 23:30", "hello there", 0),
                message("2024-01-31 10:00", "", 2),
                message("2024-04-02 08:15", "  one  two three ", 1),
            ]),
            60,
        );
        assert_eq!(stats.messages, 3);
        assert_eq!((stats.with_attachments, stats.attachments), (2, 3));
        assert_eq!(stats.without_text, 1);
        assert_eq!(stats.words, 5);
        assert_eq!(
            stats.characters,
            "hello there".len() as u64 + "one  two three".len() as u64
        );
        // 23:30 UTC is 00:30 on 1 February at UTC+1.
        let months: Vec<_> = stats
            .months
            .iter()
            .map(|m| (m.month.as_str(), m.messages))
            .collect();
        assert_eq!(
            months,
            [
                ("2024-01", 1),
                ("2024-02", 1),
                ("2024-03", 0),
                ("2024-04", 1)
            ]
        );
        // 1 February 2024 was a Thursday.
        assert_eq!(stats.week[3][0], 1);
        assert_eq!(stats.week[2][11], 1);
        assert_eq!(stats.active_days, 3);
        assert_eq!(stats.busiest_day.unwrap().date, "2024-01-31");
    }

    #[test]
    fn an_empty_package_has_no_months() {
        let stats = Statistics::of(&package(Vec::new()), 0);
        assert_eq!(stats, Statistics::default());
    }
}
