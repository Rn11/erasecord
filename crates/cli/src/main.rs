//! Command line front end for EraseCord.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, Months, NaiveDate, TimeDelta, Utc};
use clap::{Args, Parser, Subcommand};
use erasecord_core::insights::Index;
use erasecord_core::job::{self, Event, Filter, JobControl, JobOptions, PreviewEntry, Stats};
use erasecord_core::vault::{self, EncryptedBackupSettings, KeySlot, SecretString};
use erasecord_core::{
    friends_without_dm, list_channels, list_targets, open_dm, Activity, Client, ClientConfig,
    ExportFormat, ExportWriter, Has, Notice, Package, PackageTarget, SavedRun, Snowflake, Target,
    TargetKind,
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
    /// Statistics about the messages in your Discord data package: when, where and
    /// what you wrote. Needs no token; nothing is sent anywhere.
    Stats {
        /// The data package (.zip or extracted folder).
        #[arg(value_name = "PATH")]
        package: PathBuf,
        #[command(flatten)]
        scope: ScopeArgs,
        /// What to show.
        #[arg(long, value_enum, default_value = "overview")]
        section: Section,
        /// Print JSON instead of a summary.
        #[arg(long)]
        json: bool,
    },
    /// Find your messages in the data package that contain all of these words, in
    /// any case (the same rule as `delete --contains`). Needs no token.
    Search {
        /// The data package (.zip or extracted folder).
        #[arg(value_name = "PATH")]
        package: PathBuf,
        /// The words to look for.
        #[arg(value_name = "WORDS", required = true)]
        words: Vec<String>,
        #[command(flatten)]
        scope: ScopeArgs,
        /// Show at most this many messages, newest first.
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Print JSON instead of a list.
        #[arg(long)]
        json: bool,
    },
    /// Describe the structure of a data package without any of its content: file and
    /// field names, counts and kinds of values, but no messages, names, IDs or dates.
    /// Safe to share, e.g. to help support a new package format. Needs no token.
    InspectPackage {
        /// The data package (.zip or extracted folder).
        #[arg(value_name = "PATH")]
        package: PathBuf,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Write a copy of a data package with every value replaced: random words for
    /// text, placeholders for names, new IDs and links, dates moved back by a random
    /// number of weeks. For testing with realistic data. Needs no token.
    AnonymizePackage {
        /// The data package (.zip or extracted folder).
        #[arg(value_name = "PATH")]
        package: PathBuf,
        /// The new .zip file to write.
        #[arg(value_name = "OUTPUT")]
        output: PathBuf,
    },
    /// Decrypt and unpack an encrypted backup (erasecord-backup-….tar.age, or a
    /// ….parts folder of a backup that was not finished) or an encrypted export.
    OpenBackup {
        /// The .tar.age archive, .parts folder or encrypted export.
        #[arg(value_name = "PATH")]
        path: PathBuf,
        /// The folder to unpack into.
        #[arg(long, value_name = "DIR")]
        to: PathBuf,
        /// Read the passphrase from the first line of FILE instead of asking.
        #[arg(long, value_name = "FILE")]
        passphrase_file: Option<PathBuf>,
    },
    /// Continue a clean-up that was started with `delete --state FILE` and did not
    /// finish, with exactly the servers, DMs and conditions it had.
    Resume {
        /// The state file given to `delete --state`.
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Read the passphrase of the encrypted backup from the first line of FILE.
        #[arg(long, value_name = "PASSFILE")]
        passphrase_file: Option<PathBuf>,
        /// Print every deleted or skipped message.
        #[arg(short, long)]
        verbose: bool,
    },
}

#[derive(Args, Clone)]
struct ScopeArgs {
    /// Only from this day on (YYYY-MM-DD, local time).
    #[arg(long, value_name = "DATE")]
    from: Option<NaiveDate>,
    /// Only up to and including this day.
    #[arg(long, value_name = "DATE")]
    to: Option<NaiveDate>,
    /// Only this server or DM (ID from `erasecord list --package`); repeat for several.
    #[arg(long = "place", value_name = "ID")]
    places: Vec<u64>,
    /// Only this channel of a server; repeat for several.
    #[arg(long = "channel", value_name = "ID")]
    channels: Vec<u64>,
}

impl ScopeArgs {
    fn scope(&self) -> erasecord_core::insights::Scope {
        erasecord_core::insights::Scope {
            from: self.from,
            to: self.to,
            places: self.places.iter().copied().map(Snowflake).collect(),
            channels: self.channels.iter().copied().map(Snowflake).collect(),
        }
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Section {
    /// The servers and DMs, time range and totals.
    Info,
    Overview,
    /// Per month, week, weekday and hour.
    Time,
    Places,
    /// Words, emoji, mentions and message lengths.
    Words,
    /// Links and attachments.
    Links,
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
    /// Save every deleted message (or, with --dry-run, every message that would be deleted),
    /// with its text and attachment links, to FILE: JSON for a .json file, CSV otherwise.
    /// Encrypted (FILE.age) unless --no-encrypt.
    #[arg(long, value_name = "FILE")]
    export: Option<PathBuf>,
    /// Before deleting a message, save its attachments (a message whose files cannot be saved
    /// is kept) and the list of messages into one encrypted archive in DIR,
    /// erasecord-backup-<time>.tar.age; see `erasecord open-backup`. With --no-encrypt,
    /// plain files and messages-<time>.json instead. With --dry-run this only backs up.
    #[arg(long, value_name = "DIR")]
    backup: Option<PathBuf>,
    /// With --backup: in DMs and group DMs, also save the attachments the others sent in the
    /// chosen time range (their messages are not touched). Never on servers.
    #[arg(long, requires = "backup")]
    backup_others: bool,
    /// Write the backup and --export without encryption. Not recommended: they contain
    /// your messages.
    #[arg(long)]
    no_encrypt: bool,
    /// Read the passphrase that encrypts the backup and --export from the first line of FILE,
    /// instead of asking for it.
    #[arg(long, value_name = "FILE")]
    passphrase_file: Option<PathBuf>,
    /// Keep track of the progress in FILE, so that a run that is stopped or cut short can be
    /// continued with `erasecord resume FILE`. The file is removed once everything is done.
    #[arg(long, value_name = "FILE")]
    state: Option<PathBuf>,
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
            Command::Stats { package, .. } | Command::Search { package, .. } => Some(package),
            Command::Channels { .. }
            | Command::Resume { .. }
            | Command::InspectPackage { .. }
            | Command::OpenBackup { .. }
            | Command::AnonymizePackage { .. } => None,
        }
    }

    /// Catches usage mistakes before logging in.
    fn check(&self) -> Result<()> {
        let (selection, range, content, needs_confirmation) = match self {
            // Read the state file before logging in, so a wrong one fails fast.
            Command::Resume { file, .. } => {
                SavedRun::load(file)
                    .with_context(|| format!("cannot read the state file {}", file.display()))?;
                return Ok(());
            }
            Command::List { .. }
            | Command::Channels { .. }
            | Command::Stats { .. }
            | Command::Search { .. }
            | Command::InspectPackage { .. }
            | Command::OpenBackup { .. }
            | Command::AnonymizePackage { .. } => return Ok(()),
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
            } => {
                if options.state.is_some() && options.dry_run {
                    bail!("--state is for real runs; a dry run has nothing to continue");
                }
                (selection, range, content, !options.dry_run && !options.yes)
            }
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
    match &cli.command {
        Command::InspectPackage { package, json } => {
            eprintln!("Reading {}…", package.display());
            let report = erasecord_core::inspect::inspect(package)?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", report.to_text());
            }
            return Ok(ExitCode::SUCCESS);
        }
        Command::OpenBackup {
            path,
            to,
            passphrase_file,
        } => {
            let passphrase = ask_passphrase(passphrase_file.as_deref(), false)?;
            eprintln!("Decrypting {}…", path.display());
            let opened = vault::open(path, &passphrase, to).map_err(anyhow::Error::msg)?;
            println!(
                "Unpacked {} file(s) and {} message(s) into {}.",
                opened.files,
                opened.messages,
                opened.folder.display()
            );
            return Ok(ExitCode::SUCCESS);
        }
        Command::AnonymizePackage { package, output } => {
            eprintln!("Reading {}…", package.display());
            let summary = erasecord_core::anonymize::anonymize(package, output)?;
            println!(
                "Wrote an anonymized copy to {}: {} files with {} records.",
                output.display(),
                summary.files,
                summary.records
            );
            println!(
                "Every text, name, ID, link and number was replaced, and dates were moved back \
                 by a random number of weeks (not recorded anywhere)."
            );
            if !summary.omitted.is_empty() {
                let omitted: Vec<String> = summary
                    .omitted
                    .iter()
                    .map(|(kind, count)| format!("{count} {kind}"))
                    .collect();
                println!("Left out: {}.", omitted.join(", "));
            }
            println!("Please look through the copy before you share it.");
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }
    cli.command.check()?;
    let package = match cli.command.package_path() {
        Some(path) => Some(Arc::new(load_package(path)?)),
        None => None,
    };
    // Reading the package and counting in it needs no Discord account.
    let offline = package.is_some()
        && matches!(
            cli.command,
            Command::List { .. }
                | Command::Preview { .. }
                | Command::Stats { .. }
                | Command::Search { .. }
        );
    let session = if offline {
        None
    } else {
        let mut config = ClientConfig::default();
        // For testing against a fake API server.
        if let Ok(api_base) = std::env::var("ERASECORD_API_BASE") {
            config.api_base = api_base;
        }
        let client = Client::with_config(&read_token()?, config)?;
        // Say why logging in takes long (no connection, Discord down).
        client.set_notice_sink(Some(Arc::new(|notice| {
            eprintln!("  {}", describe_notice(&notice));
        })));
        let me = client.current_user().await.context("could not log in")?;
        client.set_notice_sink(None);
        eprintln!("Logged in as {} ({})", me.display_name(), me.id);
        Some((client, me.id))
    };
    let online = || session.clone().expect("logged in");

    let control = JobControl::new();
    let graceful = Arc::new(AtomicBool::new(false));
    handle_ctrl_c(control.clone(), graceful.clone());

    match cli.command {
        Command::InspectPackage { .. }
        | Command::AnonymizePackage { .. }
        | Command::OpenBackup { .. } => {
            unreachable!("handled before logging in")
        }
        Command::Stats {
            scope,
            section,
            json,
            ..
        } => {
            let index = Index::build(package.expect("package loaded"), &Local);
            let scope = scope.scope();
            let places = index.info().places;
            let print = |value: serde_json::Value| -> Result<()> {
                println!("{}", serde_json::to_string_pretty(&value)?);
                Ok(())
            };
            match section {
                Section::Info => print(serde_json::to_value(index.info())?)?,
                Section::Overview if json => print(serde_json::to_value(index.overview(&scope))?)?,
                Section::Overview => print_overview(&index.overview(&scope), &places),
                Section::Time if json => print(serde_json::to_value(index.timeline(&scope))?)?,
                Section::Time => print_timeline(&index.timeline(&scope)),
                Section::Places if json => print(serde_json::to_value(index.places(&scope))?)?,
                Section::Places => print_places(&index.places(&scope), &places),
                Section::Words if json => print(serde_json::to_value(index.words(&scope))?)?,
                Section::Words => print_words(&index.words(&scope)),
                Section::Links if json => print(serde_json::to_value(index.links(&scope))?)?,
                Section::Links => print_links(&index.links(&scope)),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Search {
            words,
            scope,
            limit,
            json,
            ..
        } => {
            let index = Index::build(package.expect("package loaded"), &Local);
            let found = index.search(&scope.scope(), &words.join(" "), limit);
            if json {
                println!("{}", serde_json::to_string_pretty(&found)?);
                return Ok(ExitCode::SUCCESS);
            }
            let places = index.info().places;
            println!("{} messages found", found.total);
            for m in &found.messages {
                println!(
                    "\n{}  {} · {}\n  {}",
                    m.sent_at.with_timezone(&Local).format("%Y-%m-%d %H:%M"),
                    places[m.place].name,
                    m.channel,
                    m.text.replace('\n', "\n  ")
                );
            }
            Ok(ExitCode::SUCCESS)
        }
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
                        println!(
                            "{:<9} {:<20} {}",
                            "friend",
                            friend.user_id,
                            safe(&friend.name)
                        );
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
                    let category = safe(channel.category.as_deref().unwrap_or("-"));
                    println!("{:<20} {category:<24} #{}", channel.id, safe(&channel.name));
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
            if let Some(package) = &package {
                package
                    .check_owner(me)
                    .context("cannot use this data package")?;
            }
            let targets = match &package {
                Some(package) => select_from_package(package, &selection)?,
                None => select(&client, &selection).await?,
            };
            let filter = build_filter(&range, &content, options.skip_pinned)?;
            let encrypt =
                !options.no_encrypt && (options.backup.is_some() || options.export.is_some());
            let mut job_options = JobOptions {
                delete_delay_ms: options.delete_delay,
                search_delay_ms: options.search_delay,
                dry_run: options.dry_run,
                overwrite: options.overwrite.clone(),
                backup_dir: options.backup.clone(),
                backup_others: options.backup_others,
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
            let passphrase = if encrypt {
                Some(ask_passphrase(options.passphrase_file.as_deref(), true)?)
            } else {
                None
            };
            if let (Some(dir), Some(passphrase)) = (&options.backup, &passphrase) {
                let (settings, keys) = EncryptedBackupSettings::create(dir, passphrase.clone())
                    .with_context(|| format!("cannot create the backup in {}", dir.display()))?;
                eprintln!(
                    "The backup is encrypted and becomes {} at the end.",
                    settings.archive.display()
                );
                job_options.backup_encryption = Some(settings);
                job_options.backup_keys = KeySlot(Some(Arc::new(keys)));
            }
            let state = match &options.state {
                Some(file) => {
                    // Absolute, so `resume` works from any folder.
                    let package_path = match &selection.package {
                        Some(path) => Some(std::fs::canonicalize(path)?),
                        None => None,
                    };
                    let saved =
                        SavedRun::new(&targets, &filter, &job_options, package_path.as_deref());
                    Some((file.clone(), saved))
                }
                None => None,
            };
            graceful.store(true, Ordering::SeqCst);
            delete(Plan {
                client,
                me,
                package,
                targets,
                filter,
                options: job_options,
                control,
                verbose: options.verbose,
                export: options.export,
                export_passphrase: passphrase,
                state,
            })
            .await
        }
        Command::Resume {
            file,
            verbose,
            passphrase_file,
        } => {
            let (client, me) = online();
            let saved = SavedRun::load(&file)
                .with_context(|| format!("cannot read the state file {}", file.display()))?;
            let mut options = saved.resume_options();
            if let Some(settings) = &saved.options.backup_encryption {
                eprintln!("The backup of this clean-up is encrypted.");
                let passphrase = ask_passphrase(passphrase_file.as_deref(), false)?;
                let keys = settings.unlock(passphrase).map_err(anyhow::Error::msg)?;
                options.backup_keys = KeySlot(Some(Arc::new(keys)));
            }
            let package = match &saved.package {
                Some(path) => {
                    let package = load_package(path)?;
                    package
                        .check_owner(me)
                        .context("cannot use this data package")?;
                    Some(Arc::new(package))
                }
                None => None,
            };
            eprintln!(
                "Continuing: {} of {} server(s)/DM(s) left, {} message(s) deleted so far.",
                saved.remaining(),
                saved.targets.len(),
                saved.checkpoint.stats.deleted
            );
            graceful.store(true, Ordering::SeqCst);
            delete(Plan {
                client,
                me,
                package,
                targets: saved.targets.clone(),
                filter: saved.filter.clone(),
                options,
                control,
                verbose,
                export: None,
                export_passphrase: None,
                state: Some((file, saved)),
            })
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
    if package.owner.is_none() {
        eprintln!("warning: the package has no account/user.json, so it cannot be checked that it is yours");
    }
    Ok(package)
}

/// A CSV or JSON record of the run, encrypted if a passphrase is given.
enum Exporter {
    Plain(PathBuf, ExportWriter<io::BufWriter<std::fs::File>>),
    Encrypted(
        PathBuf,
        ExportWriter<vault::StreamWriter<io::BufWriter<std::fs::File>>>,
    ),
}

impl Exporter {
    fn create(path: &Path, passphrase: Option<&SecretString>) -> Result<Self> {
        let format = ExportFormat::for_path(path);
        Ok(match passphrase {
            None => {
                let file = std::fs::File::create(path)
                    .with_context(|| format!("cannot create {}", path.display()))?;
                Exporter::Plain(
                    path.to_owned(),
                    ExportWriter::new(io::BufWriter::new(file), format)?,
                )
            }
            Some(passphrase) => {
                let mut name = path.as_os_str().to_owned();
                if path.extension().is_none_or(|e| e != "age") {
                    name.push(".age");
                }
                let path = PathBuf::from(name);
                let file = std::fs::File::create(&path)
                    .with_context(|| format!("cannot create {}", path.display()))?;
                let sealer = vault::encrypt(passphrase, io::BufWriter::new(file))?;
                Exporter::Encrypted(path, ExportWriter::new(sealer, format)?)
            }
        })
    }

    fn observe(&mut self, event: &Event) -> Result<()> {
        let (path, result) = match self {
            Exporter::Plain(path, w) => (path, w.observe(event)),
            Exporter::Encrypted(path, w) => (path, w.observe(event)),
        };
        result.with_context(|| format!("could not write {}", path.display()))
    }

    fn finish(self) -> Result<()> {
        let (path, rows, result) = match self {
            Exporter::Plain(path, w) => {
                let rows = w.rows();
                (path, rows, w.finish().map(drop))
            }
            Exporter::Encrypted(path, w) => {
                let rows = w.rows();
                let result = w.finish().and_then(|sealer| sealer.finish()).map(drop);
                (path, rows, result)
            }
        };
        result.with_context(|| format!("could not write {}", path.display()))?;
        println!("Saved {rows} message(s) to {}.", path.display());
        Ok(())
    }
}

/// The passphrase from `file`, or asked for. A new one may be left empty
/// to get a generated one, which must then be confirmed.
fn ask_passphrase(file: Option<&Path>, new: bool) -> Result<SecretString> {
    if let Some(file) = file {
        return vault::read_passphrase_file(file)
            .with_context(|| format!("cannot read the passphrase from {}", file.display()));
    }
    if !io::stdin().is_terminal() {
        bail!("an encrypted backup or export needs a passphrase: pass --passphrase-file FILE, or --no-encrypt");
    }
    if !new {
        return Ok(vault::secret(&rpassword::prompt_password(
            "Passphrase (input hidden): ",
        )?));
    }
    let typed = rpassword::prompt_password(
        "Passphrase for the encrypted backup and export (input hidden; press Enter to get one): ",
    )?;
    if !typed.is_empty() {
        let again = rpassword::prompt_password("The same passphrase again: ")?;
        if again != typed {
            bail!("the passphrases differ");
        }
        if typed.chars().count() < 12 {
            eprintln!("warning: a passphrase this short can be guessed; a sentence or several words are safer");
        }
        return Ok(vault::secret(&typed));
    }
    let generated = vault::generate_passphrase();
    let words: Vec<&str> = generated.split(' ').collect();
    println!("\nYour passphrase (write it down; without it the backup cannot be opened):\n");
    println!("    {generated}\n");
    for position in [3, 7, 11] {
        loop {
            print!("Type word {position} to confirm: ");
            io::stdout().flush()?;
            let mut answer = String::new();
            if io::stdin().read_line(&mut answer)? == 0 {
                bail!("not confirmed");
            }
            if answer.trim().eq_ignore_ascii_case(words[position - 1]) {
                break;
            }
            println!("That is not word {position}.");
        }
    }
    Ok(vault::secret(&generated))
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
        (Some(count), _) => println!("{count:>8}  {}", safe(&name)),
        (None, error) => println!(
            "{:>8}  {} (could not search: {})",
            "?",
            safe(&name),
            safe(error.as_deref().unwrap_or("unknown error"))
        ),
    }
}

/// Text from Discord made safe to print: control characters (such as the
/// escape sequences a message can carry, which could rewrite the screen, set
/// the window title or fill the clipboard) and characters that reverse or
/// hide text are shown as `�`.
fn safe(text: &str) -> std::borrow::Cow<'_, str> {
    let unsafe_char = |c: char| c.is_control() || erasecord_core::backup::is_invisible(c);
    if text.contains(unsafe_char) {
        text.chars()
            .map(|c| if unsafe_char(c) { '\u{FFFD}' } else { c })
            .collect::<String>()
            .into()
    } else {
        text.into()
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
            item.target.id,
            item.messages,
            safe(&item.target.name)
        );
        if item.target.kind == TargetKind::Guild && item.channels.len() > 1 {
            for channel in &item.channels {
                println!(
                    "{:<9} {:<20} {:>9}  #{}",
                    "  channel",
                    channel.id,
                    channel.messages,
                    safe(&channel.name)
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
/// A clean-up to run.
struct Plan {
    client: Client,
    me: Snowflake,
    package: Option<Arc<Package>>,
    targets: Vec<Target>,
    filter: Filter,
    options: JobOptions,
    control: JobControl,
    verbose: bool,
    export: Option<PathBuf>,
    /// Encrypts the export, if set.
    export_passphrase: Option<SecretString>,
    /// Where to keep track of the progress, and what is known so far.
    state: Option<(PathBuf, SavedRun)>,
}

/// How often the state file is written, in events.
const STATE_SAVE_EVERY: u32 = 20;

async fn delete(plan: Plan) -> Result<ExitCode> {
    let Plan {
        client,
        me,
        package,
        targets,
        filter,
        options,
        control,
        verbose,
        export,
        export_passphrase,
        mut state,
    } = plan;
    let export = export.as_deref();
    if let Some((file, saved)) = &state {
        saved
            .save(file)
            .with_context(|| format!("cannot write the state file {}", file.display()))?;
    }
    // Created before anything is deleted, so a bad path costs nothing.
    // An encrypted backup keeps its own list, inside the archive.
    let plain_backup = options
        .backup_dir
        .as_ref()
        .filter(|_| options.backup_encryption.is_none());
    let backup_list = match plain_backup {
        Some(dir) => {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
            let stamp = Local::now().format("%Y%m%d-%H%M%S");
            Some(dir.join(format!("messages-{stamp}.json")))
        }
        None => None,
    };
    let mut exporters = Vec::new();
    if let Some(path) = export {
        exporters.push(Exporter::create(path, export_passphrase.as_ref())?);
    }
    if let Some(path) = &backup_list {
        exporters.push(Exporter::create(path, None)?);
    }
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
    let mut unsaved = 0;
    while let Some(event) = rx.recv().await {
        progress.handle(&event);
        if let Some((file, saved)) = &mut state {
            saved.checkpoint.observe(&event);
            unsaved += 1;
            if unsaved >= STATE_SAVE_EVERY {
                unsaved = 0;
                saved
                    .save(file)
                    .with_context(|| format!("cannot write the state file {}", file.display()))?;
            }
        }
        for exporter in &mut exporters {
            exporter.observe(&event)?;
        }
    }
    let summary = task.await?;
    for exporter in exporters {
        exporter.finish()?;
    }

    println!(
        "Done: {} {}, {} skipped, {} failed.",
        summary.stats.deleted, progress.deleted_label, summary.stats.skipped, summary.stats.failed
    );
    if let Some((file, saved)) = &state {
        // A server or DM whose search failed is not done yet.
        let complete = summary.error.is_none() && !summary.cancelled && saved.remaining() == 0;
        if complete {
            let _ = std::fs::remove_file(file);
            println!("Everything is done; removed {}.", file.display());
        } else {
            saved
                .save(file)
                .with_context(|| format!("cannot write the state file {}", file.display()))?;
            println!("To continue later: erasecord resume {}", file.display());
        }
    }
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
            Event::SavedFromOthers {
                sent_at,
                author,
                saved,
                ..
            } => {
                self.stats.saved_from_others += 1;
                if self.verbose {
                    let sent = sent_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
                    self.line(format!(
                        "  saved {} file(s) from {author}, {sent}",
                        saved.len()
                    ));
                }
            }
            Event::NotSavedFromOthers {
                message_id, error, ..
            } => self.line(format!(
                "  could not save the files of {message_id}: {error}"
            )),
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
            Event::TargetFinished { stats, .. } => {
                let others = if stats.saved_from_others > 0 {
                    format!(
                        "; files of {} message(s) of others saved",
                        stats.saved_from_others
                    )
                } else {
                    String::new()
                };
                self.line(format!(
                    "  {} {}, {} skipped, {} failed{others}",
                    stats.deleted, self.deleted_label, stats.skipped, stats.failed
                ))
            }
            Event::Notice { notice } => self.line(format!("  {}", describe_notice(notice))),
            Event::Activity { activity } => match activity {
                Activity::Break { ms } => self.line(format!(
                    "  taking a break of {} s, to go easy on Discord",
                    ms.div_ceil(1000)
                )),
                Activity::SealingBackup => self.line("  finishing the backup…".to_owned()),
                _ => {}
            },
            Event::BackupSealed {
                archive,
                files,
                messages,
            } => self.line(format!(
                "Backup: {} file(s) and {messages} message(s) in {} (encrypted).",
                files,
                archive.display()
            )),
            Event::BackupKept { folder, reason } => self.line(format!(
                "The backup stays in {} for now: {reason}.",
                folder.display()
            )),
            Event::Finished(_) => {
                self.clear_status();
                return;
            }
        }
        self.status();
    }

    fn line(&self, text: String) {
        self.clear_status();
        eprintln!("{}", safe(&text));
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
        println!("{kind:<9} {:<20} {}", target.id, safe(&target.name));
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

fn bar(value: u64, max: u64, width: u64) -> String {
    "#".repeat((value * width).div_ceil(max.max(1)) as usize)
}

fn print_overview(
    o: &erasecord_core::insights::Overview,
    places: &[erasecord_core::insights::Place],
) {
    let day = |t: Option<&erasecord_core::insights::MessageRef>| {
        t.map(|m| {
            m.sent_at
                .with_timezone(&Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_default()
    };
    println!(
        "Messages      {} on {} of {} days, {} to {}",
        o.messages,
        o.active_days,
        o.span_days,
        day(o.first.as_ref()),
        day(o.last.as_ref())
    );
    println!("Places        {}", o.places);
    println!(
        "Words         {} ({} messages without text)",
        o.words, o.without_text
    );
    println!(
        "Attachments   {} in {} messages",
        o.attachments, o.with_attachments
    );
    println!("Links         {}", o.links);
    if let Some(busiest) = &o.busiest_day {
        println!(
            "Busiest day   {} ({} messages)",
            busiest.date, busiest.messages
        );
    }
    if let Some(streak) = &o.longest_streak {
        println!(
            "Longest streak {} days, {} to {}",
            streak.days, streak.from, streak.to
        );
    }
    if let Some(pause) = &o.longest_break {
        println!(
            "Longest break {} days, {} to {}",
            pause.days, pause.from, pause.to
        );
    }
    if !o.years.is_empty() {
        println!("\nPer year");
        let max = o.years.iter().map(|y| y.messages).max().unwrap_or(1);
        for y in &o.years {
            let top = y.top_place.map_or("", |p| places[p].name.as_str());
            println!(
                "  {}  {:>8}  {:<40}  most in {top}",
                y.year,
                y.messages,
                bar(y.messages, max, 40)
            );
        }
    }
}

fn print_timeline(t: &erasecord_core::insights::Timeline) {
    let hours: Vec<u64> = (0..24)
        .map(|h| t.week_hours.iter().map(|d| d[h]).sum())
        .collect();
    let max = hours.iter().copied().max().unwrap_or(1);
    println!("By hour (local time)");
    for (hour, n) in hours.iter().enumerate() {
        println!("  {hour:02}:00  {n:>8}  {}", bar(*n, max, 40));
    }
    let days: Vec<u64> = t.week_hours.iter().map(|d| d.iter().sum()).collect();
    let max = days.iter().copied().max().unwrap_or(1);
    println!("\nBy weekday");
    for (name, n) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
        .iter()
        .zip(&days)
    {
        println!("  {name}    {n:>8}  {}", bar(*n, max, 40));
    }
    let max = t.months.iter().map(|m| m.total).max().unwrap_or(1);
    println!("\nBy month");
    for m in &t.months {
        println!("  {}  {:>8}  {}", m.key, m.total, bar(m.total, max, 40));
    }
}

fn print_places(
    r: &erasecord_core::insights::PlacesReport,
    places: &[erasecord_core::insights::Place],
) {
    for row in r.places.iter().take(30) {
        println!("  {:>8}  {}", row.messages, safe(&places[row.place].name));
    }
    if r.places.len() > 30 {
        println!("  … and {} more", r.places.len() - 30);
    }
    if !r.channels.is_empty() {
        println!("\nChannels");
        for c in r.channels.iter().take(20) {
            println!(
                "  {:>8}  #{} in {}",
                c.messages,
                safe(&c.name),
                safe(&places[c.place].name)
            );
        }
    }
}

fn print_words(r: &erasecord_core::insights::WordsReport) {
    println!("{} words, {} different", r.total_words, r.distinct_words);
    for w in r.words.iter().take(30) {
        println!("  {:>8}  {}", w.count, safe(&w.key));
    }
    if !r.emoji.is_empty() {
        let emoji: Vec<String> = r
            .emoji
            .iter()
            .take(15)
            .map(|e| match e.id {
                Some(_) => format!(":{}: {}", safe(&e.emoji), e.count),
                None => format!("{} {}", safe(&e.emoji), e.count),
            })
            .collect();
        println!("\nEmoji  {}", emoji.join("  "));
    }
}

fn print_links(r: &erasecord_core::insights::LinksReport) {
    println!("{} links in {} messages", r.links, r.messages_with_links);
    for d in r.domains.iter().take(20) {
        println!("  {:>8}  {}", d.count, safe(&d.key));
    }
    println!("\n{} attachments", r.attachments);
    for k in r.kinds.iter().filter(|k| k.count > 0) {
        println!("  {:>8}  {}", k.count, k.key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_from_discord_cannot_control_the_terminal() {
        // Clears the screen, sets the window title, writes to the clipboard,
        // and turns "exe.jpg" around.
        let hostile = "\x1b[2J\x1b]0;hi\x07\x1b]52;c;cm0gLXJm\x07\u{9b}31m ok \u{202e}gpj.exe";
        let shown = safe(hostile);
        assert!(
            !shown.chars().any(|c| c.is_control() || c == '\u{202e}'),
            "{shown:?}"
        );
        assert!(shown.contains(" ok "));
        assert_eq!(safe("plain name"), "plain name");
    }

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
