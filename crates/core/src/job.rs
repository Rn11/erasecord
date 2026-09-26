//! A clean-up run: search each selected server or DM for the user's messages
//! in the chosen time range and delete them, strictly one at a time.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::client::{Client, Notice};
use crate::delete::{self, Outcome, SkipReason};
use crate::error::{Error, Result};
pub use crate::filter::Filter;
use crate::filter::Matcher;
use crate::models::Message;
use crate::search::SearchQuery;
use crate::snowflake::Snowflake;
use crate::targets::Target;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct JobOptions {
    /// Pause after each deletion.
    pub delete_delay_ms: u64,
    /// Pause between two search requests.
    pub search_delay_ms: u64,
    /// How often a server or DM is searched from the top again, to catch
    /// messages the search index returned late.
    pub max_rounds: u32,
    /// Only report what would be deleted.
    pub dry_run: bool,
}

impl Default for JobOptions {
    fn default() -> Self {
        JobOptions {
            delete_delay_ms: 1200,
            search_delay_ms: 2000,
            max_rounds: 3,
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    /// In a dry run: messages that would be deleted.
    pub deleted: u64,
    pub skipped: u64,
    pub failed: u64,
}

impl Stats {
    fn add(&mut self, other: Stats) {
        self.deleted += other.deleted;
        self.skipped += other.skipped;
        self.failed += other.failed;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub stats: Stats,
    pub cancelled: bool,
    /// Set when the job stopped early, e.g. because the token expired.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewEntry {
    pub target: Target,
    /// Discord's count of matching messages, including pinned ones.
    pub count: Option<u64>,
    pub error: Option<String>,
}

/// Progress of a running job.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    TargetStarted {
        index: usize,
        target_id: Snowflake,
        name: String,
    },
    /// Discord's estimate of matching messages, from the first search.
    TargetEstimate {
        target_id: Snowflake,
        total: u64,
    },
    Deleted {
        target_id: Snowflake,
        channel_id: Snowflake,
        message_id: Snowflake,
        sent_at: DateTime<Utc>,
        preview: String,
        dry_run: bool,
    },
    Skipped {
        target_id: Snowflake,
        message_id: Snowflake,
        reason: SkipReason,
    },
    Failed {
        target_id: Snowflake,
        message_id: Snowflake,
        error: String,
    },
    /// Searching this target failed; the job continues with the next one.
    TargetFailed {
        target_id: Snowflake,
        error: String,
    },
    TargetFinished {
        target_id: Snowflake,
        stats: Stats,
    },
    Notice {
        notice: Notice,
    },
    Finished(Summary),
}

/// Lets another task pause, resume or cancel a running job. Cheap to clone.
#[derive(Debug, Clone)]
pub struct JobControl {
    cancel: CancellationToken,
    paused: Arc<watch::Sender<bool>>,
}

impl Default for JobControl {
    fn default() -> Self {
        Self::new()
    }
}

impl JobControl {
    pub fn new() -> Self {
        JobControl {
            cancel: CancellationToken::new(),
            paused: Arc::new(watch::Sender::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    pub fn pause(&self) {
        self.paused.send_replace(true);
    }

    pub fn resume(&self) {
        self.paused.send_replace(false);
    }

    pub fn is_paused(&self) -> bool {
        *self.paused.borrow()
    }

    /// Returns once the job is not paused, or `Error::Cancelled`.
    async fn checkpoint(&self) -> Result<()> {
        let mut paused = self.paused.subscribe();
        loop {
            if self.cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if !*paused.borrow_and_update() {
                return Ok(());
            }
            tokio::select! {
                _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                _ = paused.changed() => {}
            }
        }
    }

    /// Runs `future` unless the job is cancelled first.
    async fn guard<T>(&self, future: impl Future<Output = T>) -> Result<T> {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Error::Cancelled),
            output = future => Ok(output),
        }
    }

    async fn sleep(&self, ms: u64) -> Result<()> {
        self.guard(tokio::time::sleep(Duration::from_millis(ms)))
            .await
    }
}

/// Asks Discord how many matching messages each target holds, without
/// deleting anything. `on_entry` is called after each target.
pub async fn preview(
    client: &Client,
    me: Snowflake,
    targets: &[Target],
    filter: &Filter,
    options: &JobOptions,
    control: &JobControl,
    mut on_entry: impl FnMut(usize, &PreviewEntry),
) -> Result<Vec<PreviewEntry>> {
    filter.compile()?;
    let query = filter.search_query(me);
    let mut entries = Vec::with_capacity(targets.len());
    for (index, target) in targets.iter().enumerate() {
        if index > 0 {
            control.sleep(options.search_delay_ms).await?;
        }
        control.checkpoint().await?;
        let (count, error) = match control.guard(client.search(target.scope(), &query)).await? {
            Ok(response) => (Some(response.total_results), None),
            Err(Error::Unauthorized) => return Err(Error::Unauthorized),
            Err(err) => (None, Some(err.to_string())),
        };
        let entry = PreviewEntry {
            target: target.clone(),
            count,
            error,
        };
        on_entry(index, &entry);
        entries.push(entry);
    }
    Ok(entries)
}

/// Deletes the matching messages in `targets`. Progress is reported through
/// `events`, ending with [`Event::Finished`].
pub async fn run(
    client: &Client,
    me: Snowflake,
    targets: &[Target],
    filter: &Filter,
    options: &JobOptions,
    control: &JobControl,
    events: mpsc::UnboundedSender<Event>,
) -> Summary {
    let matcher = match filter.compile() {
        Ok(matcher) => matcher,
        Err(err) => {
            let summary = Summary {
                stats: Stats::default(),
                cancelled: false,
                error: Some(err.to_string()),
            };
            let _ = events.send(Event::Finished(summary.clone()));
            return summary;
        }
    };
    let notices = events.clone();
    client.set_notice_sink(Some(Arc::new(move |notice| {
        let _ = notices.send(Event::Notice { notice });
    })));

    let job = Job {
        client,
        me,
        filter,
        options,
        control,
        events: &events,
        query: filter.search_query(me),
        matcher,
    };
    let mut total = Stats::default();
    let mut stopped_by = None;
    for (index, target) in targets.iter().enumerate() {
        job.emit(Event::TargetStarted {
            index,
            target_id: target.id,
            name: target.name.clone(),
        });
        let mut stats = Stats::default();
        let result = job.purge_target(target, &mut stats).await;
        total.add(stats);
        match result {
            Ok(()) => {}
            Err(err @ (Error::Unauthorized | Error::Cancelled)) => stopped_by = Some(err),
            Err(err) => job.emit(Event::TargetFailed {
                target_id: target.id,
                error: err.to_string(),
            }),
        }
        job.emit(Event::TargetFinished {
            target_id: target.id,
            stats,
        });
        if stopped_by.is_some() {
            break;
        }
    }
    client.set_notice_sink(None);

    let summary = Summary {
        stats: total,
        cancelled: matches!(stopped_by, Some(Error::Cancelled)),
        error: stopped_by
            .filter(|err| !matches!(err, Error::Cancelled))
            .map(|err| err.to_string()),
    };
    job.emit(Event::Finished(summary.clone()));
    summary
}

struct Job<'a> {
    client: &'a Client,
    me: Snowflake,
    filter: &'a Filter,
    options: &'a JobOptions,
    control: &'a JobControl,
    events: &'a mpsc::UnboundedSender<Event>,
    query: SearchQuery,
    matcher: Matcher,
}

impl Job<'_> {
    fn emit(&self, event: Event) {
        // The receiver only goes away when nobody watches the job any more.
        let _ = self.events.send(event);
    }

