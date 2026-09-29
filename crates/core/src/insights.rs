//! Insights into the messages of a data package: when, where and what the
//! owner wrote. Everything is computed on this computer, from the package
//! alone; nothing is sent anywhere.
//!
//! [`Index::build`] goes through the package once and keeps a compact
//! entry per message, in local time. Every question after that (a date
//! range, some servers) is answered from the index in a few milliseconds;
//! only words, links and search read the texts again.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::package::{Package, PackageMessage};
use crate::snowflake::Snowflake;
use crate::stopwords::is_stop_word;
use crate::targets::TargetKind;

/// Colours are given to this many places in charts; the rest is "other".
pub const SERIES: usize = 6;
const PREVIEW_CHARS: usize = 280;

/// A server, DM or group DM of the package.
#[derive(Debug, Clone, Serialize)]
pub struct Place {
    pub id: Snowflake,
    pub kind: TargetKind,
    pub name: String,
    pub messages: u64,
    pub channels: Vec<ChannelName>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelName {
    pub id: Snowflake,
    pub name: String,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    id: Snowflake,
    /// Local date as days since 1 January of year 1.
    day: i32,
    hour: u8,
    /// Monday is 0.
    weekday: u8,
    place: u32,
    /// Index into `Package::channels` and its `messages`.
    channel: u32,
    message: u32,
    chars: u32,
    words: u32,
    attachments: u16,
    links: u16,
}

pub struct Index {
    package: Arc<Package>,
    /// Most messages first; charts colour them in this order.
    places: Vec<Place>,
    /// Oldest first.
    entries: Vec<Entry>,
    /// Person → name, from DMs with them.
    people: HashMap<Snowflake, String>,
}

/// Which messages to look at. Empty lists mean all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Scope {
    /// First day, inclusive, in local time.
    pub from: Option<NaiveDate>,
    /// Last day, inclusive.
    pub to: Option<NaiveDate>,
    pub places: Vec<Snowflake>,
    pub channels: Vec<Snowflake>,
}

