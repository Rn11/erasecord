//! A clean-up run: search each selected server or DM for the user's messages
//! in the chosen time range and delete them, strictly one at a time.

use std::collections::HashSet;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::backup::Backup;
use crate::client::{Client, Notice};
use crate::delete::{self, Outcome, SkipReason};
use crate::error::{Error, Result};
pub use crate::filter::Filter;
use crate::filter::{Has, Matcher};
use crate::models::Message;
use crate::pace::Pace;
use crate::package::Package;
use crate::resume::Checkpoint;
use crate::scan::MessageCache;
use crate::search::SearchQuery;
use crate::snowflake::Snowflake;
use crate::targets::{Target, TargetKind};
use crate::vault::{EncryptedBackup, EncryptedBackupSettings, KeySlot};

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
    /// Before deleting a message, replace its text with this and remove its
    /// attachments, so the original is gone even where Discord keeps deleted
    /// messages around. An empty text means random letters.
    pub overwrite: Option<String>,
    /// Before deleting a message, save its attachments into this folder. A
    /// message whose attachments cannot be saved is not deleted. Also done in
    /// a dry run, which then backs up without deleting anything.
    pub backup_dir: Option<PathBuf>,
    /// Write the backup encrypted instead, into these parts (see
    /// [`crate::vault`]); `backup_dir` is then only where it lives.
    pub backup_encryption: Option<EncryptedBackupSettings>,
    /// The keys for finishing the encrypted backup. Never saved.
    #[serde(skip)]
    pub backup_keys: KeySlot,
    /// With a backup: also save the attachments the others sent in the
    /// chosen time range, in DMs and group DMs only (never on a server).
    /// Their messages are not touched.
    pub backup_others: bool,
    /// Continue an earlier run: skip what it finished and dealt with.
    pub resume: Option<Checkpoint>,
}

