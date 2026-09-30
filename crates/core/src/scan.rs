//! Finding the messages before deleting them, once.
//!
//! [`scan`] first asks Discord how many matching messages each server or DM
//! holds (one search each), then reads them page by page and keeps them in a
//! [`MessageCache`]. A dry run or a clean-up started afterwards takes the
//! messages from there instead of searching again, and only checks once
//! whether anything new turned up.
//!
//! The cache lives in memory only: nothing of it is ever written to disk. It
//! is emptied when the user logs out and when the app closes, and entries
//! expire when they have not been used for [`CACHE_TTL`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Local, NaiveDate, Timelike, Utc};
use serde::Serialize;

use crate::client::{Client, Notice};
use crate::error::{Error, Result};
use crate::filter::{Filter, Has, Matcher};
use crate::job::{Activity, JobControl, JobOptions};
use crate::models::Message;
use crate::pace::Pace;
use crate::search::{Scope, SearchQuery};
use crate::snowflake::Snowflake;
use crate::stopwords::is_stop_word;
use crate::targets::Target;

/// How long found messages are kept after they were last read or added to.
/// Counting a large account can itself take longer than half an hour, so the
/// time starts over whenever the entry is used.
pub const CACHE_TTL: Duration = Duration::from_secs(3 * 60 * 60);

/// A time bound that moved by less than this (e.g. "older than 30 days",
/// an hour later) still matches the cached search; the check for new
/// messages after deleting covers the difference.
const BOUND_TOLERANCE_MS: u64 = 60 * 60 * 1000;

/// What a search asked for, apart from where to start.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CacheKey {
    scope: Scope,
    author_id: Option<Snowflake>,
    content: Option<String>,
    has: Vec<Has>,
    channel_ids: Vec<Snowflake>,
}

impl CacheKey {
    fn of(scope: Scope, query: &SearchQuery) -> Self {
        CacheKey {
            scope,
            author_id: query.author_id,
            content: query.content.clone(),
            has: query.has.clone(),
            channel_ids: query.channel_ids.clone(),
        }
    }
}

#[derive(Debug)]
struct CacheEntry {
    key: CacheKey,
    min_id: Option<Snowflake>,
    max_id: Option<Snowflake>,
    /// Newest first.
    messages: Vec<Message>,
    ids: HashSet<Snowflake>,
    /// Deleted since they were found.
    gone: HashSet<Snowflake>,
    /// Where reading continues (the next `max_id`); `None` once complete.
    cursor: Option<Snowflake>,
    complete: bool,
    /// Discord's count from the first search.
    total: u64,
    /// Last read or added to.
    at: Instant,
}

/// Messages found by earlier searches, per server or DM. In memory only.
#[derive(Debug, Default, Clone)]
pub struct MessageCache {
    entries: Arc<Mutex<HashMap<Snowflake, CacheEntry>>>,
}

/// What the cache knows about a server or DM.
#[derive(Debug, Clone)]
pub struct Known {
    /// Newest first, within the requested time range.
    pub messages: Vec<Message>,
    pub complete: bool,
    /// Where reading continues if not complete.
    pub cursor: Option<Snowflake>,
    pub total: u64,
}

fn close(a: Option<Snowflake>, b: Option<Snowflake>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => (a.0 >> 22).abs_diff(b.0 >> 22) <= BOUND_TOLERANCE_MS,
        _ => false,
    }
}

impl CacheEntry {
    fn matches(&self, key: &CacheKey, query: &SearchQuery) -> bool {
        self.at.elapsed() <= CACHE_TTL
            && self.key == *key
            && close(self.min_id, query.min_id)
            && close(self.max_id, query.max_id)
    }

    /// Adds messages not known yet (and not deleted), keeping the newest
    /// first.
    fn merge(&mut self, hits: &[Message]) {
        let mut sorted = true;
        for hit in hits {
            if self.gone.contains(&hit.id) || !self.ids.insert(hit.id) {
                continue;
            }
            if self.messages.last().is_some_and(|last| hit.id > last.id) {
                sorted = false;
            }
            self.messages.push(hit.clone());
        }
        if !sorted {
            self.messages.sort_by_key(|m| std::cmp::Reverse(m.id));
        }
    }
}

