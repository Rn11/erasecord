//! What the app keeps between commands: the logged-in session, the running
//! job, and the remembered token.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use erasecord_core::insights::Index;
use erasecord_core::targets::{Friend, GuildChannel};
use erasecord_core::{Client, Event, JobControl, MessageCache, Package, Snowflake, Target, User};
use tokio::sync::watch;

use crate::commands::CommandError;

#[derive(Clone)]
pub struct Session {
    pub client: Client,
    pub me: User,
}

/// A data package, where it was read from, and its index for Insights.
pub type LoadedPackage = (Arc<Package>, PathBuf, Arc<Index>);

#[derive(Default)]
pub struct AppState {
    session: Mutex<Option<Session>>,
    /// The running preview or clean-up; only one runs at a time.
    job: Mutex<Option<JobControl>>,
    /// The imported data package, if any. Kept here: it is too big to send
    /// to the web view.
    package: Mutex<Option<LoadedPackage>>,
    /// The events of the last clean-up that an export needs: names and
    /// deleted messages.
    last_run: Mutex<Vec<Event>>,
    /// Messages found while counting, reused by dry runs and clean-ups.
    /// In memory only, like everything here.
    pub messages: MessageCache,
    /// Lists from Discord, so going back and forth does not ask again.
    lists: Mutex<Lists>,
    /// The running count, and whether it has stopped.
    scan: Mutex<Option<(JobControl, watch::Receiver<bool>)>>,
}

/// How long lists of servers, DMs, channels and friends are kept.
const LIST_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Default)]
struct Lists {
    targets: Option<(Instant, Vec<Target>)>,
    channels: HashMap<Snowflake, (Instant, Vec<GuildChannel>)>,
    friends: Option<(Instant, Vec<Friend>)>,
}

fn fresh<T: Clone>(entry: Option<&(Instant, T)>) -> Option<T> {
    entry
        .filter(|(at, _)| at.elapsed() < LIST_TTL)
        .map(|(_, value)| value.clone())
}

impl AppState {
    pub fn session(&self) -> Result<Session, CommandError> {
        self.session
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(CommandError::not_logged_in)
    }

    pub fn set_session(&self, session: Option<Session>) {
        *self.session.lock().unwrap() = session;
        // Another account, or none: forget everything found for the last.
        self.messages.clear();
        *self.lists.lock().unwrap() = Lists::default();
    }

    pub fn cached_targets(&self) -> Option<Vec<Target>> {
        fresh(self.lists.lock().unwrap().targets.as_ref())
    }

    pub fn cache_targets(&self, targets: Option<Vec<Target>>) {
        self.lists.lock().unwrap().targets = targets.map(|t| (Instant::now(), t));
    }

    pub fn cached_channels(&self, guild: Snowflake) -> Option<Vec<GuildChannel>> {
        fresh(self.lists.lock().unwrap().channels.get(&guild))
    }

    pub fn cache_channels(&self, guild: Snowflake, channels: Vec<GuildChannel>) {
        self.lists
            .lock()
            .unwrap()
            .channels
            .insert(guild, (Instant::now(), channels));
    }

    pub fn cached_friends(&self) -> Option<Vec<Friend>> {
        fresh(self.lists.lock().unwrap().friends.as_ref())
    }

    pub fn cache_friends(&self, friends: Option<Vec<Friend>>) {
        self.lists.lock().unwrap().friends = friends.map(|f| (Instant::now(), f));
    }

    /// Forgets the lists, so the next ones come from Discord.
    pub fn forget_lists(&self) {
        *self.lists.lock().unwrap() = Lists::default();
    }

    /// Starts a count, stopping one that is still running.
    pub async fn begin_scan(&self) -> (JobControl, watch::Sender<bool>) {
        self.stop_scan().await;
        let control = JobControl::new();
        let (done, rx) = watch::channel(false);
        *self.scan.lock().unwrap() = Some((control.clone(), rx));
        (control, done)
    }

    /// Stops the running count and waits until it has stopped, so what it
    /// found is complete in the cache.
    pub async fn stop_scan(&self) {
        let running = self.scan.lock().unwrap().take();
        if let Some((control, mut done)) = running {
            control.cancel();
            let _ = done.wait_for(|done| *done).await;
        }
    }

    pub fn begin_job(&self) -> Result<JobControl, CommandError> {
        let mut job = self.job.lock().unwrap();
        if job.is_some() {
            return Err(CommandError::busy());
        }
        let control = JobControl::new();
        *job = Some(control.clone());
        Ok(control)
    }

    pub fn end_job(&self) {
        *self.job.lock().unwrap() = None;
    }

    pub fn job(&self) -> Option<JobControl> {
        self.job.lock().unwrap().clone()
    }

    pub fn clear_last_run(&self) {
        self.last_run.lock().unwrap().clear();
    }

    pub fn record(&self, event: &Event) {
        if matches!(
            event,
            Event::TargetStarted { .. } | Event::Deleted { .. } | Event::SavedFromOthers { .. }
        ) {
            self.last_run.lock().unwrap().push(event.clone());
        }
    }

    pub fn last_run(&self) -> Vec<Event> {
        self.last_run.lock().unwrap().clone()
    }

    /// The imported package and where it was read from.
    pub fn package(&self) -> Option<(Arc<Package>, PathBuf)> {
        self.package
            .lock()
            .unwrap()
            .as_ref()
            .map(|(package, path, _)| (package.clone(), path.clone()))
    }

    /// The package's index for Insights.
    pub fn index(&self) -> Option<Arc<Index>> {
        self.package
            .lock()
            .unwrap()
            .as_ref()
            .map(|(_, _, index)| index.clone())
    }

    pub fn set_package(&self, package: Option<LoadedPackage>) {
        *self.package.lock().unwrap() = package;
    }
}

/// The token in the operating system's credential store (Keychain, Windows
/// Credential Manager, Secret Service). The store may block, so every call
/// runs on a blocking thread.
pub mod token_store {
    use keyring::Entry;

    const SERVICE: &str = "erasecord";
    const USER: &str = "discord-token";

    async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        tauri::async_runtime::spawn_blocking(f)
            .await
            .expect("credential store task panicked")
    }

    pub async fn load() -> Option<String> {
        blocking(|| Entry::new(SERVICE, USER).ok()?.get_password().ok()).await
    }

    pub async fn save(token: String) -> Result<(), String> {
        blocking(move || Entry::new(SERVICE, USER)?.set_password(&token))
            .await
            .map_err(|err| err.to_string())
    }

    pub async fn forget() {
        blocking(|| {
            if let Ok(entry) = Entry::new(SERVICE, USER) {
                let _ = entry.delete_credential();
            }
        })
        .await
    }
}
