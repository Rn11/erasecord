//! Commands the web frontend calls through `invoke`. All Discord traffic runs
//! here in Rust, so the token never reaches the web view after login.

use std::path::PathBuf;
use std::sync::Arc;

use erasecord_core::insights::{
    Index, Info, LinksReport, Overview, PlacesReport, Scope, SearchResult, Timeline, WordsReport,
};
use erasecord_core::job::{self, Event, Filter, JobOptions, PreviewEntry, Stats};
use erasecord_core::scan::{self, ScanEvent, ScanStats};
use erasecord_core::vault::{self, EncryptedBackupSettings, KeySlot, SecretString};
use erasecord_core::{
    Client, ClientConfig, Error, ExportFormat, ExportWriter, Friend, GuildChannel, Package,
    PackageTarget, SavedRun, Snowflake, Target, TargetKind, User,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;

use crate::state::{token_store, AppState, Session};

/// Carries [`Event`]s of a running clean-up.
pub const JOB_EVENT: &str = "job-event";
/// Carries the [`ScanEvent`]s of a running count.
pub const SCAN_EVENT: &str = "scan-event";

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
    state.stop_scan().await;
    state.set_session(None);
    state.set_package(None);
    token_store::forget().await;
    Ok(())
}

#[tauri::command]
/// The servers and DMs; from memory if asked for recently, unless
/// `refresh`.
pub async fn list_targets(
    state: State<'_, AppState>,
    refresh: Option<bool>,
) -> CommandResult<Vec<Target>> {
    let session = state.session()?;
    if refresh == Some(true) {
        // Asking Discord again means for the messages too.
        state.forget_lists();
        state.messages.clear();
    } else if let Some(targets) = state.cached_targets() {
        return Ok(targets);
    }
    let targets = erasecord_core::list_targets(&session.client).await?;
    state.cache_targets(Some(targets.clone()));
    Ok(targets)
}

#[tauri::command]
pub async fn list_channels(
    state: State<'_, AppState>,
    guild_id: Snowflake,
) -> CommandResult<Vec<GuildChannel>> {
    let session = state.session()?;
    if let Some(channels) = state.cached_channels(guild_id) {
        return Ok(channels);
    }
    let channels = erasecord_core::list_channels(&session.client, guild_id).await?;
    state.cache_channels(guild_id, channels.clone());
    Ok(channels)
}

#[tauri::command]
pub async fn list_friends(state: State<'_, AppState>) -> CommandResult<Vec<Friend>> {
    let session = state.session()?;
    if let Some(friends) = state.cached_friends() {
        return Ok(friends);
    }
    let friends = erasecord_core::friends_without_dm(&session.client).await?;
    state.cache_friends(Some(friends.clone()));
    Ok(friends)
}

#[tauri::command]
pub async fn open_dm(state: State<'_, AppState>, user_id: Snowflake) -> CommandResult<Target> {
    let session = state.session()?;
    let target = erasecord_core::open_dm(&session.client, user_id).await?;
    // The DM is open now: both lists changed.
    state.cache_targets(None);
    state.cache_friends(None);
    Ok(target)
}

/// Counts, then reads the matching messages in the background; progress
/// and statistics arrive as [`SCAN_EVENT`]s, ending with `finished`. What it
/// finds stays in memory for the dry run or clean-up that follows.
#[tauri::command]
pub async fn start_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    targets: Vec<Target>,
    filter: Filter,
    options: JobOptions,
    scan_id: u64,
) -> CommandResult<()> {
    let session = state.session()?;
    if state.job().is_some() {
        return Err(CommandError::busy());
    }
    filter.compile()?;
    let (control, done) = state.begin_scan().await;
    let cache = state.messages.clone();
    tauri::async_runtime::spawn(async move {
        // Every event names its count, so the UI can ignore late events of
        // one it has left.
        let send = move |app: &AppHandle, event: &ScanEvent| {
            if let Ok(serde_json::Value::Object(mut map)) = serde_json::to_value(event) {
                map.insert("scan_id".into(), scan_id.into());
                let _ = app.emit(SCAN_EVENT, map);
            }
        };
        let emit = |event: ScanEvent| send(&app, &event);
        let notices = app.clone();
        session.client.set_notice_sink(Some(Arc::new(move |notice| {
            send(&notices, &ScanEvent::Notice { notice });
        })));
        let result = scan::scan(
            &session.client,
            session.me.id,
            &targets,
            &filter,
            &options,
            &control,
            &cache,
            emit,
        )
        .await;
        session.client.set_notice_sink(None);
        let (cancelled, error) = match result {
            Ok(()) => (false, None),
            Err(Error::Cancelled) => (true, None),
            Err(err) => (false, Some(CommandError::from(err))),
        };
        let _ = app.emit(
            SCAN_EVENT,
            serde_json::json!({
                "type": "finished",
                "scan_id": scan_id,
                "cancelled": cancelled,
                "error": error,
            }),
        );
        let _ = done.send(true);
    });
    Ok(())
}