impl MessageCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    /// The messages known for `target` and this search, if any.
    pub fn lookup(&self, target: &Target, query: &SearchQuery) -> Option<Known> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries.get_mut(&target.id)?;
        if !entry.matches(&CacheKey::of(target.scope(), query), query) {
            return None;
        }
        entry.at = Instant::now();
        let inside = |id: Snowflake| {
            query.min_id.is_none_or(|min| id > min) && query.max_id.is_none_or(|max| id < max)
        };
        Some(Known {
            messages: entry
                .messages
                .iter()
                .filter(|m| inside(m.id))
                .cloned()
                .collect(),
            complete: entry.complete,
            cursor: entry.cursor,
            total: entry.total,
        })
    }

    /// Adds a page of search results. `from` is the `max_id` the page was
    /// searched with, `next` where the following page starts (`None` at the
    /// end). A page from the top starts an entry if there is none; a page
    /// that continues the entry where it left off moves it on. Any other
    /// page (e.g. checking from the top for new messages) only adds what it
    /// found.
    fn store(
        &self,
        target: &Target,
        query: &SearchQuery,
        from: Option<Snowflake>,
        hits: &[Message],
        next: Option<Snowflake>,
        total: Option<u64>,
    ) {
        let key = CacheKey::of(target.scope(), query);
        let mut entries = self.entries.lock().unwrap();
        let known = entries
            .get(&target.id)
            .is_some_and(|e| e.matches(&key, query));
        if !known {
            if from != query.max_id {
                return;
            }
            entries.insert(
                target.id,
                CacheEntry {
                    key,
                    min_id: query.min_id,
                    max_id: query.max_id,
                    messages: Vec::new(),
                    ids: HashSet::new(),
                    gone: HashSet::new(),
                    cursor: from,
                    complete: false,
                    total: 0,
                    at: Instant::now(),
                },
            );
        }
        let entry = entries.get_mut(&target.id).expect("entry just made");
        entry.at = Instant::now();
        entry.merge(hits);
        if !entry.complete && entry.cursor == from {
            entry.cursor = next;
            entry.complete = next.is_none();
        }
        if let Some(total) = total.filter(|_| !known) {
            entry.total = total;
        }
    }

    /// Forgets messages that were deleted; searches that still return them
    /// (the index lags behind) do not bring them back.
    pub fn forget(&self, target_id: Snowflake, ids: &[Snowflake]) {
        if let Some(entry) = self.entries.lock().unwrap().get_mut(&target_id) {
            entry.messages.retain(|m| !ids.contains(&m.id));
            for id in ids {
                entry.ids.remove(id);
                entry.gone.insert(*id);
            }
        }
    }
}

/// Searches one page for `target`, below `from`, and puts it in the cache.
/// Returns the hits, where the next page starts (`None` at the end) and
/// Discord's count.
pub(crate) async fn search_page(
    client: &Client,
    cache: Option<&MessageCache>,
    target: &Target,
    base: &SearchQuery,
    from: Option<Snowflake>,
    control: &JobControl,
) -> Result<(Vec<Message>, Option<Snowflake>, u64)> {
    let base = SearchQuery {
        channel_ids: target.channels.clone(),
        ..base.clone()
    };
    let query = SearchQuery {
        max_id: from,
        ..base.clone()
    };
    let response = control
        .guard(client.search(target.scope(), &query))
        .await??;
    let total = response.total_results;
    let hits = response.into_hits();
    let next = hits
        .last()
        .map(|oldest| oldest.id)
        // The end: nothing older can exist, or the range ends here.
        .filter(|&oldest| oldest.0 > 0 && base.min_id.is_none_or(|min| oldest > min))
        // A search that ignored the cursor must not page forever.
        .filter(|&oldest| from.is_none_or(|from| oldest <= from))
        .map(|oldest| Snowflake(oldest.0 - 1));
    if let Some(cache) = cache {
        cache.store(target, &base, from, &hits, next, Some(total));
    }
    Ok((hits, next, total))
}

/// Statistics about the messages found so far.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanStats {
    pub messages: u64,
    pub words: u64,
    pub characters: u64,
    pub with_files: u64,
    pub images: u64,
    pub videos: u64,
    pub audio: u64,
    pub links: u64,
    /// Kept because pinned messages are to be kept.
    pub pinned_kept: u64,
    pub first: Option<DateTime<Utc>>,
    pub last: Option<DateTime<Utc>>,
    /// "YYYY-MM" and the number of messages, oldest first.
    pub months: Vec<(String, u64)>,
    /// Messages per weekday (Monday first) and hour, in local time.
    pub week: Vec<Vec<u32>>,
    pub top_words: Vec<(String, u64)>,
    pub top_emoji: Vec<(String, u64)>,
    /// The day with the most messages, and how many.
    pub busiest_day: Option<(String, u64)>,
    pub longest: u64,
}

