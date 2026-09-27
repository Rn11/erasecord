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
//! expire after [`CACHE_TTL`].

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Datelike, Local, Timelike, Utc};
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

/// How long found messages are kept.
pub const CACHE_TTL: Duration = Duration::from_secs(30 * 60);

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
    /// Where reading continues (the next `max_id`); `None` once complete.
    cursor: Option<Snowflake>,
    complete: bool,
    /// Discord's count from the first search.
    total: u64,
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
        let entry = entries.get(&target.id)?;
        if entry.at.elapsed() > CACHE_TTL {
            entries.remove(&target.id);
            return None;
        }
        let key = CacheKey::of(target.scope(), query);
        if entry.key != key
            || !close(entry.min_id, query.min_id)
            || !close(entry.max_id, query.max_id)
        {
            return None;
        }
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
    /// end). A page from the top starts a new entry; any other page must
    /// continue the entry where it left off.
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
        let continues = entries
            .get(&target.id)
            .is_some_and(|e| e.key == key && !e.complete && e.cursor == from);
        if !continues {
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
                    cursor: from,
                    complete: false,
                    total: total.unwrap_or(0),
                    at: Instant::now(),
                },
            );
        }
        let entry = entries.get_mut(&target.id).expect("entry just made");
        let oldest = entry.messages.last().map(|m| m.id);
        entry.messages.extend(
            hits.iter()
                .filter(|m| oldest.is_none_or(|o| m.id < o))
                .cloned(),
        );
        entry.cursor = next;
        entry.complete = next.is_none();
        if let Some(total) = total {
            entry.total = total;
        }
    }

    /// Forgets messages that were deleted.
    pub fn forget(&self, target_id: Snowflake, ids: &[Snowflake]) {
        if ids.is_empty() {
            return;
        }
        if let Some(entry) = self.entries.lock().unwrap().get_mut(&target_id) {
            entry.messages.retain(|m| !ids.contains(&m.id));
        }
    }

    /// Forgets everything about a server or DM.
    pub fn forget_target(&self, target_id: Snowflake) {
        self.entries.lock().unwrap().remove(&target_id);
    }
}

