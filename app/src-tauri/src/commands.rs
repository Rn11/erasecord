//! Commands the web frontend calls through `invoke`. All Discord traffic runs
//! here in Rust, so the token never reaches the web view after login.

use erasecord_core::job::{self, Event, Filter, JobOptions, PreviewEntry};
use erasecord_core::{Client, ClientConfig, Error, Friend, GuildChannel, Snowflake, Target, User};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;

use crate::state::{token_store, AppState, Session};

/// Carries [`Event`]s of a running clean-up.
pub const JOB_EVENT: &str = "job-event";
/// Carries each [`PreviewEntry`] as soon as it is counted.
pub const PREVIEW_EVENT: &str = "preview-entry";

#[derive(Debug, Serialize)]
pub struct CommandError {
    kind: ErrorKind,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum ErrorKind {
    Unauthorized,
    Cancelled,
    Busy,
    NotLoggedIn,
    Other,
}

impl CommandError {
    pub fn not_logged_in() -> Self {
        CommandError {
            kind: ErrorKind::NotLoggedIn,
            message: "not logged in".into(),
        }
    }

    pub fn busy() -> Self {
        CommandError {
            kind: ErrorKind::Busy,
            message: "another run is still in progress".into(),
        }
    }
}

impl From<Error> for CommandError {
    fn from(err: Error) -> Self {
        let kind = match err {
            Error::Unauthorized | Error::InvalidToken => ErrorKind::Unauthorized,
            Error::Cancelled => ErrorKind::Cancelled,
            _ => ErrorKind::Other,
        };
        CommandError {
            kind,
            message: err.to_string(),
        }
    }
}

type CommandResult<T> = Result<T, CommandError>;

fn client_config() -> ClientConfig {
    #[allow(unused_mut)]
    let mut config = ClientConfig::default();
    // Lets `npm run tauri dev` talk to tools/fake_discord.py.
    #[cfg(debug_assertions)]
    if let Ok(api_base) = std::env::var("ERASECORD_API_BASE") {
        config.api_base = api_base;
    }
    config
}

#[derive(Serialize)]
pub struct LoginResult {
    user: User,
    /// Set when the token should be remembered but could not be stored.
    remember_error: Option<String>,
}

#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    token: String,
    remember: bool,
) -> CommandResult<LoginResult> {
    let client = Client::with_config(&token, client_config())?;
    let me = client.current_user().await?;
    let remember_error = if remember {
        token_store::save(token).await.err()
    } else {
        token_store::forget().await;
        None
    };
    state.set_session(Some(Session {
        client,
        me: me.clone(),
    }));
    Ok(LoginResult {
        user: me,
        remember_error,
    })
}

/// Logs in with the remembered token, if there is one that still works.
#[tauri::command]
pub async fn restore_session(state: State<'_, AppState>) -> CommandResult<Option<User>> {
    let Some(token) = token_store::load().await else {
        return Ok(None);
    };
    let client = Client::with_config(&token, client_config())?;
    match client.current_user().await {
        Ok(me) => {
            state.set_session(Some(Session {
                client,
                me: me.clone(),
            }));
            Ok(Some(me))
        }
        Err(Error::Unauthorized | Error::InvalidToken) => {
            token_store::forget().await;
            Ok(None)
        }
        Err(err) => Err(err.into()),
    }
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> CommandResult<()> {
    if let Some(job) = state.job() {
        job.cancel();
    }
    state.set_session(None);
    token_store::forget().await;
    Ok(())
}

#[tauri::command]
pub async fn list_targets(state: State<'_, AppState>) -> CommandResult<Vec<Target>> {
    let session = state.session()?;
    Ok(erasecord_core::list_targets(&session.client).await?)
}

#[tauri::command]
pub async fn list_channels(
    state: State<'_, AppState>,
    guild_id: Snowflake,
) -> CommandResult<Vec<GuildChannel>> {
    let session = state.session()?;
    Ok(erasecord_core::list_channels(&session.client, guild_id).await?)
}

#[tauri::command]
pub async fn list_friends(state: State<'_, AppState>) -> CommandResult<Vec<Friend>> {
    let session = state.session()?;
    Ok(erasecord_core::friends_without_dm(&session.client).await?)
}

#[tauri::command]
pub async fn open_dm(state: State<'_, AppState>, user_id: Snowflake) -> CommandResult<Target> {
    let session = state.session()?;
    Ok(erasecord_core::open_dm(&session.client, user_id).await?)
}

#[tauri::command]
pub async fn preview(
    app: AppHandle,
    state: State<'_, AppState>,
    targets: Vec<Target>,
    filter: Filter,
    options: JobOptions,
) -> CommandResult<Vec<PreviewEntry>> {
    let session = state.session()?;
    let control = state.begin_job()?;
    let result = job::preview(
        &session.client,
        session.me.id,
        &targets,
        &filter,
        &options,
        &control,
        |_, entry| {
            let _ = app.emit(PREVIEW_EVENT, entry);
        },
    )
    .await;
    state.end_job();
    Ok(result?)
}

/// Starts deleting in the background; progress arrives as [`JOB_EVENT`]s.
#[tauri::command]
pub async fn start_job(
    app: AppHandle,
    state: State<'_, AppState>,
    targets: Vec<Target>,
    filter: Filter,
    options: JobOptions,
) -> CommandResult<()> {
    let session = state.session()?;
    let control = state.begin_job()?;
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let run = job::run(
            &session.client,
            session.me.id,
            &targets,
            &filter,
            &options,
            &control,
            tx,
        );
        let forward = async {
            while let Some(event) = rx.recv().await {
                if matches!(event, Event::Finished(_)) {
                    // Free the slot before the UI hears about it, so it can
                    // start the next run right away.
                    app.state::<AppState>().end_job();
                }
                let _ = app.emit(JOB_EVENT, &event);
            }
        };
        tokio::join!(run, forward);
    });
    Ok(())
}

#[tauri::command]
pub fn pause_job(state: State<'_, AppState>) {
    if let Some(job) = state.job() {
        job.pause();
    }
}

#[tauri::command]
pub fn resume_job(state: State<'_, AppState>) {
    if let Some(job) = state.job() {
        job.resume();
    }
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>) {
    if let Some(job) = state.job() {
        job.cancel();
    }
}