/// What the Insights screen needs to know up front.
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub places: Vec<Place>,
    pub messages: u64,
    pub first_day: Option<NaiveDate>,
    pub last_day: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageRef {
    pub id: Snowflake,
    pub sent_at: DateTime<Utc>,
    /// Index into [`Info::places`].
    pub place: usize,
    pub channel: String,
    pub text: String,
    pub attachments: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayCount {
    pub date: NaiveDate,
    pub messages: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Span {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub days: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Year {
    pub year: i32,
    pub messages: u64,
    pub words: u64,
    pub active_days: u64,
    pub top_place: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Overview {
    pub messages: u64,
    pub words: u64,
    pub characters: u64,
    pub attachments: u64,
    pub with_attachments: u64,
    pub without_text: u64,
    pub links: u64,
    pub places: u64,
    pub active_days: u64,
    /// Days from the first to the last message, both included.
    pub span_days: u64,
    pub first: Option<MessageRef>,
    pub last: Option<MessageRef>,
    pub busiest_day: Option<DayCount>,
    pub longest_streak: Option<Span>,
    /// The longest time without a message, between two messages.
    pub longest_break: Option<Span>,
    pub years: Vec<Year>,
}

/// A place drawn in its own colour; `slot` picks the colour and stays the
/// same for a place whatever the date range.
#[derive(Debug, Clone, Serialize)]
pub struct Series {
    pub place: usize,
    pub slot: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bucket {
    /// `YYYY-MM` for months, the Monday for weeks.
    pub key: String,
    pub total: u64,
    /// Per series, in the order of [`Timeline::series`]; the rest is "other".
    pub series: Vec<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Timeline {
    /// Days with messages.
    pub days: Vec<DayCount>,
    pub series: Vec<Series>,
    /// Every month from the first message to the last.
    pub months: Vec<Bucket>,
    /// Every week (from Monday) from the first message to the last.
    pub weeks: Vec<Bucket>,
    /// Weekday (Monday first) × hour, local time.
    pub week_hours: [[u64; 24]; 7],
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaceRow {
    pub place: usize,
    pub messages: u64,
    pub words: u64,
    pub attachments: u64,
    pub active_days: u64,
    pub first: DateTime<Utc>,
    pub last: DateTime<Utc>,
    /// Per month of [`PlacesReport::months`].
    pub monthly: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelRow {
    pub id: Snowflake,
    pub name: String,
    pub place: usize,
    pub messages: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PlacesReport {
    pub months: Vec<String>,
    /// Most messages first.
    pub places: Vec<PlaceRow>,
    /// Channels of the selected servers, most messages first.
    pub channels: Vec<ChannelRow>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Count {
    pub key: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Emoji {
    /// The emoji itself, or the name of a custom one.
    pub emoji: String,
    /// For custom emoji: its ID, for the picture.
    pub id: Option<Snowflake>,
    pub animated: bool,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Mention {
    pub id: Snowflake,
    pub name: Option<String>,
    pub count: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct WordsReport {
    /// Most used words without the most common ones ("the", "und", …).
    pub words: Vec<Count>,
    pub total_words: u64,
    pub distinct_words: u64,
    pub emoji: Vec<Emoji>,
    pub total_emoji: u64,
    pub mentions: Vec<Mention>,
    /// Messages by length in characters; see [`LENGTH_BUCKETS`].
    pub lengths: Vec<u64>,
    /// Average characters per message with text, per month.
    pub monthly_length: Vec<(String, f64)>,
    pub longest: Option<MessageRef>,
}

/// Upper bounds (inclusive) of the length buckets; the last is open.
pub const LENGTH_BUCKETS: [u32; 8] = [10, 25, 50, 100, 250, 500, 1000, 2000];

#[derive(Debug, Clone, Default, Serialize)]
pub struct LinksReport {
    pub links: u64,
    pub messages_with_links: u64,
    /// Without `www.`.
    pub domains: Vec<Count>,
    pub attachments: u64,
    /// `image`, `video`, `audio`, `document`, `archive`, `other`.
    pub kinds: Vec<Count>,
    pub extensions: Vec<Count>,
    pub months: Vec<String>,
    /// Per month, attachments per kind in the order of [`FILE_KINDS`].
    pub monthly: Vec<Vec<u64>>,
}

pub const FILE_KINDS: [&str; 6] = ["image", "video", "audio", "document", "archive", "other"];

#[derive(Debug, Clone, Default, Serialize)]
pub struct SearchResult {
    pub total: u64,
    /// Matches per place, most first.
    pub places: Vec<(usize, u64)>,
    /// Newest first, at most `limit`.
    pub messages: Vec<MessageRef>,
}

static CUSTOM_EMOJI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<(a?):(\w{1,32}):(\d{15,21})>").expect("valid pattern"));
static MENTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<@!?(\d{15,21})>").expect("valid pattern"));
/// An emoji with its skin tone and ZWJ parts, or a flag.
pub(crate) static EMOJI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
          [\x{1F1E6}-\x{1F1FF}]{2}
        | \p{Extended_Pictographic}[\x{FE0F}\x{1F3FB}-\x{1F3FF}]?
          (?:\x{200D}\p{Extended_Pictographic}[\x{FE0F}\x{1F3FB}-\x{1F3FF}]?)*",
    )
    .expect("valid pattern")
});

/// The emoji in `text`, whole: 👍🏽 and 👩‍💻 stay one emoji each.
pub(crate) fn emoji_in(text: &str) -> impl Iterator<Item = &str> {
    EMOJI
        .find_iter(text)
        .map(|m| m.as_str())
        // Keycap digits and the like are not what people mean.
        .filter(|e| e.chars().next().is_some_and(|c| !c.is_ascii()))
}

impl Index {
    /// Goes through every message once. `tz` is the user's time zone
    /// (`chrono::Local` in the app), with daylight saving time.
    pub fn build<Tz: TimeZone + Sync>(package: Arc<Package>, tz: &Tz) -> Index {
        let mut targets = package.targets();
        targets.sort_by_key(|t| std::cmp::Reverse(t.messages));
        let mut place_of: HashMap<Snowflake, u32> = HashMap::new();
        let places: Vec<Place> = targets
            .iter()
            .enumerate()
            .map(|(i, t)| {
                for c in &t.channels {
                    place_of.insert(c.id, i as u32);
                }
                Place {
                    id: t.target.id,
                    kind: t.target.kind,
                    name: t.target.name.clone(),
                    messages: t.messages,
                    channels: t
                        .channels
                        .iter()
                        .map(|c| ChannelName {
                            id: c.id,
                            name: c.name.clone(),
                        })
                        .collect(),
                }
            })
            .collect();

        let mut people = HashMap::new();
        for channel in &package.channels {
            if channel.kind == TargetKind::Dm {
                let others: Vec<_> = channel
                    .recipients
                    .iter()
                    .filter(|id| Some(**id) != package.owner)
                    .collect();
                if let [other] = others.as_slice() {
                    people.insert(**other, channel.name.clone());
                }
            }
        }

        // Local time and counts per message, on all cores.
        let channels: Vec<(usize, &crate::package::PackageChannel)> =
            package.channels.iter().enumerate().collect();
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
        let chunk = channels.len().div_ceil(workers).max(1);
        let parts: Vec<Vec<Entry>> = std::thread::scope(|scope| {
            let handles: Vec<_> = channels
                .chunks(chunk)
                .map(|part| {
                    let place_of = &place_of;
                    scope.spawn(move || {
                        let mut entries = Vec::new();
                        for &(c, channel) in part {
                            let Some(&place) = place_of.get(&channel.id) else {
                                continue;
                            };
                            for (m, message) in channel.messages.iter().enumerate() {
                                entries.push(entry(message, tz, place, c, m));
                            }
                        }
                        entries
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("indexer"))
                .collect()
        });
        let mut entries = Vec::with_capacity(package.message_count() as usize);
        for part in parts {
            entries.extend(part);
        }
        entries.sort_unstable_by_key(|e| e.id);
        Index {
            package,
            places,
            entries,
            people,
        }
    }

    pub fn info(&self) -> Info {
        Info {
            places: self.places.clone(),
            messages: self.entries.len() as u64,
            first_day: self.entries.first().map(|e| date_of(e.day)),
            last_day: self.entries.last().map(|e| date_of(e.day)),
        }
    }

    fn message(&self, entry: &Entry) -> &PackageMessage {
        &self.package.channels[entry.channel as usize].messages[entry.message as usize]
    }

    fn message_ref(&self, entry: &Entry, full: bool) -> MessageRef {
        let message = self.message(entry);
        let text = if full {
            message.content.clone()
        } else {
            preview(&message.content)
        };
        MessageRef {
            id: entry.id,
            sent_at: entry.id.created_at(),
            place: entry.place as usize,
            channel: self.package.channels[entry.channel as usize].name.clone(),
            text,
            attachments: message.attachments.len(),
        }
    }

    /// The entries in `scope`, oldest first.
    fn select<'a>(&'a self, scope: &Scope) -> impl Iterator<Item = &'a Entry> + 'a {
        let start = match scope.from {
            Some(from) => {
                let day = from.num_days_from_ce();
                self.entries.partition_point(|e| e.day < day)
            }
            None => 0,
        };
        let end = match scope.to {
            Some(to) => {
                let day = to.num_days_from_ce();
                self.entries.partition_point(|e| e.day <= day)
            }
            None => self.entries.len(),
        };
        let places: HashSet<u32> = self
            .places
            .iter()
            .enumerate()
            .filter(|(_, p)| scope.places.contains(&p.id))
            .map(|(i, _)| i as u32)
            .collect();
        let channels: HashSet<u32> = self
            .package
            .channels
            .iter()
            .enumerate()
            .filter(|(_, c)| scope.channels.contains(&c.id))
            .map(|(i, _)| i as u32)
            .collect();
        let (all_places, all_channels) = (scope.places.is_empty(), scope.channels.is_empty());
        self.entries[start..end.max(start)].iter().filter(move |e| {
            (all_places || places.contains(&e.place))
                && (all_channels || channels.contains(&e.channel))
        })
    }

    pub fn overview(&self, scope: &Scope) -> Overview {
        let mut o = Overview::default();
        let mut first: Option<&Entry> = None;
        let mut last: Option<&Entry> = None;
        let mut places = HashSet::new();
        let mut days: Vec<(i32, u64)> = Vec::new();
        let mut years: BTreeMap<i32, (u64, u64, u64, HashMap<u32, u64>)> = BTreeMap::new();
        for e in self.select(scope) {
            o.messages += 1;
            o.words += e.words as u64;
            o.characters += e.chars as u64;
            o.attachments += e.attachments as u64;
            o.links += e.links as u64;
            if e.attachments > 0 {
                o.with_attachments += 1;
            }
            if e.chars == 0 {
                o.without_text += 1;
            }
            places.insert(e.place);
            first.get_or_insert(e);
            let new_day = days.last().is_none_or(|(d, _)| *d != e.day);
            match days.last_mut() {
                Some((d, n)) if *d == e.day => *n += 1,
                _ => days.push((e.day, 1)),
            }
            let year = years.entry(date_of(e.day).year()).or_default();
            year.0 += 1;
            year.1 += e.words as u64;
            if new_day {
                year.2 += 1;
            }
            *year.3.entry(e.place).or_default() += 1;
            last = Some(e);
        }
        o.places = places.len() as u64;
        o.active_days = days.len() as u64;
        o.first = first.map(|e| self.message_ref(e, false));
        o.last = last.map(|e| self.message_ref(e, false));
        if let (Some((a, _)), Some((b, _))) = (days.first(), days.last()) {
            o.span_days = (b - a + 1) as u64;
        }
        // The earliest of equally busy days.
        o.busiest_day = days
            .iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
            .map(|(day, n)| DayCount {
                date: date_of(*day),
                messages: *n,
            });
        let mut streak = (0, 0);
        let mut best_streak: Option<(i32, i32)> = None;
        let mut best_break: Option<(i32, i32)> = None;
        for (i, (day, _)) in days.iter().enumerate() {
            if i > 0 && days[i - 1].0 == day - 1 {
                streak.1 = *day;
            } else {
                if i > 0 {
                    let gap = (days[i - 1].0 + 1, day - 1);
                    if best_break.is_none_or(|b| gap.1 - gap.0 > b.1 - b.0) {
                        best_break = Some(gap);
                    }
                }
                streak = (*day, *day);
            }
            if best_streak.is_none_or(|b| streak.1 - streak.0 > b.1 - b.0) {
                best_streak = Some(streak);
            }
        }
        let span = |(a, b): (i32, i32)| Span {
            from: date_of(a),
            to: date_of(b),
            days: (b - a + 1) as i64,
        };
        o.longest_streak = best_streak.map(span);
        o.longest_break = best_break.map(span);
        o.years = years
            .into_iter()
            .map(|(year, (messages, words, active_days, by_place))| Year {
                year,
                messages,
                words,
                active_days,
                top_place: by_place
                    .into_iter()
                    .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
                    .map(|(p, _)| p as usize),
            })
            .collect();
        o
    }

    /// The places drawn in their own colour: the selected ones if there are
    /// few, else the biggest overall. Either way independent of dates, so
    /// colours stay put when the range changes.
    fn series(&self, scope: &Scope) -> Vec<Series> {
        let chosen: Vec<usize> = if !scope.places.is_empty() {
            let mut chosen: Vec<usize> = (0..self.places.len())
                .filter(|&i| scope.places.contains(&self.places[i].id))
                .collect();
            chosen.truncate(SERIES);
            chosen
        } else {
            (0..self.places.len().min(SERIES)).collect()
        };
        // The biggest places keep their colour when they are selected;
        // others take the free ones.
        let mut free: Vec<usize> = (0..SERIES).filter(|s| !chosen.contains(s)).collect();
        free.reverse();
        chosen
            .into_iter()
            .map(|place| Series {
                place,
                slot: if place < SERIES {
                    place
                } else {
                    free.pop().unwrap_or(0)
                },
            })
            .collect()
    }

    pub fn timeline(&self, scope: &Scope) -> Timeline {
        let series = self.series(scope);
        let slot_of: HashMap<u32, usize> = series
            .iter()
            .enumerate()
            .map(|(i, s)| (s.place as u32, i))
            .collect();
        let mut t = Timeline::default();
        let mut months: BTreeMap<(i32, u32), Vec<u64>> = BTreeMap::new();
        let mut weeks: BTreeMap<i32, Vec<u64>> = BTreeMap::new();
        let width = series.len() + 1;
        for e in self.select(scope) {
            match t.days.last_mut() {
                Some(d) if d.date == date_of(e.day) => d.messages += 1,
                _ => t.days.push(DayCount {
                    date: date_of(e.day),
                    messages: 1,
                }),
            }
            let column = slot_of.get(&e.place).copied().unwrap_or(series.len());
            let date = date_of(e.day);
            months
                .entry((date.year(), date.month()))
                .or_insert_with(|| vec![0; width])[column] += 1;
            weeks
                .entry(e.day - e.weekday as i32)
                .or_insert_with(|| vec![0; width])[column] += 1;
            t.week_hours[e.weekday as usize][e.hour as usize] += 1;
        }
        let bucket = |key: String, counts: Option<&Vec<u64>>| {
            let counts = counts.cloned().unwrap_or_else(|| vec![0; width]);
            Bucket {
                key,
                total: counts.iter().sum(),
                series: counts[..width - 1].to_vec(),
            }
        };
        t.months = month_range(months.keys().next(), months.keys().next_back())
            .into_iter()
            .map(|(y, m)| bucket(format!("{y:04}-{m:02}"), months.get(&(y, m))))
            .collect();
        if let (Some(&a), Some(&b)) = (weeks.keys().next(), weeks.keys().next_back()) {
            t.weeks = (a..=b)
                .step_by(7)
                .map(|w| bucket(date_of(w).to_string(), weeks.get(&w)))
                .collect();
        }
        t.series = series;
        t
    }

    pub fn places(&self, scope: &Scope) -> PlacesReport {
        struct Acc {
            messages: u64,
            words: u64,
            attachments: u64,
            days: u64,
            last_day: i32,
            first: Snowflake,
            last: Snowflake,
            monthly: BTreeMap<(i32, u32), u64>,
        }
        let mut by_place: HashMap<u32, Acc> = HashMap::new();
        let mut by_channel: HashMap<u32, u64> = HashMap::new();
        let mut all_months: Option<((i32, u32), (i32, u32))> = None;
        for e in self.select(scope) {
            let date = date_of(e.day);
            let month = (date.year(), date.month());
            all_months = Some(match all_months {
                Some((a, _)) => (a, month),
                None => (month, month),
            });
            let acc = by_place.entry(e.place).or_insert(Acc {
                messages: 0,
                words: 0,
                attachments: 0,
                days: 0,
                last_day: i32::MIN,
                first: e.id,
                last: e.id,
                monthly: BTreeMap::new(),
            });
            acc.messages += 1;
            acc.words += e.words as u64;
            acc.attachments += e.attachments as u64;
            if acc.last_day != e.day {
                acc.days += 1;
                acc.last_day = e.day;
            }
            acc.last = e.id;
            *acc.monthly.entry(month).or_default() += 1;
            *by_channel.entry(e.channel).or_default() += 1;
        }
        let months = all_months
            .map(|(a, b)| month_range(Some(&a), Some(&b)))
            .unwrap_or_default();
        let mut places: Vec<PlaceRow> = by_place
            .into_iter()
            .map(|(place, acc)| PlaceRow {
                place: place as usize,
                messages: acc.messages,
                words: acc.words,
                attachments: acc.attachments,
                active_days: acc.days,
                first: acc.first.created_at(),
                last: acc.last.created_at(),
                monthly: months
                    .iter()
                    .map(|m| acc.monthly.get(m).copied().unwrap_or(0))
                    .collect(),
            })
            .collect();
        places.sort_by_key(|p| (std::cmp::Reverse(p.messages), p.place));
        // Channels only say something within servers.
        let servers: HashSet<usize> = places
            .iter()
            .filter(|p| self.places[p.place].kind == TargetKind::Guild)
            .map(|p| p.place)
            .collect();
        let mut channels: Vec<ChannelRow> = by_channel
            .into_iter()
            .filter_map(|(c, messages)| {
                let channel = &self.package.channels[c as usize];
                let place = self
                    .places
                    .iter()
                    .position(|p| p.channels.iter().any(|pc| pc.id == channel.id))?;
                servers.contains(&place).then(|| ChannelRow {
                    id: channel.id,
                    name: channel.name.clone(),
                    place,
                    messages,
                })
            })
            .collect();
        channels.sort_by_key(|c| (std::cmp::Reverse(c.messages), c.id));
        PlacesReport {
            months: months
                .iter()
                .map(|(y, m)| format!("{y:04}-{m:02}"))
                .collect(),
            places,
            channels,
        }
    }

    pub fn words(&self, scope: &Scope) -> WordsReport {
        let entries: Vec<&Entry> = self.select(scope).collect();
        // Counting words is the slowest part: split the work.
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
        let chunk = entries.len().div_ceil(workers).max(1);
        let parts: Vec<WordCounts> = std::thread::scope(|scope| {
            let handles: Vec<_> = entries
                .chunks(chunk)
                .map(|part| scope.spawn(move || self.count_words(part)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("word counter"))
                .collect()
        });
        let mut total = WordCounts::default();
        for part in parts {
            total.merge(part);
        }

        let mut report = WordsReport {
            total_words: total.total_words,
            distinct_words: total.words.len() as u64,
            total_emoji: total.emoji.values().sum::<u64>() + total.custom.values().sum::<u64>(),
            lengths: vec![0; LENGTH_BUCKETS.len() + 1],
            ..Default::default()
        };
        report.words = top(total.words, 200);
        let mut emoji: Vec<Emoji> = total
            .emoji
            .into_iter()
            .map(|(emoji, count)| Emoji {
                emoji,
                id: None,
                animated: false,
                count,
            })
            .chain(
                total
                    .custom
                    .into_iter()
                    .map(|((name, id, animated), count)| Emoji {
                        emoji: name,
                        id: Some(id),
                        animated,
                        count,
                    }),
            )
            .collect();
        emoji.sort_by(|a, b| b.count.cmp(&a.count).then(a.emoji.cmp(&b.emoji)));
        emoji.truncate(60);
        report.emoji = emoji;
        let mut mentions: Vec<Mention> = total
            .mentions
            .into_iter()
            .map(|(id, count)| Mention {
                id,
                name: self.people.get(&id).cloned(),
                count,
            })
            .collect();
        mentions.sort_by(|a, b| b.count.cmp(&a.count).then(a.id.cmp(&b.id)));
        mentions.truncate(30);
        report.mentions = mentions;

        let mut monthly: BTreeMap<(i32, u32), (u64, u64)> = BTreeMap::new();
        let mut longest: Option<&Entry> = None;
        for e in &entries {
            if e.chars == 0 {
                continue;
            }
            let bucket = LENGTH_BUCKETS
                .iter()
                .position(|max| e.chars <= *max)
                .unwrap_or(LENGTH_BUCKETS.len());
            report.lengths[bucket] += 1;
            let date = date_of(e.day);
            let month = monthly.entry((date.year(), date.month())).or_default();
            month.0 += e.chars as u64;
            month.1 += 1;
            if longest.is_none_or(|l| e.chars > l.chars) {
                longest = Some(e);
            }
        }
        report.monthly_length = monthly
            .into_iter()
            .map(|((y, m), (chars, n))| (format!("{y:04}-{m:02}"), chars as f64 / n as f64))
            .collect();
        report.longest = longest.map(|e| self.message_ref(e, true));
        report
    }

    fn count_words(&self, entries: &[&Entry]) -> WordCounts {
        let mut counts = WordCounts::default();
        let mut lower = String::new();
        for e in entries {
            let text = self.message(e).content.as_str();
            if text.is_empty() {
                continue;
            }
            if text.contains('<') {
                for caps in CUSTOM_EMOJI.captures_iter(text) {
                    if let Ok(id) = caps[3].parse() {
                        let key = (caps[2].to_owned(), Snowflake(id), &caps[1] == "a");
                        *counts.custom.entry(key).or_default() += 1;
                    }
                }
                for caps in MENTION.captures_iter(text) {
                    if let Ok(id) = caps[1].parse() {
                        *counts.mentions.entry(Snowflake(id)).or_default() += 1;
                    }
                }
            }
            if !text.is_ascii() {
                for emoji in emoji_in(text) {
                    *counts.emoji.entry(emoji.to_owned()).or_default() += 1;
                }
            }
            for token in text.split_whitespace() {
                if token.starts_with("http://")
                    || token.starts_with("https://")
                    || (token.starts_with('<') && token.ends_with('>'))
                {
                    continue;
                }
                for piece in token.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
                {
                    let piece = piece.trim_matches(|c| c == '\'' || c == '’');
                    if piece.is_empty() {
                        continue;
                    }
                    counts.total_words += 1;
                    if piece.chars().count() < 2 || piece.chars().all(|c| c.is_numeric()) {
                        continue;
                    }
                    lower.clear();
                    lower.extend(piece.chars().flat_map(char::to_lowercase));
                    let normalized = lower.replace('’', "'");
                    if is_stop_word(&normalized) {
                        continue;
                    }
                    *counts.words.entry(normalized).or_default() += 1;
                }
            }
        }
        counts
    }

    pub fn links(&self, scope: &Scope) -> LinksReport {
        let mut report = LinksReport::default();
        let mut domains: HashMap<String, u64> = HashMap::new();
        let mut kinds: HashMap<&'static str, u64> = HashMap::new();
        let mut extensions: HashMap<String, u64> = HashMap::new();
        let mut monthly: BTreeMap<(i32, u32), Vec<u64>> = BTreeMap::new();
        for e in self.select(scope) {
            if e.links == 0 && e.attachments == 0 {
                continue;
            }
            let message = self.message(e);
            if e.links > 0 {
                report.messages_with_links += 1;
                for token in message.content.split_whitespace() {
                    if let Some(host) = host_of(token) {
                        report.links += 1;
                        *domains.entry(host).or_default() += 1;
                    }
                }
            }
            let date = date_of(e.day);
            for url in &message.attachments {
                report.attachments += 1;
                let ext = extension_of(url);
                let kind = file_kind(&ext);
                *kinds.entry(kind).or_default() += 1;
                if !ext.is_empty() {
                    *extensions.entry(ext).or_default() += 1;
                }
                let column = FILE_KINDS.iter().position(|k| *k == kind).unwrap_or(5);
                monthly
                    .entry((date.year(), date.month()))
                    .or_insert_with(|| vec![0; FILE_KINDS.len()])[column] += 1;
            }
        }
        report.domains = top(domains, 40);
        report.kinds = FILE_KINDS
            .iter()
            .map(|k| Count {
                key: (*k).to_owned(),
                count: kinds.get(k).copied().unwrap_or(0),
            })
            .collect();
        report.extensions = top(extensions, 20);
        let months = month_range(monthly.keys().next(), monthly.keys().next_back());
        report.monthly = months
            .iter()
            .map(|m| {
                monthly
                    .get(m)
                    .cloned()
                    .unwrap_or_else(|| vec![0; FILE_KINDS.len()])
            })
            .collect();
        report.months = months
            .iter()
            .map(|(y, m)| format!("{y:04}-{m:02}"))
            .collect();
        report
    }

    /// Messages containing all words of `query`, in any case: the same rule
    /// as "Containing all of these words" when cleaning up, so the results
    /// can be handed over as they are.
    pub fn search(&self, scope: &Scope, query: &str, limit: usize) -> SearchResult {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut result = SearchResult::default();
        if words.is_empty() {
            return result;
        }
        let entries: Vec<&Entry> = self.select(scope).collect();
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
        let chunk = entries.len().div_ceil(workers).max(1);
        let found: Vec<&Entry> = std::thread::scope(|s| {
            let handles: Vec<_> = entries
                .chunks(chunk)
                .map(|part| {
                    let words = &words;
                    s.spawn(move || {
                        part.iter()
                            .copied()
                            .filter(|e| {
                                let text = self.message(e).content.to_lowercase();
                                words.iter().all(|w| text.contains(w.as_str()))
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().expect("search"))
                .collect()
        });
        result.total = found.len() as u64;
        let mut places: HashMap<usize, u64> = HashMap::new();
        for e in &found {
            *places.entry(e.place as usize).or_default() += 1;
        }
        let mut places: Vec<(usize, u64)> = places.into_iter().collect();
        places.sort_by_key(|(p, n)| (std::cmp::Reverse(*n), *p));
        result.places = places;
        result.messages = found
            .iter()
            .rev()
            .take(limit)
            .map(|e| self.message_ref(e, true))
            .collect();
        result
    }
}

#[derive(Default)]
struct WordCounts {
    words: HashMap<String, u64>,
    total_words: u64,
    emoji: HashMap<String, u64>,
    custom: HashMap<(String, Snowflake, bool), u64>,
    mentions: HashMap<Snowflake, u64>,
}

impl WordCounts {
    fn merge(&mut self, other: WordCounts) {
        fn add<K: std::hash::Hash + Eq>(into: &mut HashMap<K, u64>, from: HashMap<K, u64>) {
            for (k, v) in from {
                *into.entry(k).or_default() += v;
            }
        }
        self.total_words += other.total_words;
        add(&mut self.words, other.words);
        add(&mut self.emoji, other.emoji);
        add(&mut self.custom, other.custom);
        add(&mut self.mentions, other.mentions);
    }
}

fn top<K: Into<String>>(counts: HashMap<K, u64>, n: usize) -> Vec<Count> {
    let mut list: Vec<Count> = counts
        .into_iter()
        .map(|(key, count)| Count {
            key: key.into(),
            count,
        })
        .collect();
    list.sort_by(|a, b| b.count.cmp(&a.count).then(a.key.cmp(&b.key)));
    list.truncate(n);
    list
}

fn entry<Tz: TimeZone>(
    message: &PackageMessage,
    tz: &Tz,
    place: u32,
    channel: usize,
    index: usize,
) -> Entry {
    let local = message.id.created_at().with_timezone(tz).naive_local();
    let text = message.content.as_str();
    Entry {
        id: message.id,
        day: local.date().num_days_from_ce(),
        hour: local.hour() as u8,
        weekday: local.weekday().num_days_from_monday() as u8,
        place,
        channel: channel as u32,
        message: index as u32,
        chars: text.chars().count() as u32,
        words: text.split_whitespace().count() as u32,
        attachments: message.attachments.len().min(u16::MAX as usize) as u16,
        links: (text.matches("http://").count() + text.matches("https://").count())
            .min(u16::MAX as usize) as u16,
    }
}

fn date_of(day: i32) -> NaiveDate {
    NaiveDate::from_num_days_from_ce_opt(day).unwrap_or_default()
}

fn month_range(first: Option<&(i32, u32)>, last: Option<&(i32, u32)>) -> Vec<(i32, u32)> {
    let (Some(&first), Some(&last)) = (first, last) else {
        return Vec::new();
    };
    let mut months = Vec::new();
    let (mut y, mut m) = first;
    while (y, m) <= last {
        months.push((y, m));
        (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    }
    months
}

fn preview(text: &str) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(PREVIEW_CHARS).collect();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}

/// `https://www.Example.com:443/x` → `example.com`.
fn host_of(token: &str) -> Option<String> {
    let start = token
        .find("https://")
        .map(|i| i + 8)
        .or_else(|| token.find("http://").map(|i| i + 7))?;
    let rest = &token[start..];
    let host = rest
        .split(['/', '?', '#', '>', ')', ']', '"', '\''])
        .next()
        .unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    let host = host
        .strip_prefix("www.")
        .unwrap_or(&host)
        .trim_end_matches('.');
    (host.contains('.') && host.len() <= 253).then(|| host.to_owned())
}

fn extension_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let file = path.rsplit('/').next().unwrap_or(path);
    match file.rsplit_once('.') {
        Some((_, ext))
            if (1..=5).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            ext.to_ascii_lowercase()
        }
        _ => String::new(),
    }
}

fn file_kind(ext: &str) -> &'static str {
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "heif" | "svg" | "avif"
        | "tif" | "tiff" => "image",
        "mp4" | "mov" | "webm" | "mkv" | "avi" | "m4v" | "wmv" => "video",
        "mp3" | "ogg" | "wav" | "m4a" | "flac" | "opus" | "aac" | "wma" => "audio",
        "pdf" | "txt" | "doc" | "docx" | "odt" | "rtf" | "md" | "xls" | "xlsx" | "csv" | "ppt"
        | "pptx" | "json" | "log" => "document",
        "zip" | "rar" | "7z" | "tar" | "gz" | "xz" => "archive",
        _ => "other",
    }
}

/// Days since the start of the Discord epoch, for tests and the CLI.
pub fn local_now<Tz: TimeZone>(tz: &Tz) -> NaiveDate {
    Utc::now().with_timezone(tz).date_naive()
}

/// The last `days` days up to today, as a scope.
pub fn last_days<Tz: TimeZone>(tz: &Tz, days: i64) -> Scope {
    let today = local_now(tz);
    Scope {
        from: Some(today - Duration::days(days - 1)),
        to: Some(today),
        ..Scope::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PackageChannel;
    use chrono::FixedOffset;

    const OWNER: u64 = 1;
    const PERSON: u64 = 100_000_000_000_000_000;

    fn at(time: &str) -> Snowflake {
        let time = chrono::NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M").unwrap();
        Snowflake(Snowflake::from_datetime(Utc.from_utc_datetime(&time)).0 | 1)
    }

    fn message(time: &str, content: &str, attachments: &[&str]) -> PackageMessage {
        PackageMessage {
            id: at(time),
            content: content.into(),
            attachments: attachments.iter().map(|a| a.to_string()).collect(),
        }
    }

    fn channel(
        id: u64,
        kind: TargetKind,
        name: &str,
        guild: Option<(u64, &str)>,
        mut messages: Vec<PackageMessage>,
    ) -> PackageChannel {
        messages.sort_by_key(|m| std::cmp::Reverse(m.id));
        PackageChannel {
            id: Snowflake(id),
            kind,
            name: name.into(),
            guild: guild.map(|(id, name)| (Snowflake(id), name.into())),
            messages,
            recipients: if kind == TargetKind::Dm {
                vec![Snowflake(OWNER), Snowflake(id + PERSON)]
            } else {
                Vec::new()
            },
        }
    }

    fn index() -> Index {
        let package = Package {
            owner: Some(Snowflake(OWNER)),
            channels: vec![
                channel(
                    10,
                    TargetKind::Guild,
                    "general",
                    Some((100, "Rust")),
                    vec![
                        message("2024-01-01 09:00", "Hello world, the world is big", &[]),
                        message(
                            "2024-01-02 23:30",
                            "world again 😂😂 <:blob:123456789012345678>",
                            &[],
                        ),
                        message(
                            "2024-01-03 12:00",
                            "",
                            &["https://cdn.discordapp.com/a/1/cat.PNG?ex=1"],
                        ),
                        message(
                            "2024-01-10 12:00",
                            "see https://www.Example.com/x and http://github.com",
                            &[],
                        ),
                    ],
                ),
                channel(
                    11,
                    TargetKind::Guild,
                    "memes",
                    Some((100, "Rust")),
                    vec![message(
                        "2024-03-05 08:00",
                        "memes <@100000000000000012>",
                        &["https://cdn/x/clip.mp4"],
                    )],
                ),
                channel(
                    12,
                    TargetKind::Dm,
                    "alice",
                    None,
                    vec![
                        message("2024-01-02 10:00", "hi alice", &[]),
                        message("2024-02-20 10:00", "Hello again", &[]),
                    ],
                ),
            ],
        };
        // UTC+1: 23:30 UTC on 2 January is 00:30 on 3 January.
        Index::build(Arc::new(package), &FixedOffset::east_opt(3600).unwrap())
    }

    fn day(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn overview_counts_days_streaks_and_breaks() {
        let index = index();
        let o = index.overview(&Scope::default());
        assert_eq!(o.messages, 7);
        assert_eq!(o.attachments, 2);
        assert_eq!(o.without_text, 1);
        assert_eq!(o.links, 2);
        assert_eq!(o.places, 2);
        // 1, 2, 3 (twice, once only in local time) and 10 January,
        // 20 February, 5 March.
        assert_eq!(o.active_days, 6);
        let streak = o.longest_streak.unwrap();
        assert_eq!(
            (streak.from, streak.to, streak.days),
            (day("2024-01-01"), day("2024-01-03"), 3)
        );
        let pause = o.longest_break.unwrap();
        assert_eq!(
            (pause.from, pause.to),
            (day("2024-01-11"), day("2024-02-19"))
        );
        assert_eq!(o.busiest_day.unwrap().date, day("2024-01-03"));
        assert_eq!(o.first.unwrap().text, "Hello world, the world is big");
        assert_eq!(o.years.len(), 1);
        assert_eq!(o.years[0].top_place, Some(0));
    }

    #[test]
    fn scope_limits_dates_places_and_channels() {
        let index = index();
        let alice = index
            .info()
            .places
            .iter()
            .find(|p| p.name == "alice")
            .unwrap()
            .id;
        let only_alice = Scope {
            places: vec![alice],
            ..Scope::default()
        };
        assert_eq!(index.overview(&only_alice).messages, 2);
        let january = Scope {
            from: Some(day("2024-01-02")),
            to: Some(day("2024-01-03")),
            ..Scope::default()
        };
        assert_eq!(index.overview(&january).messages, 3);
        let memes = Scope {
            channels: vec![Snowflake(11)],
            places: vec![Snowflake(100)],
            ..Scope::default()
        };
        assert_eq!(index.overview(&memes).messages, 1);
    }

    #[test]
    fn timeline_fills_months_and_weeks() {
        let t = index().timeline(&Scope::default());
        let months: Vec<(&str, u64)> = t.months.iter().map(|m| (m.key.as_str(), m.total)).collect();
        assert_eq!(months, [("2024-01", 5), ("2024-02", 1), ("2024-03", 1)]);
        assert_eq!(t.series.len(), 2);
        assert_eq!(t.months[0].series, [4, 1]);
        assert_eq!(t.weeks.first().unwrap().key, "2024-01-01");
        assert!(t.weeks.windows(2).all(|w| w[0].key < w[1].key));
        // 3 January 2024 was a Wednesday; 00:30 local time.
        assert_eq!(t.week_hours[2][0], 1);
    }

    #[test]
    fn series_colours_do_not_depend_on_dates() {
        let index = index();
        let all = index.timeline(&Scope::default()).series;
        let later = index
            .timeline(&Scope {
                from: Some(day("2024-02-01")),
                ..Scope::default()
            })
            .series;
        let slots = |s: &[Series]| s.iter().map(|s| (s.place, s.slot)).collect::<Vec<_>>();
        assert_eq!(slots(&all), slots(&later));
    }

    #[test]
    fn places_and_channels() {
        let report = index().places(&Scope::default());
        assert_eq!(report.places[0].messages, 5);
        assert_eq!(report.places[0].monthly, [4, 0, 1]);
        let channels: Vec<(&str, u64)> = report
            .channels
            .iter()
            .map(|c| (c.name.as_str(), c.messages))
            .collect();
        assert_eq!(channels, [("general", 4), ("memes", 1)]);
    }

    #[test]
    fn words_emoji_and_mentions() {
        let report = index().words(&Scope::default());
        assert_eq!(report.words[0].key, "world");
        assert_eq!(report.words[0].count, 3);
        assert!(!report.words.iter().any(|w| w.key == "the" || w.key == "is"));
        assert!(!report.words.iter().any(|w| w.key.contains("example")));
        let laugh = report.emoji.iter().find(|e| e.emoji == "😂").unwrap();
        assert_eq!(laugh.count, 2);
        assert!(report
            .emoji
            .iter()
            .any(|e| e.emoji == "blob" && e.id.is_some()));
        assert_eq!(report.mentions[0].name.as_deref(), Some("alice"));
        assert_eq!(report.lengths.iter().sum::<u64>(), 6);
        assert!(report.longest.unwrap().text.starts_with("see https"));
    }

    #[test]
    fn links_and_files() {
        let report = index().links(&Scope::default());
        assert_eq!(report.links, 2);
        let domains: Vec<&str> = report.domains.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(domains, ["example.com", "github.com"]);
        assert_eq!(report.attachments, 2);
        let kinds: HashMap<&str, u64> = report
            .kinds
            .iter()
            .map(|k| (k.key.as_str(), k.count))
            .collect();
        assert_eq!((kinds["image"], kinds["video"]), (1, 1));
        assert_eq!(report.months, ["2024-01", "2024-02", "2024-03"]);
    }

    #[test]
    fn search_uses_the_clean_up_rule() {
        let index = index();
        let found = index.search(&Scope::default(), "HELLO again", 10);
        assert_eq!(found.total, 1);
        assert_eq!(found.messages[0].text, "Hello again");
        assert_eq!(index.search(&Scope::default(), "hello", 10).total, 2);
        assert_eq!(index.search(&Scope::default(), "   ", 10).total, 0);
    }
}