/// Stops reading messages; what was found so far is kept.
#[tauri::command]
pub async fn stop_scan(state: State<'_, AppState>) -> CommandResult<()> {
    state.stop_scan().await;
    Ok(())
}

/// Saves the messages found while counting to `path` (CSV, JSON or JSON
/// Lines by its name, encrypted as `.age` with a passphrase), before
/// anything is deleted. Returns how many were saved.
#[tauri::command]
pub async fn export_found(
    state: State<'_, AppState>,
    targets: Vec<Target>,
    filter: Filter,
    path: PathBuf,
    passphrase: Option<PassphraseInput>,
) -> CommandResult<u64> {
    let session = state.session()?;
    let found = scan::found_messages(&state.messages, session.me.id, &targets, &filter)?;
    let passphrase = passphrase.map(PassphraseInput::resolve).transpose()?;
    tauri::async_runtime::spawn_blocking(move || -> std::io::Result<u64> {
        let format = ExportFormat::for_path(&plain_path(&path));
        let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
        fn write_all<W: std::io::Write>(
            writer: &mut ExportWriter<W>,
            found: &[(Target, Vec<erasecord_core::Message>)],
        ) -> std::io::Result<()> {
            for (target, messages) in found {
                for message in messages {
                    writer.found(target, message)?;
                }
            }
            Ok(())
        }
        match passphrase {
            None => {
                let mut writer = ExportWriter::new(file, format)?;
                write_all(&mut writer, &found)?;
                let rows = writer.rows();
                writer.finish()?;
                Ok(rows)
            }
            Some(passphrase) => {
                let mut writer = ExportWriter::new(vault::encrypt(&passphrase, file)?, format)?;
                write_all(&mut writer, &found)?;
                let rows = writer.rows();
                writer.finish()?.finish()?;
                Ok(rows)
            }
        }
    })
    .await
    .map_err(|err| CommandError::other(err.to_string()))?
    .map_err(|err| CommandError::other(format!("could not save the file: {err}")))
}

/// `list.csv.age` is a CSV file, encrypted.
fn plain_path(path: &std::path::Path) -> PathBuf {
    if path.extension().is_some_and(|e| e == "age") {
        path.with_extension("")
    } else {
        path.to_path_buf()
    }
}

/// Starts deleting in the background; progress arrives as [`JOB_EVENT`]s.
#[tauri::command]
pub async fn start_job(
    app: AppHandle,
    state: State<'_, AppState>,
    targets: Vec<Target>,
    filter: Filter,
    options: JobOptions,
    backup_passphrase: Option<PassphraseInput>,
    export: Option<ExportSettings>,
) -> CommandResult<Option<PathBuf>> {
    // Before anything is created or saved: a second run must not touch the
    // running one's progress file.
    if state.job().is_some() {
        return Err(CommandError::busy());
    }
    let passphrase = backup_passphrase
        .map(PassphraseInput::resolve)
        .transpose()?;
    state.stop_scan().await;
    let export = open_export(export, options.dry_run, passphrase.clone()).await?;
    let path = export.as_ref().map(|e| e.path.clone());
    spawn_job(
        app, &state, None, targets, filter, options, None, passphrase, export,
    )?;
    Ok(path)
}

/// Where and how to save the messages of a run while it goes.
#[derive(Deserialize)]
pub struct ExportSettings {
    dir: PathBuf,
    format: ExportFormat,
}

/// The export of a running clean-up, plain or encrypted.
pub struct RunExport {
    path: PathBuf,
    writer: RunWriter,
}

