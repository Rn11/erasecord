//! What the app keeps between commands: the logged-in session, the running
//! job, and the remembered token.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use erasecord_core::{Client, Event, JobControl, Package, User};

use crate::commands::CommandError;

#[derive(Clone)]
pub struct Session {
    pub client: Client,
    pub me: User,
}

#[derive(Default)]
pub struct AppState {
    session: Mutex<Option<Session>>,
    /// The running preview or clean-up; only one runs at a time.
    job: Mutex<Option<JobControl>>,
    /// The imported data package, if any. Kept here: it is too big to send
    /// to the web view.
    package: Mutex<Option<(Arc<Package>, PathBuf)>>,
    /// The events of the last clean-up that an export needs: names and
    /// deleted messages.
    last_run: Mutex<Vec<Event>>,
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
        if matches!(event, Event::TargetStarted { .. } | Event::Deleted { .. }) {
            self.last_run.lock().unwrap().push(event.clone());
        }
    }

    pub fn last_run(&self) -> Vec<Event> {
        self.last_run.lock().unwrap().clone()
    }

    /// The imported package and where it was read from.
    pub fn package(&self) -> Option<(Arc<Package>, PathBuf)> {
        self.package.lock().unwrap().clone()
    }

    pub fn set_package(&self, package: Option<(Arc<Package>, PathBuf)>) {
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
