//! Commands the web frontend calls through `invoke`. All Discord traffic runs
//! here in Rust, so the token never reaches the web view after login.

use std::path::PathBuf;
use std::sync::Arc;

use erasecord_core::job::{self, Event, Filter, JobOptions, PreviewEntry};
use erasecord_core::{
    Client, ClientConfig, Error, ExportFormat, ExportWriter, Friend, GuildChannel, Package,
    PackageTarget, Snowflake, Target, TargetKind, User,
};
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
    NoPackage,
    Other,
}

impl CommandError {
    pub fn not_logged_in() -> Self {
        CommandError {
            kind: ErrorKind::NotLoggedIn,
            message: "not logged in".into(),
        }
    }

    fn other(message: String) -> Self {
        CommandError {
            kind: ErrorKind::Other,
            message,
        }
    }

    fn no_package() -> Self {
        CommandError {
            kind: ErrorKind::NoPackage,
            message: "no data package is open".into(),
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
    state.set_package(None);
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
    spawn_job(app, &state, None, targets, filter, options)
}

/// Like [`start_job`], with the messages of the imported data package.
#[tauri::command]
pub async fn start_package_job(
    app: AppHandle,
    state: State<'_, AppState>,
    targets: Vec<Target>,
    filter: Filter,
    options: JobOptions,
) -> CommandResult<()> {
    let package = state.package().ok_or_else(CommandError::no_package)?;
    spawn_job(app, &state, Some(package), targets, filter, options)
}

fn spawn_job(
    app: AppHandle,
    state: &AppState,
    package: Option<Arc<Package>>,
    targets: Vec<Target>,
    filter: Filter,
    options: JobOptions,
) -> CommandResult<()> {
    let session = state.session()?;
    // The list of messages next to the backed-up files; created before the
    // run, so a folder that cannot be written stops it before anything is
    // deleted.
    let mut backup_list =
        match &options.backup_dir {
            Some(dir) => Some(backup_list(dir).map_err(|err| {
                CommandError::other(format!("cannot use the backup folder: {err}"))
            })?),
            None => None,
        };
    let control = state.begin_job()?;
    state.clear_last_run();
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (client, me) = (&session.client, session.me.id);
        let run = async {
            match &package {
                Some(package) => {
                    job::run_package(
                        client, me, package, &targets, &filter, &options, &control, tx,
                    )
                    .await
                }
                None => job::run(client, me, &targets, &filter, &options, &control, tx).await,
            }
        };
        let forward = async {
            while let Some(event) = rx.recv().await {
                app.state::<AppState>().record(&event);
                if let Some(list) = backup_list.as_mut() {
                    // A broken list must not stop the run; the files are saved.
                    if list.observe(&event).is_err() {
                        backup_list = None;
                    }
                }
                if matches!(event, Event::Finished(_)) {
                    if let Some(list) = backup_list.take() {
                        let _ = list.finish();
                    }
                }
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

type ListWriter = ExportWriter<std::io::BufWriter<std::fs::File>>;

/// `messages-<time>.json` in the backup folder.
fn backup_list(dir: &std::path::Path) -> std::io::Result<ListWriter> {
    std::fs::create_dir_all(dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let file = std::fs::File::create(dir.join(format!("messages-{stamp}.json")))?;
    ExportWriter::new(std::io::BufWriter::new(file), ExportFormat::Json)
}

/// Saves what the last clean-up deleted (or would delete) to `path`: JSON
/// for a .json file, CSV otherwise. Returns the number of messages.
#[tauri::command]
pub async fn export_run(state: State<'_, AppState>, path: PathBuf) -> CommandResult<u64> {
    let events = state.last_run();
    tauri::async_runtime::spawn_blocking(move || -> std::io::Result<u64> {
        let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
        let mut writer = ExportWriter::new(file, ExportFormat::for_path(&path))?;
        for event in &events {
            writer.observe(event)?;
        }
        let rows = writer.rows();
        writer.finish()?;
        Ok(rows)
    })
    .await
    .map_err(|err| CommandError::other(err.to_string()))?
    .map_err(|err| CommandError::other(format!("could not save the file: {err}")))
}

#[derive(Serialize)]
pub struct PackageSummary {
    targets: Vec<PackageTarget>,
    messages: u64,
    /// Servers in the package that the user is no longer a member of.
    left_servers: Vec<Snowflake>,
}

/// Reads a data package (a .zip file or an extracted folder) and keeps it
/// for [`preview_package`] and [`start_package_job`].
#[tauri::command]
pub async fn import_package(
    state: State<'_, AppState>,
    path: PathBuf,
) -> CommandResult<PackageSummary> {
    let session = state.session()?;
    let package = tauri::async_runtime::spawn_blocking(move || Package::open(&path))
        .await
        .map_err(|err| CommandError::from(Error::Package(err.to_string())))??;
    package.check_owner(session.me.id)?;
    let mut targets = package.targets();
    // The package knows the people in a DM only by ID; open conversations
    // lend their current names and pictures. Best effort: a failure here
    // leaves the package's names.
    match session.client.private_channels().await {
        Ok(channels) => {
            let live: Vec<Target> = channels
                .into_iter()
                .filter_map(Target::from_channel)
                .collect();
            erasecord_core::package::apply_live_names(&mut targets, &live);
        }
        Err(Error::Unauthorized) => return Err(Error::Unauthorized.into()),
        Err(_) => {}
    }
    let member_of: Vec<Snowflake> = session
        .client
        .guilds()
        .await?
        .into_iter()
        .map(|g| g.id)
        .collect();
    let left_servers = targets
        .iter()
        .filter(|t| t.target.kind == TargetKind::Guild && !member_of.contains(&t.target.id))
        .map(|t| t.target.id)
        .collect();
    let summary = PackageSummary {
        messages: package.message_count(),
        targets,
        left_servers,
    };
    state.set_package(Some(Arc::new(package)));
    Ok(summary)
}

#[tauri::command]
pub fn close_package(state: State<'_, AppState>) {
    state.set_package(None);
}

/// Exact counts from the imported package; needs no network.
#[tauri::command]
pub async fn preview_package(
    state: State<'_, AppState>,
    targets: Vec<Target>,
    filter: Filter,
) -> CommandResult<Vec<PreviewEntry>> {
    let session = state.session()?;
    let package = state.package().ok_or_else(CommandError::no_package)?;
    Ok(job::preview_package(
        &package,
        session.me.id,
        &targets,
        &filter,
    )?)
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