#[derive(Default)]
pub(crate) struct StatsBuilder {
    stats: ScanStats,
    months: BTreeMap<(i32, u32), u64>,
    days: HashMap<NaiveDate, u64>,
    week: [[u32; 24]; 7],
    words: HashMap<String, u64>,
    emoji: HashMap<String, u64>,
}

impl StatsBuilder {
    fn add(&mut self, message: &Message) {
        let s = &mut self.stats;
        s.messages += 1;
        let chars = message.content.chars().count() as u64;
        s.characters += chars;
        s.longest = s.longest.max(chars);
        if !message.attachments.is_empty() {
            s.with_files += 1;
        }
        s.images += u64::from(Has::Image.found_in(message));
        s.videos += u64::from(Has::Video.found_in(message));
        s.audio += u64::from(Has::Sound.found_in(message));
        s.links += u64::from(Has::Link.found_in(message));
        let sent = message.id.created_at();
        s.first = Some(s.first.map_or(sent, |f| f.min(sent)));
        s.last = Some(s.last.map_or(sent, |l| l.max(sent)));
        let local = sent.with_timezone(&Local);
        *self
            .months
            .entry((local.year(), local.month()))
            .or_default() += 1;
        *self.days.entry(local.date_naive()).or_default() += 1;
        self.week[local.weekday().num_days_from_monday() as usize][local.hour() as usize] += 1;
        for token in message.content.split_whitespace() {
            if token.contains("://") || token.starts_with('<') {
                continue;
            }
            s.words += 1;
            if !token.is_ascii() {
                for emoji in crate::insights::emoji_in(token) {
                    *self.emoji.entry(emoji.to_owned()).or_default() += 1;
                }
            }
            let word: String = token
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'')
                .collect::<String>()
                .to_lowercase();
            let word = word.trim_matches('\'');
            if word.chars().count() >= 3
                && !word.chars().all(|c| c.is_ascii_digit())
                && !is_stop_word(word)
            {
                *self.words.entry(word.to_owned()).or_default() += 1;
            }
        }
    }

    /// Adds what another builder counted.
    fn merge(&mut self, other: StatsBuilder) {
        let (a, b) = (&mut self.stats, other.stats);
        a.messages += b.messages;
        a.words += b.words;
        a.characters += b.characters;
        a.with_files += b.with_files;
        a.images += b.images;
        a.videos += b.videos;
        a.audio += b.audio;
        a.links += b.links;
        a.pinned_kept += b.pinned_kept;
        a.longest = a.longest.max(b.longest);
        a.first = a.first.min(b.first).or(a.first).or(b.first);
        a.last = a.last.max(b.last);
        for (k, v) in other.months {
            *self.months.entry(k).or_default() += v;
        }
        for (k, v) in other.days {
            *self.days.entry(k).or_default() += v;
        }
        for (row, other_row) in self.week.iter_mut().zip(other.week) {
            for (cell, n) in row.iter_mut().zip(other_row) {
                *cell += n;
            }
        }
        for (k, v) in other.words {
            *self.words.entry(k).or_default() += v;
        }
        for (k, v) in other.emoji {
            *self.emoji.entry(k).or_default() += v;
        }
    }

    fn snapshot(&self) -> ScanStats {
        let top = |map: &HashMap<String, u64>, n: usize| {
            let mut all: Vec<(String, u64)> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
            all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            all.truncate(n);
            all
        };
        let mut stats = self.stats.clone();
        stats.months = self
            .months
            .iter()
            .map(|((y, m), n)| (format!("{y:04}-{m:02}"), *n))
            .collect();
        stats.week = self.week.iter().map(|row| row.to_vec()).collect();
        stats.top_words = top(&self.words, 24);
        stats.top_emoji = top(&self.emoji, 8);
        stats.busiest_day = self
            .days
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(day, n)| (day.format("%Y-%m-%d").to_string(), *n));
        stats
    }
}