    /// Pages through the search results from newest to oldest, moving the
    /// `max_id` cursor below the oldest hit (no `offset`, which Discord caps).
    /// Then searches again from the top, because the index can lag behind;
    /// stops once a round turns up nothing new.
    async fn purge_target(&self, target: &Target, stats: &mut Stats) -> Result<()> {
        let scope = target.scope();
        let mut seen = HashSet::new();
        let mut searched = false;
        for _ in 0..self.options.max_rounds.max(1) {
            let mut cursor = self.query.max_id;
            let mut new_messages = 0;
            loop {
                if searched {
                    self.control.sleep(self.options.search_delay_ms).await?;
                }
                self.control.checkpoint().await?;
                let query = SearchQuery {
                    max_id: cursor,
                    ..self.query.clone()
                };
                let response = self
                    .control
                    .guard(self.client.search(scope, &query))
                    .await??;
                if !searched {
                    searched = true;
                    self.emit(Event::TargetEstimate {
                        target_id: target.id,
                        total: response.total_results,
                    });
                }

                let hits = response.into_hits();
                let Some(oldest) = hits.last().map(|m| m.id) else {
                    break;
                };
                for message in hits {
                    if seen.insert(message.id) {
                        new_messages += 1;
                        self.handle(target, message, stats).await?;
                    }
                }
                if oldest.0 == 0 {
                    break;
                }
                cursor = Some(Snowflake(oldest.0 - 1));
                if self.query.min_id.is_some_and(|min| oldest <= min) {
                    break;
                }
            }
            if new_messages == 0 || self.options.dry_run {
                break;
            }
        }
        Ok(())
    }