impl Default for JobOptions {
    fn default() -> Self {
        JobOptions {
            // Well below Discord's limits; see crate::pace.
            delete_delay_ms: 2500,
            search_delay_ms: 3000,
            max_rounds: 3,
            dry_run: false,
            overwrite: None,
            backup_dir: None,
            backup_encryption: None,
            backup_keys: KeySlot::default(),
            backup_others: false,
            resume: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    /// In a dry run: messages that would be deleted.
    pub deleted: u64,
    pub skipped: u64,
    pub failed: u64,
    /// Messages of others whose attachments were saved.
    #[serde(default)]
    pub saved_from_others: u64,
}

impl Stats {
    fn add(&mut self, other: Stats) {
        self.deleted += other.deleted;
        self.skipped += other.skipped;
        self.failed += other.failed;
        self.saved_from_others += other.saved_from_others;
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
        /// The full text, for exports.
        content: String,
        /// Attachment URLs, for exports.
        attachments: Vec<String>,
        /// Where the attachments were saved, relative to the backup folder.
        saved: Vec<String>,
        dry_run: bool,
    },
    Skipped {
        target_id: Snowflake,
        message_id: Snowflake,
        reason: SkipReason,
    },
    /// The attachments of someone else's message were saved (see
    /// [`JobOptions::backup_others`]); the message itself stays.
    SavedFromOthers {
        target_id: Snowflake,
        channel_id: Snowflake,
        message_id: Snowflake,
        sent_at: DateTime<Utc>,
        author: String,
        content: String,
        attachments: Vec<String>,
        saved: Vec<String>,
    },
    /// Someone else's attachments could not be saved.
    NotSavedFromOthers {
        target_id: Snowflake,
        message_id: Snowflake,
        error: String,
    },
    Failed {
        target_id: Snowflake,
        message_id: Snowflake,
        error: String,
    },
    /// A channel from the data package cannot be reached (deleted, or no
    /// longer accessible); its messages are skipped.
    ChannelUnreachable {
        target_id: Snowflake,
        channel_id: Snowflake,
        messages: u64,
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
        /// False when the target was interrupted or could not be searched.
        complete: bool,
    },
    /// The encrypted backup was combined into one archive.
    BackupSealed {
        archive: PathBuf,
        files: u64,
        messages: u64,
    },
    /// The encrypted backup stays in parts, e.g. because the run was stopped;
    /// a continued run finishes it.
    BackupKept {
        folder: PathBuf,
        reason: String,
    },
    Notice {
        notice: Notice,
    },
    /// What the job is doing right now, for a status line.
    Activity {
        activity: Activity,
    },
    Finished(Summary),
}

/// What a job or a scan is busy with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Activity {
    /// Asking how many messages a server or DM holds.
    Counting {
        target_id: Snowflake,
    },
    /// Reading messages page by page.
    Reading {
        target_id: Snowflake,
        page: u32,
    },
    /// Searching for messages to delete (`again`: checking for messages
    /// the search returned late).
    Searching {
        target_id: Snowflake,
        page: u32,
        again: bool,
    },
    /// Using the messages found while counting.
    UsingFound {
        target_id: Snowflake,
        messages: u64,
    },
    Deleting {
        target_id: Snowflake,
    },
    BackingUp {
        target_id: Snowflake,
    },
    /// Looking for attachments the others sent, page by page.
    SearchingOthers {
        target_id: Snowflake,
        page: u32,
    },
    CheckingChannel {
        target_id: Snowflake,
    },
    /// A longer break, to go easy on Discord.
    Break {
        ms: u64,
    },
    SealingBackup,
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
    pub(crate) async fn checkpoint(&self) -> Result<()> {
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
    pub(crate) async fn guard<T>(&self, future: impl Future<Output = T>) -> Result<T> {
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(Error::Cancelled),
            output = future => Ok(output),
        }
    }

    pub(crate) async fn sleep_for(&self, duration: Duration) -> Result<()> {
        if duration.is_zero() {
            return Ok(());
        }
        self.guard(tokio::time::sleep(duration)).await
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
    let pace = Pace::new(options.delete_delay_ms, options.search_delay_ms);
    let mut entries = Vec::with_capacity(targets.len());
    for (index, target) in targets.iter().enumerate() {
        if index > 0 {
            control.sleep_for(pace.before_search()).await?;
        }
        control.checkpoint().await?;
        let query = SearchQuery {
            channel_ids: target.channels.clone(),
            ..query.clone()
        };
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

/// Counts the matching messages of each target in a data package. Exact,
/// except that pinned messages are only known while deleting.
pub fn preview_package(
    package: &Package,
    me: Snowflake,
    targets: &[Target],
    filter: &Filter,
) -> Result<Vec<PreviewEntry>> {
    let matcher = filter.compile_for_package()?;
    Ok(targets
        .iter()
        .map(|target| {
            let count = package
                .channels_of(target)
                .map(|channel| {
                    channel
                        .messages
                        .iter()
                        .filter(|m| {
                            filter.contains(m.id) && matcher.matches(&m.to_message(channel.id, me))
                        })
                        .count() as u64
                })
                .sum();
            PreviewEntry {
                target: target.clone(),
                count: Some(count),
                error: None,
            }
        })
        .collect())
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
    run_with_cache(client, me, targets, filter, options, control, None, events).await
}

/// Like [`run`], but first deletes what an earlier [`crate::scan::scan`]
/// (or dry run) found and put in `cache`, then only checks for anything
/// new. Messages found while searching are added to the cache, deleted ones
/// removed from it.
#[allow(clippy::too_many_arguments)]
pub async fn run_with_cache(
    client: &Client,
    me: Snowflake,
    targets: &[Target],
    filter: &Filter,
    options: &JobOptions,
    control: &JobControl,
    cache: Option<&MessageCache>,
    events: mpsc::UnboundedSender<Event>,
) -> Summary {
    run_from(
        Source::Search(cache),
        client,
        me,
        targets,
        filter,
        options,
        control,
        events,
    )
    .await
}

/// Like [`run`], but takes the messages from a data package instead of
/// searching for them. `targets` come from [`Package::targets`].
#[allow(clippy::too_many_arguments)]
pub async fn run_package(
    client: &Client,
    me: Snowflake,
    package: &Package,
    targets: &[Target],
    filter: &Filter,
    options: &JobOptions,
    control: &JobControl,
    events: mpsc::UnboundedSender<Event>,
) -> Summary {
    run_from(
        Source::Package(package),
        client,
        me,
        targets,
        filter,
        options,
        control,
        events,
    )
    .await
}

#[derive(Clone, Copy)]
enum Source<'a> {
    Search(Option<&'a MessageCache>),
    Package(&'a Package),
}

#[allow(clippy::too_many_arguments)]
async fn run_from(
    source: Source<'_>,
    client: &Client,
    me: Snowflake,
    targets: &[Target],
    filter: &Filter,
    options: &JobOptions,
    control: &JobControl,
    events: mpsc::UnboundedSender<Event>,
) -> Summary {
    let compiled = match source {
        Source::Search(_) => filter.compile(),
        Source::Package(package) => package
            .check_owner(me)
            .and_then(|()| filter.compile_for_package()),
    };
    let matcher = match compiled {
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
    let pace = Arc::new(Pace::new(options.delete_delay_ms, options.search_delay_ms));
    let mut vault = match options
        .backup_encryption
        .as_ref()
        .map(|settings| EncryptedBackup::open(settings, options.backup_keys.0.clone()))
        .transpose()
    {
        Ok(vault) => vault,
        Err(err) => {
            let summary = Summary {
                stats: Stats::default(),
                cancelled: false,
                error: Some(format!("cannot write the encrypted backup: {err}")),
            };
            let _ = events.send(Event::Finished(summary.clone()));
            return summary;
        }
    };
    let plain_dir = if vault.is_some() {
        None
    } else {
        options.backup_dir.as_deref()
    };
    let mut backup = match plain_dir.map(Backup::new).transpose() {
        Ok(backup) => backup,
        Err(err) => {
            let summary = Summary {
                stats: Stats::default(),
                cancelled: false,
                error: Some(format!("cannot use the backup folder: {err}")),
            };
            let _ = events.send(Event::Finished(summary.clone()));
            return summary;
        }
    };
    if let Some(vault) = &mut vault {
        vault.set_pace(pace.clone());
    }
    if let Some(backup) = &mut backup {
        backup.set_pace(pace.clone());
    }
    let notices = events.clone();
    let slow = pace.clone();
    client.set_notice_sink(Some(Arc::new(move |notice| {
        if matches!(notice, Notice::RateLimited { .. }) {
            slow.slow_down();
        }
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
        backup,
        vault: vault.as_ref(),
        pending: Mutex::new(Vec::new()),
        pace,
        cache: match source {
            Source::Search(cache) => cache,
            Source::Package(_) => None,
        },
    };
    let mut total = Stats::default();
    let mut stopped_by = None;
    // Every server and DM was done completely; otherwise the run can be
    // continued, and an encrypted backup stays open for it.
    let mut all_complete = true;
    for (index, target) in targets.iter().enumerate() {
        if job.resume().is_some_and(|r| r.is_finished(target.id)) {
            continue;
        }
        job.emit(Event::TargetStarted {
            index,
            target_id: target.id,
            name: target.name.clone(),
        });
        let mut stats = Stats::default();
        let mut result = match source {
            Source::Search(_) => job.purge_target(target, &mut stats).await,
            Source::Package(package) => job.purge_known(target, package, &mut stats).await,
        };
        // Messages waiting for their part of the encrypted backup.
        if result.is_ok() {
            result = job.flush(target, &mut stats).await;
        } else {
            job.abandon_pending();
        }
        if result.is_ok() && options.backup_others {
            result = job.save_from_others(target, &mut stats).await;
        }
        total.add(stats);
        let complete = result.is_ok();
        all_complete &= complete;
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
            complete,
        });
        if stopped_by.is_some() {
            break;
        }
    }
    client.set_notice_sink(None);
    drop(job);

    if let Some(vault) = vault {
        let _ = events.send(Event::Activity {
            activity: Activity::SealingBackup,
        });
        let finished = stopped_by.is_none() && all_complete;
        let reason = if stopped_by.is_some() {
            "the clean-up was stopped; continue it to finish the backup"
        } else {
            "not every server or DM could be searched; continue the clean-up to finish the backup"
        };
        let outcome = tokio::task::spawn_blocking(move || {
            if finished {
                vault.seal().map(Ok)
            } else {
                vault.suspend().map(Err)
            }
        })
        .await;
        let event = match outcome {
            Ok(Ok(Ok(sealed))) => Event::BackupSealed {
                archive: sealed.archive,
                files: sealed.files,
                messages: sealed.messages,
            },
            Ok(Ok(Err(folder))) => Event::BackupKept {
                folder,
                reason: reason.into(),
            },
            Ok(Err(reason)) => Event::BackupKept {
                folder: options
                    .backup_encryption
                    .as_ref()
                    .map(|s| s.parts.clone())
                    .unwrap_or_default(),
                reason,
            },
            Err(err) => Event::BackupKept {
                folder: PathBuf::new(),
                reason: err.to_string(),
            },
        };
        let _ = events.send(event);
    }

    let summary = Summary {
        stats: total,
        cancelled: matches!(stopped_by, Some(Error::Cancelled)),
        error: stopped_by
            .filter(|err| !matches!(err, Error::Cancelled))
            .map(|err| err.to_string()),
    };
    let _ = events.send(Event::Finished(summary.clone()));
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
    backup: Option<Backup>,
    vault: Option<&'a EncryptedBackup>,
    /// Backed-up messages waiting for their part to be sealed before they
    /// are deleted.
    pending: Mutex<Vec<(Message, Vec<String>)>>,
    pace: Arc<Pace>,
    /// Messages found earlier, and where found ones are kept.
    cache: Option<&'a MessageCache>,
}

impl Job<'_> {
    fn resume(&self) -> Option<&Checkpoint> {
        self.options.resume.as_ref()
    }

    /// Dealt with by the run being continued.
    fn already_done(&self, id: Snowflake) -> bool {
        self.resume().is_some_and(|r| r.done.contains(&id))
    }

    fn activity(&self, activity: Activity) {
        self.emit(Event::Activity { activity });
    }

    fn emit(&self, event: Event) {
        if let Some(vault) = self.vault {
            vault.observe(&event);
        }
        // The receiver only goes away when nobody watches the job any more.
        let _ = self.events.send(event);
    }

    /// Seals the open part of the encrypted backup, then deletes the
    /// messages whose files are in it. If sealing fails they are kept.
    async fn flush(&self, target: &Target, stats: &mut Stats) -> Result<()> {
        let Some(vault) = self.vault else {
            return Ok(());
        };
        let waiting = std::mem::take(&mut *self.pending.lock().unwrap());
        if let Err(err) = vault.commit() {
            for (message, _) in waiting {
                stats.failed += 1;
                self.emit(Event::Failed {
                    target_id: target.id,
                    message_id: message.id,
                    error: format!("kept, because the backup could not be written: {err}"),
                });
            }
            return Ok(());
        }
        for (message, saved) in waiting {
            self.delete(target, message, saved, stats).await?;
        }
        Ok(())
    }

    /// The run stops: what is backed up stays backed up, and its messages
    /// are not deleted.
    fn abandon_pending(&self) {
        if let Some(vault) = self.vault {
            self.pending.lock().unwrap().clear();
            if let Err(err) = vault.commit() {
                tracing::warn!(%err, "could not seal the backup part");
            }
        }
    }

    /// Pages through the search results from newest to oldest, moving the
    /// `max_id` cursor below the oldest hit (no `offset`, which Discord caps).
    /// Then searches again from the top, because the index can lag behind;
    /// stops once a round turns up nothing new.
    async fn purge_target(&self, target: &Target, stats: &mut Stats) -> Result<()> {
        let mut seen = HashSet::new();
        let mut searched = false;
        let base = SearchQuery {
            channel_ids: target.channels.clone(),
            ..self.query.clone()
        };
        // Messages found while counting (or in a dry run) need no searching.
        let known = if self.resume().is_none() {
            self.cache.and_then(|cache| cache.lookup(target, &base))
        } else {
            None
        };
        let mut read_on = None;
        if let Some(known) = &known {
            self.activity(Activity::UsingFound {
                target_id: target.id,
                messages: known.messages.len() as u64,
            });
            self.emit(Event::TargetEstimate {
                target_id: target.id,
                total: if known.complete {
                    known.messages.len() as u64
                } else {
                    known.total
                },
            });
            for message in &known.messages {
                seen.insert(message.id);
                self.handle(target, message.clone(), stats).await?;
            }
            if !known.complete {
                read_on = known.cursor;
            }
        }
        // A continued run starts below the oldest message it dealt with;
        // later rounds start from the top again, as always.
        let resume_cursor = self
            .resume()
            .and_then(|r| r.cursors.get(&target.id))
            .copied()
            .or(read_on);
        let complete = known.as_ref().is_some_and(|k| k.complete);
        // Everything was found before: a dry run is done, a real run only
        // checks whether anything new turned up.
        if complete && self.options.dry_run {
            return Ok(());
        }
        // A continued run's first round only covers what is below where it
        // stopped. Finding nothing there says nothing about the messages
        // above (failed ones, or ones the index returned late), so it does
        // not count as a round.
        let continued = self
            .resume()
            .is_some_and(|r| r.cursors.contains_key(&target.id));
        let rounds = self.options.max_rounds.max(1) + u32::from(continued);
        for round in 0..rounds {
            let first_round = round == 0 && !complete;
            let mut cursor = match (first_round, resume_cursor) {
                (true, Some(resume)) => {
                    Some(self.query.max_id.map_or(resume, |max| max.min(resume)))
                }
                _ => self.query.max_id,
            };
            let mut new_messages = 0;
            let mut page = 0;
            loop {
                if searched {
                    self.control.sleep_for(self.pace.before_search()).await?;
                }
                self.control.checkpoint().await?;
                page += 1;
                self.activity(Activity::Searching {
                    target_id: target.id,
                    page,
                    again: !first_round,
                });
                let (hits, next, total) = crate::scan::search_page(
                    self.client,
                    self.cache,
                    target,
                    &self.query,
                    cursor,
                    self.control,
                )
                .await?;
                if !searched {
                    searched = true;
                    if known.is_none() {
                        self.emit(Event::TargetEstimate {
                            target_id: target.id,
                            total,
                        });
                    }
                }
                let mut new_on_page = 0;
                for message in hits {
                    if seen.insert(message.id) && !self.already_done(message.id) {
                        new_on_page += 1;
                        self.handle(target, message, stats).await?;
                    }
                }
                new_messages += new_on_page;
                let Some(next) = next else {
                    break;
                };
                // Checking a place whose messages were all found before:
                // a page with nothing new means there is nothing more.
                if complete && new_on_page == 0 {
                    break;
                }
                cursor = Some(next);
            }
            let only_below = first_round && continued;
            if (new_messages == 0 && !only_below) || self.options.dry_run {
                break;
            }
        }
        Ok(())
    }

    /// Saves the attachments the others sent in a DM or group DM in the
    /// chosen time range, without touching their messages. Never on a
    /// server: there, what others posted is not the user's to take.
    async fn save_from_others(&self, target: &Target, stats: &mut Stats) -> Result<()> {
        if target.kind == TargetKind::Guild || (self.backup.is_none() && self.vault.is_none()) {
            return Ok(());
        }
        let query = SearchQuery {
            min_id: self.query.min_id,
            max_id: self.query.max_id,
            has: vec![Has::File],
            ..SearchQuery::default()
        };
        let mut cursor = query.max_id;
        let mut seen = HashSet::new();
        let mut page = 0;
        loop {
            self.control.sleep_for(self.pace.before_search()).await?;
            self.control.checkpoint().await?;
            page += 1;
            self.activity(Activity::SearchingOthers {
                target_id: target.id,
                page,
            });
            let found =
                crate::scan::search_page(self.client, None, target, &query, cursor, self.control)
                    .await;
            let (hits, next) = match found {
                Ok((hits, next, _)) => (hits, next),
                Err(err @ (Error::Unauthorized | Error::Cancelled)) => return Err(err),
                Err(err) => {
                    self.commit_others();
                    return Err(Error::Incomplete(format!(
                        "could not look for the others' attachments: {err}"
                    )));
                }
            };
            for message in hits {
                if message.author.id != self.me
                    && message.channel_id == target.id
                    && !message.attachments.is_empty()
                    && self.filter.contains(message.id)
                    && seen.insert(message.id)
                {
                    self.save_other(target, message, stats).await?;
                }
            }
            match next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        self.commit_others();
        Ok(())
    }

    async fn save_other(&self, target: &Target, message: Message, stats: &mut Stats) -> Result<()> {
        self.control.checkpoint().await?;
        self.activity(Activity::BackingUp {
            target_id: target.id,
        });
        let result = match (self.vault, &self.backup) {
            (Some(vault), _) => self.control.guard(vault.add(self.client, &message)).await?,
            (None, Some(backup)) => {
                self.control
                    .guard(backup.save(self.client, &message))
                    .await?
            }
            (None, None) => return Ok(()),
        };
        match result {
            Ok(saved) => {
                stats.saved_from_others += 1;
                self.emit(Event::SavedFromOthers {
                    target_id: target.id,
                    channel_id: message.channel_id,
                    message_id: message.id,
                    sent_at: message.id.created_at(),
                    author: message.author.display_name().to_owned(),
                    content: message.content.clone(),
                    attachments: attachment_urls(&message),
                    saved,
                });
                if self.vault.is_some_and(EncryptedBackup::part_is_full) {
                    self.commit_others();
                }
            }
            Err(error) => self.emit(Event::NotSavedFromOthers {
                target_id: target.id,
                message_id: message.id,
                error,
            }),
        }
        Ok(())
    }

    /// Seals the open part of the encrypted backup. Nothing waits for it,
    /// so a failure is only logged; the run's end reports on the backup.
    fn commit_others(&self) {
        if let Some(vault) = self.vault {
            if let Err(err) = vault.commit() {
                tracing::warn!(%err, "could not seal the backup part");
            }
        }
    }

    /// Deletes the matching messages of a data package, channel by channel.
    /// Each channel is looked up first, so channels that are gone or out of
    /// reach cost one request instead of one per message.
    async fn purge_known(
        &self,
        target: &Target,
        package: &Package,
        stats: &mut Stats,
    ) -> Result<()> {
        let work: Vec<(Snowflake, Vec<Message>)> = package
            .channels_of(target)
            .filter_map(|channel| {
                let messages: Vec<Message> = channel
                    .messages
                    .iter()
                    .filter(|m| self.filter.contains(m.id) && !self.already_done(m.id))
                    .map(|m| m.to_message(channel.id, self.me))
                    .filter(|m| self.matcher.matches(m))
                    .collect();
                (!messages.is_empty()).then_some((channel.id, messages))
            })
            .collect();
        self.emit(Event::TargetEstimate {
            target_id: target.id,
            total: work.iter().map(|(_, m)| m.len() as u64).sum(),
        });

        for (channel_id, messages) in work {
            self.control.checkpoint().await?;
            self.activity(Activity::CheckingChannel {
                target_id: target.id,
            });
            let pinned = match self.reach_channel(channel_id).await? {
                Ok(pinned) => pinned,
                Err(error) => {
                    stats.skipped += messages.len() as u64;
                    self.emit(Event::ChannelUnreachable {
                        target_id: target.id,
                        channel_id,
                        messages: messages.len() as u64,
                        error,
                    });
                    continue;
                }
            };
            for mut message in messages {
                message.pinned = pinned.contains(&message.id);
                self.handle(target, message, stats).await?;
            }
        }
        Ok(())
    }

    /// Checks that a channel still exists and can be reached, and returns its
    /// pinned messages if those are to be kept. The inner error says why the
    /// channel is out of reach.
    async fn reach_channel(
        &self,
        channel_id: Snowflake,
    ) -> Result<std::result::Result<HashSet<Snowflake>, String>> {
        match self.control.guard(self.client.channel(channel_id)).await? {
            Ok(_) => {}
            Err(Error::Unauthorized) => return Err(Error::Unauthorized),
            Err(err) => return Ok(Err(err.to_string())),
        }
        if !self.filter.skip_pinned {
            return Ok(Ok(HashSet::new()));
        }
        match self
            .control
            .guard(self.client.pinned_message_ids(channel_id))
            .await?
        {
            Ok(pins) => Ok(Ok(pins.into_iter().collect())),
            Err(Error::Unauthorized) => Err(Error::Unauthorized),
            Err(err) => Ok(Err(format!("could not check its pinned messages: {err}"))),
        }
    }

    async fn handle(&self, target: &Target, message: Message, stats: &mut Stats) -> Result<()> {
        // The search already filters by author and time. Check again so a
        // misbehaving index can never make us delete anything else.
        if message.author.id != self.me
            || !self.filter.contains(message.id)
            || !target.covers_channel(message.channel_id)
        {
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
        // Encrypted backup: the message waits until its part is sealed.
        if let Some(vault) = self.vault {
            if !message.attachments.is_empty() {
                self.control.checkpoint().await?;
                self.activity(Activity::BackingUp {
                    target_id: target.id,
                });
                let saved = match self.control.guard(vault.add(self.client, &message)).await? {
                    Ok(saved) => saved,
                    Err(error) => {
                        stats.failed += 1;
                        self.emit(Event::Failed {
                            target_id: target.id,
                            message_id: message.id,
                            error: format!(
                                "kept, because its attachments could not be backed up: {error}"
                            ),
                        });
                        return Ok(());
                    }
                };
                if self.options.dry_run {
                    stats.deleted += 1;
                    self.deleted(target, &message, true, saved);
                    return Ok(());
                }
                self.pending.lock().unwrap().push((message, saved));
                if vault.part_is_full() {
                    self.flush(target, stats).await?;
                }
                return Ok(());
            }
        }
        // Attachments are saved first; without a copy the message stays.
        let saved = match &self.backup {
            Some(backup) if !message.attachments.is_empty() => {
                self.control.checkpoint().await?;
                self.activity(Activity::BackingUp {
                    target_id: target.id,
                });
                match self
                    .control
                    .guard(backup.save(self.client, &message))
                    .await?
                {
                    Ok(saved) => saved,
                    Err(error) => {
                        stats.failed += 1;
                        self.emit(Event::Failed {
                            target_id: target.id,
                            message_id: message.id,
                            error: format!(
                                "kept, because its attachments could not be backed up: {error}"
                            ),
                        });
                        return Ok(());
                    }
                }
            }
            _ => Vec::new(),
        };
        if self.options.dry_run {
            stats.deleted += 1;
            self.deleted(target, &message, true, saved);
            return Ok(());
        }
        self.delete(target, message, saved, stats).await
    }

    /// Overwrites (if asked) and deletes a message that passed every check.
    async fn delete(
        &self,
        target: &Target,
        message: Message,
        saved: Vec<String>,
        stats: &mut Stats,
    ) -> Result<()> {
        self.control.checkpoint().await?;
        self.activity(Activity::Deleting {
            target_id: target.id,
        });
        // From here on the requests are not abandoned when the job is stopped:
        // Discord may already have carried them out, and the message must be
        // counted. The job stops at the next checkpoint instead.
        if let Some(text) = &self.options.overwrite {
            let text = if text.trim().is_empty() {
                random_text(message.id)
            } else {
                text.clone()
            };
            match self
                .client
                .overwrite_message(message.channel_id, message.id, &text)
                .await
            {
                Ok(()) => {}
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                // Deleting is what matters; it reports its own errors.
                Err(err) => tracing::warn!(%err, "could not overwrite message {}", message.id),
            }
        }
        let result = self
            .client
            .delete_message(message.channel_id, message.id)
            .await;
        match delete::classify(result)? {
            Outcome::Deleted | Outcome::AlreadyGone => {
                stats.deleted += 1;
                if let Some(cache) = self.cache {
                    cache.forget(target.id, &[message.id]);
                }
                self.deleted(target, &message, false, saved);
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
        let (pause, long) = self.pace.after_delete();
        if long {
            self.activity(Activity::Break {
                ms: pause.as_millis() as u64,
            });
        }
        self.control.sleep_for(pause).await
    }

    fn deleted(&self, target: &Target, message: &Message, dry_run: bool, saved: Vec<String>) {
        self.emit(Event::Deleted {
            target_id: target.id,
            channel_id: message.channel_id,
            message_id: message.id,
            sent_at: message.id.created_at(),
            preview: preview_text(message),
            content: message.content.clone(),
            attachments: attachment_urls(message),
            saved,
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

fn attachment_urls(message: &Message) -> Vec<String> {
    message
        .attachments
        .iter()
        .filter_map(|a| a["url"].as_str().map(str::to_owned))
        .collect()
}

/// 8 to 24 random lowercase letters.
fn random_text(seed: Snowflake) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::from(d.subsec_nanos()))
        .unwrap_or(0);
    // xorshift64; the state must not be zero.
    let mut state = (seed.0 ^ nanos.rotate_left(32)) | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let len = 8 + (next() % 17) as usize;
    (0..len)
        .map(|_| char::from(b'a' + (next() % 26) as u8))
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_text_is_letters_of_varying_length() {
        let texts: Vec<String> = (0..50).map(|i| random_text(Snowflake(i))).collect();
        assert!(texts
            .iter()
            .all(|t| (8..=24).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_lowercase())));
        assert!(texts.iter().any(|t| t.len() != texts[0].len()));
    }
}
