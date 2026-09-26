//! Command line front end for EraseCord.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, Months, NaiveDate, TimeDelta, Utc};
use clap::{Args, Parser, Subcommand};
use erasecord_core::job::{self, Event, Filter, JobControl, JobOptions, PreviewEntry, Stats};
use erasecord_core::{
    friends_without_dm, list_channels, list_targets, open_dm, Client, ClientConfig, Has, Notice,
    Package, PackageTarget, Snowflake, Target, TargetKind,
};
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(
    name = "erasecord",
    version,
    about = "Delete your own Discord messages from selected servers and DMs.",
    after_help = "The token is read from the DISCORD_TOKEN environment variable, or asked for \
                  (hidden) when it is not set. Automating a user account is against Discord's \
                  Terms of Service; use at your own risk."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List your servers and open DMs with their IDs.
    List {
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
        /// Also list friends whose DM is closed, with their user IDs for --dm-with.
        #[arg(long)]
        friends: bool,
        /// List what your Discord data package contains instead (no token needed).
        #[arg(long, value_name = "PATH", conflicts_with = "friends")]
        package: Option<PathBuf>,
    },
    /// List the channels of a server with their IDs, for --channel.
    Channels {
        /// Server ID (see `erasecord list`).
        #[arg(value_name = "SERVER_ID")]
        server: Snowflake,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Count your messages that match, without deleting anything.
    Preview {
        #[command(flatten)]
        selection: Selection,
        #[command(flatten)]
        range: Range,
        #[command(flatten)]
        content: Content,
    },
    /// Delete your messages that match.
    Delete {
        #[command(flatten)]
        selection: Selection,
        #[command(flatten)]
        range: Range,
        #[command(flatten)]
        content: Content,
        #[command(flatten)]
        options: DeleteOptions,
    },
}

#[derive(Args)]
struct Selection {
    /// Take the messages from your Discord data package (the .zip or its extracted folder)
    /// instead of searching: reaches every message it lists, also in closed DMs. Counting
    /// needs no token. IDs for --target and --channel: `erasecord list --package PATH`.
    #[arg(long, value_name = "PATH")]
    package: Option<PathBuf>,
    /// Server or DM channel ID to clean up; repeat for several. See `erasecord list`.
    #[arg(short, long = "target", value_name = "ID")]
    targets: Vec<Snowflake>,
    /// All servers you are a member of.
    #[arg(long)]
    all_servers: bool,
    /// All open DMs and group DMs.
    #[arg(long)]
    all_dms: bool,
    /// Only this channel of a server; repeat for several. Narrows its server down to the
    /// given channels (the server does not need to be selected with --target). Threads
    /// are separate channels. See `erasecord channels <SERVER_ID>`.
    #[arg(short, long = "channel", value_name = "ID")]
    channels: Vec<Snowflake>,
    /// The DM with this user, opening it if it is closed (only your own DM list changes).
    /// Repeat for several. See `erasecord list --friends`.
    #[arg(long = "dm-with", value_name = "USER_ID")]
    dm_with: Vec<Snowflake>,
}

#[derive(Args)]
struct Range {
    /// Only messages sent at or after DATE: YYYY-MM-DD (local time), an RFC 3339
    /// timestamp, or an age such as 30d, 12w, 6m or 1y.
    #[arg(long, value_name = "DATE", value_parser = parse_time)]
    after: Option<DateTime<Utc>>,
    /// Only messages sent before DATE (same formats). `--before 30d` keeps the last 30 days.
    #[arg(long, value_name = "DATE", value_parser = parse_time)]
    before: Option<DateTime<Utc>>,
}

#[derive(Args)]
struct Content {
    /// Only messages containing all of these words (any case), e.g. --contains "party tonight".
    #[arg(long, value_name = "WORDS")]
    contains: Option<String>,
    /// Only messages whose text matches this regular expression (any case). Checked by
    /// EraseCord only, so the counts in the preview can be too high.
    #[arg(long, value_name = "REGEX")]
    pattern: Option<String>,
    /// Only messages with any of these: link, file, image, video, sound, embed, sticker.
    /// Repeat or separate with commas.
    #[arg(long, value_name = "KIND", value_delimiter = ',')]
    has: Vec<Has>,
    /// Keep messages with any of these (same kinds as --has), e.g. --without image,video.
    #[arg(long, value_name = "KIND", value_delimiter = ',')]
    without: Vec<Has>,
}

#[derive(Args)]
struct DeleteOptions {
    /// Keep pinned messages.
    #[arg(long)]
    skip_pinned: bool,
    /// List what would be deleted without deleting anything.
    #[arg(long)]
    dry_run: bool,
    /// Edit each message before deleting it: replace the text with TEXT (random letters if
    /// left out) and remove attachments. Takes about twice as long.
    #[arg(long, value_name = "TEXT", num_args = 0..=1, default_missing_value = "")]
    overwrite: Option<String>,
    /// Do not ask for confirmation.
    #[arg(short, long)]
    yes: bool,
    /// Print every deleted or skipped message.
    #[arg(short, long)]
    verbose: bool,
    /// Pause after each deletion, in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = JobOptions::default().delete_delay_ms)]
    delete_delay: u64,
    /// Pause between search requests, in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = JobOptions::default().search_delay_ms)]
    search_delay: u64,
}

fn build_filter(range: &Range, content: &Content, skip_pinned: bool) -> Result<Filter> {
    if let (Some(after), Some(before)) = (range.after, range.before) {
        if after >= before {
            bail!("--after must be earlier than --before");
        }
    }
    let filter = Filter {
        after: range.after,
        before: range.before,
        skip_pinned,
        content: content.contains.clone(),
        pattern: content.pattern.clone(),
        has: content.has.clone(),
        without: content.without.clone(),
    };
    filter.compile()?;
    Ok(filter)
}

impl Command {
    fn package_path(&self) -> Option<&Path> {
        match self {
            Command::List { package, .. } => package.as_deref(),
            Command::Preview { selection, .. } | Command::Delete { selection, .. } => {
                selection.package.as_deref()
            }
            Command::Channels { .. } => None,
        }
    }

    /// Catches usage mistakes before logging in.
    fn check(&self) -> Result<()> {
        let (selection, range, content, needs_confirmation) = match self {
            Command::List { .. } | Command::Channels { .. } => return Ok(()),
            Command::Preview {
                selection,
                range,
                content,
            } => (selection, range, content, false),
            Command::Delete {
                selection,
                range,
                content,
                options,
            } => (selection, range, content, !options.dry_run && !options.yes),
        };
        if selection.targets.is_empty()
            && selection.channels.is_empty()
            && selection.dm_with.is_empty()
            && !selection.all_servers
            && !selection.all_dms
        {
            bail!("choose what to clean up: --target <ID>, --channel <ID>, --dm-with <USER_ID>, --all-servers and/or --all-dms (see `erasecord list`)");
        }
        let filter = build_filter(range, content, false)?;
        if selection.package.is_some() {
            if !selection.dm_with.is_empty() {
                bail!("--dm-with does not work with --package; the package already contains closed DMs");
            }
            filter.compile_for_package()?;
        }
        if needs_confirmation && !io::stdin().is_terminal() {
            bail!("refusing to delete without confirmation; pass --yes to skip it");
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode> {
    cli.command.check()?;
    let package = match cli.command.package_path() {
        Some(path) => Some(Arc::new(load_package(path)?)),
        None => None,
    };
    // Reading the package and counting in it needs no Discord account.
    let offline =
        package.is_some() && matches!(cli.command, Command::List { .. } | Command::Preview { .. });
    let session = if offline {
        None
    } else {
        let mut config = ClientConfig::default();
        // For testing against a fake API server.
        if let Ok(api_base) = std::env::var("ERASECORD_API_BASE") {
            config.api_base = api_base;
        }
        let client = Client::with_config(&read_token()?, config)?;
        let me = client.current_user().await.context("could not log in")?;
        eprintln!("Logged in as {} ({})", me.display_name(), me.id);
        Some((client, me.id))
    };
    let online = || session.clone().expect("logged in");

    let control = JobControl::new();
    let graceful = Arc::new(AtomicBool::new(false));
    handle_ctrl_c(control.clone(), graceful.clone());

    match cli.command {
        Command::List { json, friends, .. } => {
            if let Some(package) = &package {
                let targets = package.targets();
                if json {
                    println!("{}", serde_json::to_string_pretty(&targets)?);
                } else {
                    print_package_targets(&targets);
                }
                return Ok(ExitCode::SUCCESS);
            }
            let (client, _) = online();
            let targets = list_targets(&client).await?;
            let friends = if friends {
                Some(friends_without_dm(&client).await?)
            } else {
                None
            };
            if json {
                let value = match friends {
                    Some(friends) => {
                        serde_json::json!({ "targets": targets, "friends_without_dm": friends })
                    }
                    None => serde_json::to_value(&targets)?,
                };
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                print_targets(&targets);
                if let Some(friends) = friends {
                    println!();
                    println!("Friends without an open DM (use --dm-with <USER_ID>):");
                    for friend in friends {
                        println!("{:<9} {:<20} {}", "friend", friend.user_id, friend.name);
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Channels { server, json } => {
            let (client, _) = online();
            let channels = list_channels(&client, server)
                .await
                .with_context(|| format!("could not list the channels of {server}"))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&channels)?);
            } else {
                println!("{:<20} {:<24} NAME", "ID", "CATEGORY");
                for channel in channels {
                    let category = channel.category.as_deref().unwrap_or("-");
                    println!("{:<20} {category:<24} #{}", channel.id, channel.name);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Preview {
            selection,
            range,
            content,
        } => {
            let filter = build_filter(&range, &content, false)?;
            if let Some(package) = &package {
                let targets = select_from_package(package, &selection)?;
                let me = session.as_ref().map_or(Snowflake(0), |(_, me)| *me);
                print_package_preview(package, me, &targets, &filter)?;
                return Ok(ExitCode::SUCCESS);
            }
            let (client, me) = online();
            let targets = select(&client, &selection).await?;
            preview(
                &client,
                me,
                &targets,
                &filter,
                &JobOptions::default(),
                &control,
            )
            .await?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Delete {
            selection,
            range,
            content,
            options,
        } => {
            let (client, me) = online();
            let targets = match &package {
                Some(package) => select_from_package(package, &selection)?,
                None => select(&client, &selection).await?,
            };
            let filter = build_filter(&range, &content, options.skip_pinned)?;
            let job_options = JobOptions {
                delete_delay_ms: options.delete_delay,
                search_delay_ms: options.search_delay,
                dry_run: options.dry_run,
                overwrite: options.overwrite.clone(),
                ..Default::default()
            };
            if !options.dry_run && !options.yes {
                let total = match &package {
                    Some(package) => print_package_preview(package, me, &targets, &filter)?,
                    None => preview(&client, me, &targets, &filter, &job_options, &control).await?,
                };
                if total == 0 {
                    println!("Nothing to delete.");
                    return Ok(ExitCode::SUCCESS);
                }
                if !confirm(total)? {
                    println!("Nothing was deleted.");
                    return Ok(ExitCode::SUCCESS);
                }
            }
            graceful.store(true, Ordering::SeqCst);
            delete(
                client,
                me,
                package,
                targets,
                filter,
                job_options,
                control,
                options.verbose,
            )
            .await
        }
    }
}

fn load_package(path: &Path) -> Result<Package> {
    eprintln!("Reading {}…", path.display());
    let package = Package::open(path)?;
    eprintln!(
        "The data package lists {} messages in {} channels.",
        package.message_count(),
        package.channels.len()
    );
    Ok(package)
}

fn read_token() -> Result<String> {
    if let Ok(token) = std::env::var("DISCORD_TOKEN") {
        if !token.trim().is_empty() {
            return Ok(token);
        }
    }
    if !io::stdin().is_terminal() {
        bail!("set DISCORD_TOKEN, or run erasecord in a terminal to type the token");
    }
    Ok(rpassword::prompt_password(
        "Discord token (input hidden): ",
    )?)
}

/// Before deleting starts, Ctrl+C quits at once. While deleting, the first
/// Ctrl+C stops after the current request and the second quits.
fn handle_ctrl_c(control: JobControl, graceful: Arc<AtomicBool>) {
    tokio::spawn(async move {
        while tokio::signal::ctrl_c().await.is_ok() {
            if graceful.load(Ordering::SeqCst) && !control.is_cancelled() {
                eprintln!("\nStopping after the current request; press Ctrl+C again to quit now.");
                control.cancel();
            } else {
                eprintln!();
                std::process::exit(130);
            }
        }
    });
}

async fn select(client: &Client, selection: &Selection) -> Result<Vec<Target>> {
    let available = list_targets(client).await?;
    if let Some(unknown) = selection
        .targets
        .iter()
        .find(|id| !available.iter().any(|t| t.id == **id))
    {
        bail!("{unknown} is neither one of your servers nor an open DM (see `erasecord list`)");
    }
    let mut chosen: Vec<Target> = available
        .iter()
        .filter(|t| {
            selection.targets.contains(&t.id)
                || (selection.all_servers && t.kind == TargetKind::Guild)
                || (selection.all_dms && t.kind != TargetKind::Guild)
        })
        .cloned()
        .collect();
    for &channel_id in &selection.channels {
        let channel = client
            .channel(channel_id)
            .await
            .with_context(|| format!("could not look up channel {channel_id}"))?;
        let Some(guild_id) = channel.guild_id else {
            bail!("{channel_id} is not a server channel; choose DMs with --target");
        };
        let index = match chosen.iter().position(|t| t.id == guild_id) {
            Some(index) => index,
            None => {
                let Some(guild) = available.iter().find(|t| t.id == guild_id) else {
                    bail!("channel {channel_id} belongs to a server you are not a member of");
                };
                chosen.push(guild.clone());
                chosen.len() - 1
            }
        };
        if !chosen[index].channels.contains(&channel_id) {
            chosen[index].channels.push(channel_id);
        }
    }
    for &user_id in &selection.dm_with {
        let target = open_dm(client, user_id)
            .await
            .with_context(|| format!("could not open the DM with user {user_id}"))?;
        if !chosen.iter().any(|t| t.id == target.id) {
            chosen.push(target);
        }
    }
    Ok(chosen)
}

/// Prints the number of matching messages per target and returns the total.
async fn preview(
    client: &Client,
    me: Snowflake,
    targets: &[Target],
    filter: &Filter,
    options: &JobOptions,
    control: &JobControl,
) -> Result<u64> {
    eprintln!(
        "Counting matching messages in {} server(s)/DM(s)…",
        targets.len()
    );
    let entries = job::preview(client, me, targets, filter, options, control, |_, entry| {
        print_preview_entry(entry)
    })
    .await?;
    let total = entries.iter().filter_map(|e| e.count).sum();
    println!("{:>8}  total", total);
    if filter.skip_pinned {
        println!("          (pinned messages are included in the counts but will be kept)");
    }
    if filter.checks_locally() {
        println!(
            "          (--pattern/--without are applied while deleting; fewer messages may match)"
        );
    }
    Ok(total)
}

fn print_preview_entry(entry: &PreviewEntry) {
    let name = target_label(&entry.target);
    match (entry.count, &entry.error) {
        (Some(count), _) => println!("{count:>8}  {name}"),
        (None, error) => println!(
            "{:>8}  {name} (could not search: {})",
            "?",
            error.as_deref().unwrap_or("unknown error")
        ),
    }
}

fn target_label(target: &Target) -> String {
    match target.channels.len() {
        0 => target.name.clone(),
        1 => format!("{} (1 channel)", target.name),
        n => format!("{} ({n} channels)", target.name),
    }
}

/// The package's servers and DMs picked by `selection`.
fn select_from_package(package: &Package, selection: &Selection) -> Result<Vec<Target>> {
    let available = package.targets();
    if let Some(unknown) = selection
        .targets
        .iter()
        .find(|id| !available.iter().any(|t| t.target.id == **id))
    {
        bail!("{unknown} is not in the data package (see `erasecord list --package PATH`)");
    }
    let mut chosen: Vec<Target> = available
        .iter()
        .map(|t| &t.target)
        .filter(|t| {
            selection.targets.contains(&t.id)
                || (selection.all_servers && t.kind == TargetKind::Guild)
                || (selection.all_dms && t.kind != TargetKind::Guild)
        })
        .cloned()
        .collect();
    for &channel_id in &selection.channels {
        let Some(owner) = available
            .iter()
            .find(|t| t.channels.iter().any(|c| c.id == channel_id))
        else {
            bail!("channel {channel_id} is not in the data package");
        };
        if owner.target.kind != TargetKind::Guild {
            bail!("{channel_id} is a DM; choose it with --target");
        }
        let index = match chosen.iter().position(|t| t.id == owner.target.id) {
            Some(index) => index,
            None => {
                chosen.push(owner.target.clone());
                chosen.len() - 1
            }
        };
        if !chosen[index].channels.contains(&channel_id) {
            chosen[index].channels.push(channel_id);
        }
    }
    Ok(chosen)
}

/// Prints the exact number of matching messages per target and returns the total.
fn print_package_preview(
    package: &Package,
    me: Snowflake,
    targets: &[Target],
    filter: &Filter,
) -> Result<u64> {
    let entries = job::preview_package(package, me, targets, filter)?;
    for entry in &entries {
        print_preview_entry(entry);
    }
    let total = entries.iter().filter_map(|e| e.count).sum();
    println!("{:>8}  total", total);
    if filter.skip_pinned {
        println!("          (pinned messages are included in the counts but will be kept)");
    }
    Ok(total)
}

fn print_package_targets(targets: &[PackageTarget]) {
    println!("{:<9} {:<20} {:>9}  NAME", "KIND", "ID", "MESSAGES");
    for item in targets {
        let kind = match item.target.kind {
            TargetKind::Guild => "server",
            TargetKind::Dm => "dm",
            TargetKind::GroupDm => "group-dm",
        };
        println!(
            "{kind:<9} {:<20} {:>9}  {}",
            item.target.id, item.messages, item.target.name
        );
        if item.target.kind == TargetKind::Guild && item.channels.len() > 1 {
            for channel in &item.channels {
                println!(
                    "{:<9} {:<20} {:>9}  #{}",
                    "  channel", channel.id, channel.messages, channel.name
                );
            }
        }
    }
}

fn confirm(total: u64) -> Result<bool> {
    print!("Permanently delete up to {total} messages? Type 'delete' to continue: ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(answer.trim().eq_ignore_ascii_case("delete"))
}

#[allow(clippy::too_many_arguments)]
async fn delete(
    client: Client,
    me: Snowflake,
    package: Option<Arc<Package>>,
    targets: Vec<Target>,
    filter: Filter,
    options: JobOptions,
    control: JobControl,
    verbose: bool,
) -> Result<ExitCode> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let dry_run = options.dry_run;
    let target_count = targets.len();
    let task = tokio::spawn(async move {
        match package {
            Some(package) => {
                job::run_package(
                    &client, me, &package, &targets, &filter, &options, &control, tx,
                )
                .await
            }
            None => job::run(&client, me, &targets, &filter, &options, &control, tx).await,
        }
    });

    let mut progress = Progress::new(target_count, verbose || dry_run, dry_run);
    while let Some(event) = rx.recv().await {
        progress.handle(&event);
    }
    let summary = task.await?;

    println!(
        "Done: {} {}, {} skipped, {} failed.",
        summary.stats.deleted, progress.deleted_label, summary.stats.skipped, summary.stats.failed
    );
    if let Some(error) = summary.error {
        bail!("stopped early: {error}");
    }
    if summary.cancelled {
        println!("Stopped before the end.");
        return Ok(ExitCode::from(130));
    }
    Ok(ExitCode::SUCCESS)
}

/// Prints job events; on a terminal it keeps a live status line at the bottom.
struct Progress {
    target_count: usize,
    verbose: bool,
    /// "deleted", or "would be deleted" in a dry run.
    deleted_label: &'static str,
    live: bool,
    estimate: Option<u64>,
    stats: Stats,
}

impl Progress {
    fn new(target_count: usize, verbose: bool, dry_run: bool) -> Self {
        Progress {
            target_count,
            verbose,
            deleted_label: if dry_run {
                "would be deleted"
            } else {
                "deleted"
            },
            live: io::stderr().is_terminal(),
            estimate: None,
            stats: Stats::default(),
        }
    }

    fn handle(&mut self, event: &Event) {
        match event {
            Event::TargetStarted { index, name, .. } => {
                self.estimate = None;
                self.stats = Stats::default();
                self.line(format!("[{}/{}] {name}", index + 1, self.target_count));
            }
            Event::TargetEstimate { total, .. } => self.estimate = Some(*total),
            Event::Deleted {
                sent_at,
                preview,
                dry_run,
                ..
            } => {
                self.stats.deleted += 1;
                if self.verbose {
                    let verb = if *dry_run { "would delete" } else { "deleted" };
                    let sent = sent_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
                    self.line(format!("  {verb} {sent}  {preview}"));
                }
            }
            Event::Skipped {
                message_id, reason, ..
            } => {
                self.stats.skipped += 1;
                if self.verbose {
                    self.line(format!("  skipped {message_id} ({})", reason.describe()));
                }
            }
            Event::Failed {
                message_id, error, ..
            } => {
                self.stats.failed += 1;
                self.line(format!("  failed to delete {message_id}: {error}"));
            }
            Event::TargetFailed { error, .. } => self.line(format!("  could not search: {error}")),
            Event::ChannelUnreachable {
                channel_id,
                messages,
                error,
                ..
            } => {
                self.stats.skipped += messages;
                self.line(format!(
                    "  skipped {messages} message(s) in channel {channel_id}: {error}"
                ));
            }
            Event::TargetFinished { stats, .. } => self.line(format!(
                "  {} {}, {} skipped, {} failed",
                stats.deleted, self.deleted_label, stats.skipped, stats.failed
            )),
            Event::Notice { notice } => self.line(format!("  {}", describe_notice(notice))),
            Event::Finished(_) => {
                self.clear_status();
                return;
            }
        }
        self.status();
    }

    fn line(&self, text: String) {
        self.clear_status();
        eprintln!("{text}");
    }

    fn status(&self) {
        if !self.live {
            return;
        }
        let of = self
            .estimate
            .map(|n| format!(" of ~{n}"))
            .unwrap_or_default();
        let status = format!(
            "  {}{of} {}, {} skipped, {} failed",
            self.stats.deleted, self.deleted_label, self.stats.skipped, self.stats.failed
        );
        eprint!("\r{status:<60}");
        let _ = io::stderr().flush();
    }

    fn clear_status(&self) {
        if self.live {
            eprint!("\r{:<60}\r", "");
        }
    }
}

fn describe_notice(notice: &Notice) -> String {
    let seconds = |ms: &u64| *ms as f64 / 1000.0;
    match notice {
        Notice::RateLimited { wait_ms, global } => format!(
            "rate limited{}, waiting {:.1}s",
            if *global { " (global)" } else { "" },
            seconds(wait_ms)
        ),
        Notice::IndexNotReady { wait_ms } => {
            format!(
                "Discord is still indexing messages, waiting {:.1}s",
                seconds(wait_ms)
            )
        }
        Notice::Retrying {
            reason,
            attempt,
            wait_ms,
        } => format!("{reason}; retry {attempt} in {:.1}s", seconds(wait_ms)),
    }
}

fn print_targets(targets: &[Target]) {
    println!("{:<9} {:<20} NAME", "KIND", "ID");
    for target in targets {
        let kind = match target.kind {
            TargetKind::Guild => "server",
            TargetKind::Dm => "dm",
            TargetKind::GroupDm => "group-dm",
        };
        println!("{kind:<9} {:<20} {}", target.id, target.name);
    }
}

fn parse_time(input: &str) -> Result<DateTime<Utc>, String> {
    let input = input.trim();
    if let Some(time) = parse_age(input) {
        return time;
    }
    if let Ok(time) = DateTime::parse_from_rfc3339(input) {
        return Ok(time.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        return date
            .and_hms_opt(0, 0, 0)
            .and_then(|t| t.and_local_timezone(Local).earliest())
            .map(|t| t.with_timezone(&Utc))
            .ok_or_else(|| format!("{input} does not exist in your time zone"));
    }
    Err(format!(
        "expected YYYY-MM-DD, an RFC 3339 timestamp or an age like 30d/12w/6m/1y, got {input:?}"
    ))
}

/// "30d", "12w", "6m", "1y": that long before now.
fn parse_age(input: &str) -> Option<Result<DateTime<Utc>, String>> {
    let unit = input.chars().last()?;
    let amount: u32 = input[..input.len() - unit.len_utf8()].parse().ok()?;
    let now = Utc::now();
    let time = match unit {
        'd' => now.checked_sub_signed(TimeDelta::days(amount.into())),
        'w' => now.checked_sub_signed(TimeDelta::weeks(amount.into())),
        'm' => now.checked_sub_months(Months::new(amount)),
        'y' => now.checked_sub_months(Months::new(amount.checked_mul(12)?)),
        _ => return None,
    };
    Some(time.ok_or_else(|| format!("{input} is too far in the past")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ages() {
        let thirty_days = parse_time("30d").unwrap();
        let expected = Utc::now() - TimeDelta::days(30);
        assert!((thirty_days - expected).num_seconds().abs() < 5);
        assert!(parse_time("2w").unwrap() < parse_time("1w").unwrap());
        assert!(parse_time("1y").unwrap() < parse_time("11m").unwrap());
    }

    #[test]
    fn parses_dates_and_timestamps() {
        assert_eq!(
            parse_time("2024-03-01T12:00:00Z").unwrap().to_rfc3339(),
            "2024-03-01T12:00:00+00:00"
        );
        let local_midnight = parse_time("2024-03-01").unwrap().with_timezone(&Local);
        assert_eq!(
            local_midnight.format("%Y-%m-%d %H:%M").to_string(),
            "2024-03-01 00:00"
        );
    }

    #[test]
    fn rejects_garbage() {
        for input in ["", "d", "yesterday", "30x", "2024-13-01", "ü"] {
            assert!(parse_time(input).is_err(), "{input:?} should be rejected");
        }
    }

    #[test]
    fn after_must_precede_before() {
        let range = Range {
            after: parse_time("2024-02-01").ok(),
            before: parse_time("2024-01-01").ok(),
        };
        assert!(build_filter(&range, &no_content(), false).is_err());
    }

    fn no_content() -> Content {
        Content {
            contains: None,
            pattern: None,
            has: vec![],
            without: vec![],
        }
    }

    #[test]
    fn overwrite_text_is_optional() {
        let parse = |extra: &[&str]| {
            let args = [&["erasecord", "delete", "--all-dms"][..], extra].concat();
            match Cli::try_parse_from(args).unwrap().command {
                Command::Delete { options, .. } => options.overwrite,
                _ => panic!("expected delete"),
            }
        };
        assert_eq!(parse(&[]), None);
        assert_eq!(parse(&["--overwrite"]), Some(String::new()));
        assert_eq!(parse(&["--overwrite", "gone"]), Some("gone".into()));
        assert_eq!(parse(&["--overwrite", "--dry-run"]), Some(String::new()));
    }

    #[test]
    fn content_flags_parse() {
        let cli = Cli::try_parse_from([
            "erasecord",
            "preview",
            "--all-dms",
            "--has",
            "link,image",
            "--without",
            "file",
            "--pattern",
            "^gg",
        ])
        .unwrap();
        let Command::Preview { range, content, .. } = cli.command else {
            panic!("expected preview");
        };
        let filter = build_filter(&range, &content, false).unwrap();
        assert_eq!(filter.has, [Has::Link, Has::Image]);
        assert_eq!(filter.without, [Has::File]);
        let bad = Content {
            pattern: Some("(".into()),
            ..no_content()
        };
        assert!(build_filter(&range, &bad, false).is_err());
    }
}