    async fn handle(&self, target: &Target, message: Message, stats: &mut Stats) -> Result<()> {
        // The search already filters by author and time. Check again so a
        // misbehaving index can never make us delete anything else.
        if message.author.id != self.me || !self.filter.contains(message.id) {
            return Ok(());
        }
        let skip = if !self.matcher.matches(&message) {
            Some(SkipReason::Excluded)
        } else if self.filter.skip_pinned && message.pinned {
            Some(SkipReason::Pinned)
        } else if !delete::is_deletable_kind(message.kind) {
            Some(SkipReason::SystemMessage)
        } else {
            None
        };
        if let Some(reason) = skip {
            stats.skipped += 1;
            self.skipped(target, &message, reason);
            return Ok(());
        }
        if self.options.dry_run {
            stats.deleted += 1;
            self.deleted(target, &message, true);
            return Ok(());
        }

        self.control.checkpoint().await?;
        let result = self
            .control
            .guard(self.client.delete_message(message.channel_id, message.id))
            .await?;
        match delete::classify(result)? {
            Outcome::Deleted | Outcome::AlreadyGone => {
                stats.deleted += 1;
                self.deleted(target, &message, false);
            }
            Outcome::Skipped(reason) => {
                stats.skipped += 1;
                self.skipped(target, &message, reason);
            }
            Outcome::Failed(err) => {
                stats.failed += 1;
                self.emit(Event::Failed {
                    target_id: target.id,
                    message_id: message.id,
                    error: err.to_string(),
                });
            }
        }
        self.control.sleep(self.options.delete_delay_ms).await
    }

    fn deleted(&self, target: &Target, message: &Message, dry_run: bool) {
        self.emit(Event::Deleted {
            target_id: target.id,
            channel_id: message.channel_id,
            message_id: message.id,
            sent_at: message.id.created_at(),
            preview: preview_text(message),
            dry_run,
        });
    }

    fn skipped(&self, target: &Target, message: &Message, reason: SkipReason) {
        self.emit(Event::Skipped {
            target_id: target.id,
            message_id: message.id,
            reason,
        });
    }
}

/// A one-line excerpt of a message for the progress log.
fn preview_text(message: &Message) -> String {
    const MAX_CHARS: usize = 100;
    let content = message
        .content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut text: String = content.chars().take(MAX_CHARS).collect();
    if content.chars().count() > MAX_CHARS {
        text.push('…');
    }
    match (text.is_empty(), message.attachments.len()) {
        (true, 0) => "(no text)".to_owned(),
        (true, n) => format!("({n} attachment{})", if n == 1 { "" } else { "s" }),
        (false, _) => text,
    }
}