/// Progress of a [`scan`].
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScanEvent {
    /// Discord's count for a server or DM, from its first search (or the
    /// exact number, when every message was already read).
    Counted {
        target_id: Snowflake,
        total: u64,
        error: Option<String>,
    },
    /// Messages read so far; `matching` is how many would be deleted.
    Read {
        target_id: Snowflake,
        read: u64,
        matching: u64,
        complete: bool,
    },
    Activity {
        activity: Activity,
    },
    Notice {
        notice: Notice,
    },
    Stats {
        stats: ScanStats,
    },
}

/// Counts, then reads the matching messages of every target into `cache`,
/// reporting progress and statistics. Stops early when cancelled, keeping
/// what was read.
#[allow(clippy::too_many_arguments)]
pub async fn scan(
    client: &Client,
    me: Snowflake,
    targets: &[Target],
    filter: &Filter,
    options: &JobOptions,
    control: &JobControl,
    cache: &MessageCache,
    mut on_event: impl FnMut(ScanEvent),
) -> Result<()> {
    let matcher = filter.compile()?;
    let base = filter.search_query(me);
    let pace = Pace::new(options.delete_delay_ms, options.search_delay_ms);
    let targets = &crate::targets::with_threads(client, targets, &base, &pace, control).await;
    let mut builder = StatsBuilder::default();
    let mut searched = false;

    /// Where reading a server or DM stands.
    #[derive(Default)]
    struct Place {
        read: u64,
        matching: u64,
        cursor: Option<Snowflake>,
        complete: bool,
        failed: bool,
    }
    // Counts the messages that would be deleted and adds them to the
    // statistics.
    let tally = |place: &mut Place, messages: &[Message], builder: &mut StatsBuilder| {
        place.read += messages.len() as u64;
        for m in messages.iter().filter(|m| wanted(m, me, filter, &matcher)) {
            if filter.skip_pinned && m.pinned {
                builder.stats.pinned_kept += 1;
            } else {
                place.matching += 1;
                builder.add(m);
            }
        }
    };
    let read_event = |target: &Target, place: &Place| ScanEvent::Read {
        target_id: target.id,
        read: place.read,
        matching: place.matching,
        complete: place.complete,
    };

    // First every count, so all numbers are there quickly.
    let mut places = Vec::with_capacity(targets.len());
    for target in targets {
        let mut place = Place::default();
        let query = SearchQuery {
            channel_ids: target.channels.clone(),
            ..base.clone()
        };
        let total = if let Some(known) = cache.lookup(target, &query) {
            tally(&mut place, &known.messages, &mut builder);
            place.cursor = known.cursor;
            place.complete = known.complete;
            Ok(known.total)
        } else {
            if searched {
                control.sleep_for(pace.before_search()).await?;
            }
            control.checkpoint().await?;
            searched = true;
            on_event(ScanEvent::Activity {
                activity: Activity::Counting {
                    target_id: target.id,
                },
            });
            match search_page(client, Some(cache), target, &base, base.max_id, control).await {
                Ok((hits, next, total)) => {
                    tally(&mut place, &hits, &mut builder);
                    place.cursor = next;
                    place.complete = next.is_none();
                    Ok(total)
                }
                Err(err @ (Error::Unauthorized | Error::Cancelled)) => return Err(err),
                Err(err) => Err(err.to_string()),
            }
        };
        match total {
            Ok(total) => {
                on_event(ScanEvent::Counted {
                    target_id: target.id,
                    // Once everything is read, the count is exact.
                    total: if place.complete {
                        place.matching
                    } else {
                        total
                    },
                    error: None,
                });
                on_event(read_event(target, &place));
            }
            Err(error) => {
                place.failed = true;
                on_event(ScanEvent::Counted {
                    target_id: target.id,
                    total: 0,
                    error: Some(error),
                });
            }
        }
        on_event(ScanEvent::Stats {
            stats: builder.snapshot(),
        });
        places.push(place);
    }

    // Then read the rest, place by place.
    for (target, place) in targets.iter().zip(places.iter_mut()) {
        let mut page = 1;
        while !place.complete && !place.failed {
            if searched {
                control.sleep_for(pace.before_search()).await?;
            }
            control.checkpoint().await?;
            searched = true;
            page += 1;
            on_event(ScanEvent::Activity {
                activity: Activity::Reading {
                    target_id: target.id,
                    page,
                },
            });
            match search_page(client, Some(cache), target, &base, place.cursor, control).await {
                Ok((hits, next, _)) => {
                    tally(place, &hits, &mut builder);
                    place.cursor = next;
                    place.complete = next.is_none();
                    on_event(read_event(target, place));
                    if place.complete {
                        on_event(ScanEvent::Counted {
                            target_id: target.id,
                            total: place.matching,
                            error: None,
                        });
                    }
                    on_event(ScanEvent::Stats {
                        stats: builder.snapshot(),
                    });
                }
                Err(err @ (Error::Unauthorized | Error::Cancelled)) => return Err(err),
                Err(err) => {
                    place.failed = true;
                    on_event(ScanEvent::Counted {
                        target_id: target.id,
                        total: place.matching,
                        error: Some(err.to_string()),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Statistics about the messages of a data package that match `filter`,
/// counted on all cores.
pub fn package_stats(
    package: &crate::package::Package,
    me: Snowflake,
    targets: &[Target],
    filter: &Filter,
) -> Result<ScanStats> {
    let matcher = filter.compile_for_package()?;
    let work: Vec<(Snowflake, &crate::package::PackageMessage)> = targets
        .iter()
        .flat_map(|target| package.channels_of(target))
        .flat_map(|channel| channel.messages.iter().map(move |m| (channel.id, m)))
        .filter(|(_, m)| filter.contains(m.id))
        .collect();
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
    let chunk = work.len().div_ceil(workers).max(1);
    let parts: Vec<StatsBuilder> = std::thread::scope(|scope| {
        let handles: Vec<_> = work
            .chunks(chunk)
            .map(|part| {
                let matcher = &matcher;
                scope.spawn(move || {
                    let mut builder = StatsBuilder::default();
                    for (channel, m) in part {
                        let message = m.to_message(*channel, me);
                        if matcher.matches(&message) {
                            builder.add(&message);
                        }
                    }
                    builder
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("statistics worker"))
            .collect()
    });
    let mut total = StatsBuilder::default();
    for part in parts {
        total.merge(part);
    }
    Ok(total.snapshot())
}

/// Whether a search result would be dealt with (deleted, or kept because
/// it is pinned): the same checks as when deleting.
fn wanted(message: &Message, me: Snowflake, filter: &Filter, matcher: &Matcher) -> bool {
    message.author.id == me
        && filter.contains(message.id)
        && matcher.matches(message)
        && crate::delete::is_deletable_kind(message.kind)
}

/// Lets other tests build statistics.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    pub(crate) fn builder() -> StatsBuilder {
        StatsBuilder::default()
    }

    pub(crate) fn add(builder: &mut StatsBuilder, message: &Message) {
        builder.add(message);
    }

    pub(crate) fn snapshot(builder: &StatsBuilder) -> ScanStats {
        builder.snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::User;

    fn message(id: u64) -> Message {
        Message {
            id: Snowflake(id << 22),
            channel_id: Snowflake(1),
            kind: 0,
            content: format!("hello world number {id} 😀"),
            author: User {
                id: Snowflake(7),
                username: "me".into(),
                global_name: None,
                avatar: None,
            },
            pinned: false,
            attachments: vec![],
            embeds: vec![],
            sticker_items: vec![],
            hit: None,
        }
    }

    fn target() -> Target {
        serde_json::from_value(serde_json::json!({
            "id": "5", "kind": "dm", "name": "Alice", "icon_url": null, "channels": []
        }))
        .unwrap()
    }

    #[test]
    fn pages_are_kept_in_order_and_complete_at_the_end() {
        let cache = MessageCache::new();
        let t = target();
        let query = SearchQuery {
            author_id: Some(Snowflake(7)),
            ..Default::default()
        };
        let page1: Vec<Message> = (5..=10).rev().map(message).collect();
        cache.store(
            &t,
            &query,
            None,
            &page1,
            Some(Snowflake((5 << 22) - 1)),
            Some(10),
        );
        let known = cache.lookup(&t, &query).unwrap();
        assert_eq!(known.messages.len(), 6);
        assert!(!known.complete);
        assert_eq!(known.total, 10);
        // A page that does not continue where the entry stopped adds its
        // messages, but reading still continues where it left off.
        cache.store(
            &t,
            &query,
            Some(Snowflake(3 << 22)),
            &[message(2)],
            None,
            None,
        );
        let known = cache.lookup(&t, &query).unwrap();
        assert!(!known.complete);
        assert_eq!(known.cursor, Some(Snowflake((5 << 22) - 1)));
        let page2: Vec<Message> = (1..=4).rev().map(message).collect();
        cache.store(
            &t,
            &query,
            Some(Snowflake((5 << 22) - 1)),
            &page2,
            None,
            None,
        );
        let known = cache.lookup(&t, &query).unwrap();
        assert!(known.complete);
        assert_eq!(known.messages.len(), 10);
        assert!(known.messages.windows(2).all(|w| w[0].id > w[1].id));
        cache.forget(t.id, &[Snowflake(3 << 22)]);
        assert_eq!(cache.lookup(&t, &query).unwrap().messages.len(), 9);
        // Checking from the top again: a deleted message the index still
        // returns stays gone, a new one is added, and the entry stays complete.
        cache.store(
            &t,
            &query,
            None,
            &[message(11), message(3)],
            Some(Snowflake(1)),
            Some(99),
        );
        let known = cache.lookup(&t, &query).unwrap();
        assert!(known.complete);
        assert_eq!(known.total, 10);
        assert_eq!(known.messages.len(), 10);
        assert_eq!(known.messages[0].id, Snowflake(11 << 22));
        assert!(!known.messages.iter().any(|m| m.id == Snowflake(3 << 22)));
        // Another search does not use it.
        let other = SearchQuery {
            content: Some("x".into()),
            ..query.clone()
        };
        assert!(cache.lookup(&t, &other).is_none());
    }

    #[test]
    fn a_slightly_moved_time_bound_still_matches() {
        let cache = MessageCache::new();
        let t = target();
        let at = |ms: u64| Some(Snowflake(ms << 22));
        let query = SearchQuery {
            max_id: at(10_000_000),
            ..Default::default()
        };
        cache.store(&t, &query, query.max_id, &[message(5)], None, Some(1));
        let later = SearchQuery {
            max_id: at(10_000_000 + 60_000),
            ..Default::default()
        };
        assert!(cache.lookup(&t, &later).is_some());
        let much_later = SearchQuery {
            max_id: at(10_000_000 + 2 * BOUND_TOLERANCE_MS),
            ..Default::default()
        };
        assert!(cache.lookup(&t, &much_later).is_none());
    }

    #[test]
    fn merged_statistics_equal_counting_in_one_go() {
        let messages: Vec<Message> = (0..300u64)
            .map(|i| {
                let mut m = message(1_600_000_000_000 - 1_420_070_400_000 + i * 7_919_000_000);
                m.content = format!(
                    "word{} shared 🎉 https://x.y {}",
                    i % 13,
                    "a".repeat((i % 50) as usize)
                );
                m
            })
            .collect();
        let mut whole = StatsBuilder::default();
        for m in &messages {
            whole.add(m);
        }
        let mut merged = StatsBuilder::default();
        for part in messages.chunks(37) {
            let mut builder = StatsBuilder::default();
            for m in part {
                builder.add(m);
            }
            merged.merge(builder);
        }
        let (a, b) = (whole.snapshot(), merged.snapshot());
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            serde_json::to_value(&b).unwrap()
        );
        assert!(a.first < a.last);
    }

    #[test]
    fn statistics_count_words_emoji_and_months() {
        let mut builder = StatsBuilder::default();
        for id in 1..=3 {
            builder.add(&message(1_700_000_000_000 - 1_420_070_400_000 + id));
        }
        let stats = builder.snapshot();
        assert_eq!(stats.messages, 3);
        assert_eq!(stats.top_words[0], ("hello".to_owned(), 3));
        assert_eq!(stats.top_emoji[0], ("😀".to_owned(), 3));
        assert_eq!(stats.months.len(), 1);
        assert_eq!(stats.week.iter().flatten().sum::<u32>(), 3);
    }

    #[test]
    fn statistics_keep_emoji_whole() {
        let mut builder = StatsBuilder::default();
        let mut m = message(1);
        m.content = "🫠🩷 👍🏽 hi👩‍💻 🇩🇪 ❤️ 1️⃣".into();
        builder.add(&m);
        let mut emoji: Vec<String> = builder
            .snapshot()
            .top_emoji
            .into_iter()
            .map(|(e, _)| e)
            .collect();
        emoji.sort();
        let mut expected = ["🫠", "🩷", "👍🏽", "👩‍💻", "🇩🇪", "❤️"].map(str::to_owned);
        expected.sort();
        assert_eq!(emoji, expected);
    }
}