enum RunWriter {
    Plain(ExportWriter<std::io::BufWriter<std::fs::File>>),
    Encrypted(ExportWriter<vault::StreamWriter<std::io::BufWriter<std::fs::File>>>),
}

impl RunExport {
    fn observe(&mut self, event: &Event) -> std::io::Result<()> {
        match &mut self.writer {
            RunWriter::Plain(w) => w.observe(event),
            RunWriter::Encrypted(w) => w.observe(event),
        }
    }

    fn finish(self) -> std::io::Result<()> {
        match self.writer {
            RunWriter::Plain(w) => w.finish().map(drop),
            RunWriter::Encrypted(w) => w.finish()?.finish().map(drop),
        }
    }
}

/// Creates the export file before anything is deleted, so a folder that
/// cannot be written stops the run first: `erasecord-deleted-<time>.csv`
/// (or `-dry-run-`), with `.age` when encrypted.
async fn open_export(
    settings: Option<ExportSettings>,
    dry_run: bool,
    passphrase: Option<SecretString>,
) -> CommandResult<Option<RunExport>> {
    let Some(settings) = settings else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || -> std::io::Result<RunExport> {
        std::fs::create_dir_all(&settings.dir)?;
        let ext = match settings.format {
            ExportFormat::Csv => "csv",
            ExportFormat::Json => "json",
            ExportFormat::JsonLines => "jsonl",
        };
        let kind = if dry_run { "dry-run" } else { "deleted" };
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let age = if passphrase.is_some() { ".age" } else { "" };
        // Never overwrite an earlier export, even from the same second.
        let (path, file) = (1..)
            .map(|n| {
                let suffix = if n == 1 {
                    String::new()
                } else {
                    format!("-{n}")
                };
                settings
                    .dir
                    .join(format!("erasecord-{kind}-{stamp}{suffix}.{ext}{age}"))
            })
            .take(100)
            .find_map(|path| {
                std::fs::File::options()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .ok()
                    .map(|file| (path, file))
            })
            .ok_or_else(|| std::io::Error::other("no free file name"))?;
        let file = std::io::BufWriter::new(file);
        let writer = match passphrase {
            None => RunWriter::Plain(ExportWriter::new(file, settings.format)?),
            Some(passphrase) => RunWriter::Encrypted(ExportWriter::new(
                vault::encrypt(&passphrase, file)?,
                settings.format,
            )?),
        };
        Ok(RunExport { path, writer })
    })
    .await
    .map_err(|err| CommandError::other(err.to_string()))?
    .map(Some)
    .map_err(|err| CommandError::other(format!("cannot create the export file: {err}")))
}

/// A passphrase as typed, or the file it is in (its first line).
#[derive(Deserialize)]
pub struct PassphraseInput {
    text: Option<String>,
    file: Option<PathBuf>,
}

impl PassphraseInput {
    fn resolve(self) -> CommandResult<SecretString> {
        match (self.text, self.file) {
            (Some(text), _) if !text.is_empty() => Ok(vault::secret(&text)),
            (_, Some(file)) => vault::read_passphrase_file(&file).map_err(|err| {
                CommandError::other(format!("cannot read the passphrase file: {err}"))
            }),
            _ => Err(CommandError::other("a passphrase is needed".into())),
        }
    }
}

/// Twelve random words, for a new encrypted backup or export.
#[tauri::command]
pub fn generate_passphrase() -> String {
    vault::generate_passphrase()
}

/// Decrypts and unpacks a backup (archive or parts folder) or an encrypted
/// export into `into`.
#[tauri::command]
pub async fn open_backup(
    path: PathBuf,
    passphrase: PassphraseInput,
    into: PathBuf,
) -> CommandResult<vault::Opened> {
    let passphrase = passphrase.resolve()?;
    tauri::async_runtime::spawn_blocking(move || vault::open(&path, &passphrase, &into))
        .await
        .map_err(|err| CommandError::other(err.to_string()))?
        .map_err(CommandError::other)
}

