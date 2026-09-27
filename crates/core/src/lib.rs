//! Core of EraseCord: finds a user's own Discord messages through the search
//! API and deletes them, within a chosen time range and in chosen servers and
//! DMs.
//!
//! Everything here is independent of the user interface; the CLI and the
//! desktop app are thin layers on top.

pub mod anonymize;
pub mod backup;
pub mod client;
pub mod delete;
pub mod error;
pub mod export;
pub mod filter;
pub mod insights;
pub mod inspect;
pub mod job;
pub mod models;
pub mod package;
mod ratelimit;
pub mod resume;
pub mod search;
pub mod snowflake;
mod stopwords;
pub mod targets;

pub use client::{Client, ClientConfig, Notice, NoticeSink};
pub use delete::SkipReason;
pub use error::{Error, Result};
pub use export::{ExportFormat, ExportWriter};
pub use filter::{Filter, Has};
pub use job::{Event, JobControl, JobOptions, PreviewEntry, Stats, Summary};
pub use models::{Message, User};
pub use package::{Package, PackageTarget};
pub use resume::{Checkpoint, SavedRun};
pub use snowflake::Snowflake;
pub use targets::{
    friends_without_dm, list_channels, list_targets, open_dm, Friend, GuildChannel, Target,
    TargetKind,
};