/// Searches one page for `target`, below `from`, and puts it in the cache.
/// Returns the hits and where the next page starts.
pub(crate) async fn search_page(
    client: &Client,
    cache: Option<&MessageCache>,
    target: &Target,
    base: &SearchQuery,
    from: Option<Snowflake>,
    control: &JobControl,
) -> Result<(Vec<Message>, Option<Snowflake>, u64)> {
    let query = SearchQuery {
        max_id: from,
        channel_ids: target.channels.clone(),
        ..base.clone()
    };
    let response = control
        .guard(client.search(target.scope(), &query))
        .await??;
    let total = response.total_results;
    let hits = response.into_hits();
    let next = match hits.last() {
        None => None,
        Some(oldest) if oldest.id.0 == 0 => None,
        Some(oldest) if base.min_id.is_some_and(|min| oldest.id <= min) => None,
        Some(oldest) => Some(Snowflake(oldest.id.0 - 1)),
    };
    if let Some(cache) = cache {
        let first = from == base.max_id;
        let base = SearchQuery {
            channel_ids: target.channels.clone(),
            ..base.clone()
        };
        cache.store(target, &base, from, &hits, next, first.then_some(total));
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
struct StatsBuilder {
    stats: ScanStats,
    months: BTreeMap<String, u64>,
    days: HashMap<String, u64>,
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
            .entry(local.format("%Y-%m").to_string())
            .or_default() += 1;
        *self
            .days
            .entry(local.format("%Y-%m-%d").to_string())
            .or_default() += 1;
        self.week[local.weekday().num_days_from_monday() as usize][local.hour() as usize] += 1;
        for token in message.content.split_whitespace() {
            if token.contains("://") || token.starts_with('<') {
                continue;
            }
            s.words += 1;
            for c in token.chars().filter(|&c| is_emoji(c)) {
                *self.emoji.entry(c.to_string()).or_default() += 1;
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

    fn snapshot(&self) -> ScanStats {
        let top = |map: &HashMap<String, u64>, n: usize| {
            let mut all: Vec<(String, u64)> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
            all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            all.truncate(n);
            all
        };
        let mut stats = self.stats.clone();
        stats.months = self.months.iter().map(|(k, v)| (k.clone(), *v)).collect();
        stats.week = self.week.iter().map(|row| row.to_vec()).collect();
        stats.top_words = top(&self.words, 24);
        stats.top_emoji = top(&self.emoji, 8);
        stats.busiest_day = top(&self.days, 1).into_iter().next();
        stats
    }
}

fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF)
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
    Finished {
        cancelled: bool,
        error: Option<String>,
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
    let mut searched = false;
    let mut builder = StatsBuilder::default();

    struct State {
        read: u64,
        matching: u64,
        cursor: Option<Snowflake>,
        complete: bool,
        failed: bool,
    }
    let wanted = |m: &Message| wanted(m, me, filter, &matcher);
    let mut states = Vec::with_capacity(targets.len());

    // First every count, so all numbers are there quickly.
    for target in targets {
        let query = SearchQuery {
            channel_ids: target.channels.clone(),
            ..base.clone()
        };
        if let Some(known) = cache.lookup(target, &query) {
            let mut matching = 0;
            for m in known.messages.iter().filter(|m| wanted(m)) {
                matching += 1;
                if filter.skip_pinned && m.pinned {
                    builder.stats.pinned_kept += 1;
                    matching -= 1;
                } else {
                    builder.add(m);
                }
            }
            on_event(ScanEvent::Counted {
                target_id: target.id,
                total: if known.complete {
                    matching
                } else {
                    known.total
                },
                error: None,
            });
            on_event(ScanEvent::Read {
                target_id: target.id,
                read: known.messages.len() as u64,
                matching,
                complete: known.complete,
            });
            states.push(State {
                read: known.messages.len() as u64,
                matching,
                cursor: known.cursor,
                complete: known.complete,
                failed: false,
            });
            continue;
        }
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
                let mut matching = 0;
                for m in hits.iter().filter(|m| wanted(m)) {
                    if filter.skip_pinned && m.pinned {
                        builder.stats.pinned_kept += 1;
                    } else {
                        matching += 1;
                        builder.add(m);
                    }
                }
                on_event(ScanEvent::Counted {
                    target_id: target.id,
                    total: if next.is_none() { matching } else { total },
                    error: None,
                });
                on_event(ScanEvent::Read {
                    target_id: target.id,
                    read: hits.len() as u64,
                    matching,
                    complete: next.is_none(),
                });
                states.push(State {
                    read: hits.len() as u64,
                    matching,
                    cursor: next,
                    complete: next.is_none(),
                    failed: false,
                });
            }
            Err(err @ (Error::Unauthorized | Error::Cancelled)) => return Err(err),
            Err(err) => {
                on_event(ScanEvent::Counted {
                    target_id: target.id,
                    total: 0,
                    error: Some(err.to_string()),
                });
                states.push(State {
                    read: 0,
                    matching: 0,
                    cursor: None,
                    complete: false,
                    failed: true,
                });
            }
        }
        on_event(ScanEvent::Stats {
            stats: builder.snapshot(),
        });
    }
    on_event(ScanEvent::Stats {
        stats: builder.snapshot(),
    });

    // Then read the rest, place by place.
    for (target, state) in targets.iter().zip(states.iter_mut()) {
        let mut page = 1;
        while !state.complete && !state.failed {
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
            match search_page(client, Some(cache), target, &base, state.cursor, control).await {
                Ok((hits, next, _)) => {
                    state.read += hits.len() as u64;
                    for m in hits.iter().filter(|m| wanted(m)) {
                        if filter.skip_pinned && m.pinned {
                            builder.stats.pinned_kept += 1;
                        } else {
                            state.matching += 1;
                            builder.add(m);
                        }
                    }
                    state.cursor = next;
                    state.complete = next.is_none();
                    on_event(ScanEvent::Read {
                        target_id: target.id,
                        read: state.read,
                        matching: state.matching,
                        complete: state.complete,
                    });
                    if state.complete {
                        // Now the count is exact.
                        on_event(ScanEvent::Counted {
                            target_id: target.id,
                            total: state.matching,
                            error: None,
                        });
                    }
                    on_event(ScanEvent::Stats {
                        stats: builder.snapshot(),
                    });
                }
                Err(err @ (Error::Unauthorized | Error::Cancelled)) => return Err(err),
                Err(err) => {
                    state.failed = true;
                    on_event(ScanEvent::Counted {
                        target_id: target.id,
                        total: state.matching,
                        error: Some(err.to_string()),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Statistics about the messages of a data package that match `filter`.
pub fn package_stats(
    package: &crate::package::Package,
    me: Snowflake,
    targets: &[Target],
    filter: &Filter,
) -> Result<ScanStats> {
    let matcher = filter.compile_for_package()?;
    let mut builder = StatsBuilder::default();
    for target in targets {
        for channel in package.channels_of(target) {
            for m in &channel.messages {
                if !filter.contains(m.id) {
                    continue;
                }
                let message = m.to_message(channel.id, me);
                if matcher.matches(&message) {
                    builder.add(&message);
                }
            }
        }
    }
    Ok(builder.snapshot())
}

/// Whether a search result would be dealt with (deleted, or kept because
/// it is pinned): the same checks as when deleting.
fn wanted(message: &Message, me: Snowflake, filter: &Filter, matcher: &Matcher) -> bool {
    message.author.id == me
        && filter.contains(message.id)
        && matcher.matches(message)
        && crate::delete::is_deletable_kind(message.kind)
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
        // A page that does not continue where the entry stopped is ignored.
        cache.store(
            &t,
            &query,
            Some(Snowflake(3 << 22)),
            &[message(2)],
            None,
            None,
        );
        assert!(!cache.lookup(&t, &query).unwrap().complete);
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
        cache.forget(t.id, &[Snowflake(3 << 22)]);
        assert_eq!(cache.lookup(&t, &query).unwrap().messages.len(), 9);
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
}