/// Like [`start_job`], with the messages of the imported data package.
#[tauri::command]
pub async fn start_package_job(
    app: AppHandle,
    state: State<'_, AppState>,
    targets: Vec<Target>,
    filter: Filter,
    options: JobOptions,
    backup_passphrase: Option<PassphraseInput>,
    export: Option<ExportSettings>,
) -> CommandResult<Option<PathBuf>> {
    if state.job().is_some() {
        return Err(CommandError::busy());
    }
    let package = state.package().ok_or_else(CommandError::no_package)?;
    // The package may have been opened before logging in.
    package.0.check_owner(state.session()?.me.id)?;
    let passphrase = backup_passphrase
        .map(PassphraseInput::resolve)
        .transpose()?;
    let export = open_export(export, options.dry_run, passphrase.clone()).await?;
    let path = export.as_ref().map(|e| e.path.clone());
    spawn_job(
        app,
        &state,
        Some(package),
        targets,
        filter,
        options,
        None,
        passphrase,
        export,
    )?;
    Ok(path)
}

/// Starts a clean-up. `saved` continues an earlier one. Real runs keep their
/// progress in [`run_state_file`], so they can be continued after a stop or
/// a crash; the file is removed once everything is done.
#[allow(clippy::too_many_arguments)]
fn spawn_job(
    app: AppHandle,
    state: &AppState,
    package: Option<(Arc<Package>, PathBuf)>,
    targets: Vec<Target>,
    filter: Filter,
    mut options: JobOptions,
    saved: Option<SavedRun>,
    backup_passphrase: Option<SecretString>,
    mut export: Option<RunExport>,
) -> CommandResult<()> {
    let session = state.session()?;
    // A new encrypted backup: its folder and key are made before anything
    // is deleted.
    if let (Some(dir), Some(passphrase), None) = (
        &options.backup_dir,
        backup_passphrase,
        &options.backup_encryption,
    ) {
        let (settings, keys) = EncryptedBackupSettings::create(dir, passphrase)
            .map_err(|err| CommandError::other(format!("cannot use the backup folder: {err}")))?;
        options.backup_encryption = Some(settings);
        options.backup_keys = KeySlot(Some(Arc::new(keys)));
    }
    let state_file = run_state_file(&app);
    let mut saved = if options.dry_run {
        None
    } else {
        Some(saved.unwrap_or_else(|| {
            let path = package.as_ref().map(|(_, path)| path.as_path());
            SavedRun::new(&targets, &filter, &options, path)
        }))
    };
    if let (Some(file), Some(saved)) = (&state_file, &saved) {
        saved.save(file).map_err(|err| {
            CommandError::other(format!("cannot save the progress of the clean-up: {err}"))
        })?;
    }
    let package = package.map(|(package, _)| package);
    // The list of messages next to the backed-up files; created before the
    // run, so a folder that cannot be written stops it before anything is
    // deleted.
    let mut backup_list =
        match options
            .backup_dir
            .as_ref()
            .filter(|_| options.backup_encryption.is_none())
        {
            Some(dir) => Some(backup_list(dir).map_err(|err| {
                CommandError::other(format!("cannot use the backup folder: {err}"))
            })?),
            None => None,
        };
    let control = state.begin_job()?;
    state.clear_last_run();
    let cache = state.messages.clone();
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
                None => {
                    job::run_with_cache(
                        client,
                        me,
                        &targets,
                        &filter,
                        &options,
                        &control,
                        Some(&cache),
                        tx,
                    )
                    .await
                }
            }
        };
        let forward = async {
            let mut unsaved = 0;
            while let Some(event) = rx.recv().await {
                app.state::<AppState>().record(&event);
                if let (Some(file), Some(saved)) = (&state_file, saved.as_mut()) {
                    saved.checkpoint.observe(&event);
                    unsaved += 1;
                    if let Event::Finished(summary) = &event {
                        // A server or DM whose search failed is not done yet.
                        if summary.error.is_none() && !summary.cancelled && saved.remaining() == 0 {
                            let _ = std::fs::remove_file(file);
                        } else {
                            let _ = saved.save(file);
                        }
                    } else if unsaved >= STATE_SAVE_EVERY {
                        unsaved = 0;
                        let _ = saved.save(file);
                    }
                }
                if let Some(list) = backup_list.as_mut() {
                    // A broken list must not stop the run; the files are saved.
                    if list.observe(&event).is_err() {
                        backup_list = None;
                    }
                }
                if let Some(file) = export.as_mut() {
                    if file.observe(&event).is_err() {
                        export = None;
                    }
                }
                if matches!(event, Event::Finished(_)) {
                    if let Some(list) = backup_list.take() {
                        let _ = list.finish();
                    }
                    if let Some(file) = export.take() {
                        let _ = file.finish();
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

/// How often the progress file is written, in events.
const STATE_SAVE_EVERY: u32 = 20;

fn run_state_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("unfinished-run.json"))
}

/// What the app shows about a clean-up that did not finish.
#[derive(Serialize)]
pub struct UnfinishedRun {
    targets: Vec<Target>,
    finished: Vec<Snowflake>,
    stats: Stats,
    from_package: bool,
    /// Continuing needs the backup's passphrase.
    encrypted_backup: bool,
}

impl UnfinishedRun {
    fn of(saved: &SavedRun) -> Self {
        UnfinishedRun {
            targets: saved.targets.clone(),
            finished: saved.checkpoint.finished.clone(),
            stats: saved.checkpoint.stats,
            from_package: saved.package.is_some(),
            encrypted_backup: saved.options.backup_encryption.is_some(),
        }
    }
}

/// The clean-up that was stopped or cut short, if there is one.
#[tauri::command]
pub fn unfinished_run(app: AppHandle) -> Option<UnfinishedRun> {
    let saved = SavedRun::load(&run_state_file(&app)?).ok()?;
    Some(UnfinishedRun::of(&saved))
}

/// Continues the unfinished clean-up, with exactly its servers, DMs and
/// conditions. Progress arrives as [`JOB_EVENT`]s.
#[tauri::command]
pub async fn resume_run(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: Option<PassphraseInput>,
) -> CommandResult<UnfinishedRun> {
    if state.job().is_some() {
        return Err(CommandError::busy());
    }
    let file =
        run_state_file(&app).ok_or_else(|| CommandError::other("no app data folder".into()))?;
    let saved = SavedRun::load(&file).map_err(|err| {
        CommandError::other(format!("cannot read the unfinished clean-up: {err}"))
    })?;
    let session = state.session()?;
    state.stop_scan().await;
    let package = match &saved.package {
        Some(path) => {
            let source = path.clone();
            let package = tauri::async_runtime::spawn_blocking(move || Package::open(&source))
                .await
                .map_err(|err| CommandError::from(Error::Package(err.to_string())))??;
            package.check_owner(session.me.id)?;
            Some((Arc::new(package), path.clone()))
        }
        None => None,
    };
    let info = UnfinishedRun::of(&saved);
    let (targets, filter, mut options) = (
        saved.targets.clone(),
        saved.filter.clone(),
        saved.resume_options(),
    );
    if let Some(settings) = &saved.options.backup_encryption {
        let passphrase = passphrase
            .ok_or_else(|| CommandError::other("the backup's passphrase is needed".into()))?
            .resolve()?;
        let keys = tauri::async_runtime::spawn_blocking({
            let settings = settings.clone();
            move || settings.unlock(passphrase)
        })
        .await
        .map_err(|err| CommandError::other(err.to_string()))?
        .map_err(CommandError::other)?;
        options.backup_keys = KeySlot(Some(Arc::new(keys)));
    }
    spawn_job(
        app,
        &state,
        package,
        targets,
        filter,
        options,
        Some(saved),
        None,
        None,
    )?;
    Ok(info)
}

/// Forgets the unfinished clean-up.
#[tauri::command]
pub fn discard_run(app: AppHandle) {
    if let Some(file) = run_state_file(&app) {
        let _ = std::fs::remove_file(file);
    }
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
pub async fn export_run(
    state: State<'_, AppState>,
    path: PathBuf,
    passphrase: Option<PassphraseInput>,
) -> CommandResult<u64> {
    let events = state.last_run();
    let passphrase = passphrase.map(PassphraseInput::resolve).transpose()?;
    tauri::async_runtime::spawn_blocking(move || -> std::io::Result<u64> {
        // `list.csv.age` is a CSV file, encrypted.
        let plain = if path.extension().is_some_and(|e| e == "age") {
            path.with_extension("")
        } else {
            path.clone()
        };
        let format = ExportFormat::for_path(&plain);
        let file = std::io::BufWriter::new(std::fs::File::create(&path)?);
        let rows = match passphrase {
            None => {
                let mut writer = ExportWriter::new(file, format)?;
                for event in &events {
                    writer.observe(event)?;
                }
                let rows = writer.rows();
                writer.finish()?;
                rows
            }
            Some(passphrase) => {
                let mut writer = ExportWriter::new(vault::encrypt(&passphrase, file)?, format)?;
                for event in &events {
                    writer.observe(event)?;
                }
                let rows = writer.rows();
                writer.finish()?.finish()?;
                rows
            }
        };
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
/// for [`preview_package`], [`start_package_job`] and Insights. Works
/// without logging in; then nothing is sent anywhere, and the servers and
/// DMs keep the names from the package.
#[tauri::command]
pub async fn import_package(
    state: State<'_, AppState>,
    path: PathBuf,
) -> CommandResult<PackageSummary> {
    let session = state.session().ok();
    let source = path.clone();
    let (package, index) = tauri::async_runtime::spawn_blocking(move || {
        let package = Arc::new(Package::open(&source)?);
        let index = Arc::new(Index::build(package.clone(), &chrono::Local));
        Ok::<_, Error>((package, index))
    })
    .await
    .map_err(|err| CommandError::from(Error::Package(err.to_string())))??;
    let mut targets = package.targets();
    let Some(session) = session else {
        let summary = PackageSummary {
            messages: package.message_count(),
            targets,
            left_servers: Vec::new(),
        };
        state.set_package(Some((package, path, index)));
        return Ok(summary);
    };
    package.check_owner(session.me.id)?;
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
    state.set_package(Some((package, path, index)));
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
) -> CommandResult<PackagePreview> {
    let session = state.session()?;
    let (package, _) = state.package().ok_or_else(CommandError::no_package)?;
    package.check_owner(session.me.id)?;
    let me = session.me.id;
    tauri::async_runtime::spawn_blocking(move || -> CommandResult<PackagePreview> {
        Ok(PackagePreview {
            entries: job::preview_package(&package, me, &targets, &filter)?,
            stats: scan::package_stats(&package, me, &targets, &filter)?,
        })
    })
    .await
    .map_err(|err| CommandError::other(err.to_string()))?
}

#[derive(Serialize)]
pub struct PackagePreview {
    entries: Vec<PreviewEntry>,
    stats: ScanStats,
}

/// Runs an Insights query on the package's index, off the main thread.
async fn insight<T: Send + 'static>(
    state: &AppState,
    query: impl FnOnce(&Index) -> T + Send + 'static,
) -> CommandResult<T> {
    let index = state.index().ok_or_else(CommandError::no_package)?;
    tauri::async_runtime::spawn_blocking(move || query(&index))
        .await
        .map_err(|err| CommandError::other(err.to_string()))
}

#[tauri::command]
pub async fn insights_info(state: State<'_, AppState>) -> CommandResult<Info> {
    insight(&state, |index| index.info()).await
}

#[tauri::command]
pub async fn insights_overview(
    state: State<'_, AppState>,
    scope: Scope,
) -> CommandResult<Overview> {
    insight(&state, move |index| index.overview(&scope)).await
}

#[tauri::command]
pub async fn insights_time(state: State<'_, AppState>, scope: Scope) -> CommandResult<Timeline> {
    insight(&state, move |index| index.timeline(&scope)).await
}

#[tauri::command]
pub async fn insights_places(
    state: State<'_, AppState>,
    scope: Scope,
) -> CommandResult<PlacesReport> {
    insight(&state, move |index| index.places(&scope)).await
}

#[tauri::command]
pub async fn insights_words(
    state: State<'_, AppState>,
    scope: Scope,
) -> CommandResult<WordsReport> {
    insight(&state, move |index| index.words(&scope)).await
}

#[tauri::command]
pub async fn insights_links(
    state: State<'_, AppState>,
    scope: Scope,
) -> CommandResult<LinksReport> {
    insight(&state, move |index| index.links(&scope)).await
}

#[tauri::command]
pub async fn insights_search(
    state: State<'_, AppState>,
    scope: Scope,
    query: String,
    limit: usize,
) -> CommandResult<SearchResult> {
    insight(&state, move |index| {
        index.search(&scope, &query, limit.min(500))
    })
    .await
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
