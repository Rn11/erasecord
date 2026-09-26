//! Core of purgecord: finds a user's own Discord messages through the search
//! API and deletes them, within a chosen time range and in chosen servers and
//! DMs.
//!
//! Everything here is independent of the user interface; the CLI and the
//! desktop app are thin layers on top.

pub mod client;
pub mod delete;
pub mod error;
pub mod filter;
pub mod job;
pub mod models;
mod ratelimit;
pub mod search;
pub mod snowflake;
pub mod targets;

pub use client::{Client, ClientConfig, Notice, NoticeSink};
pub use delete::SkipReason;
pub use error::{Error, Result};
pub use filter::{Filter, Has};
pub use job::{Event, JobControl, JobOptions, PreviewEntry, Stats, Summary};
pub use models::{Message, User};
pub use snowflake::Snowflake;
pub use targets::{
    friends_without_dm, list_channels, list_targets, open_dm, Friend, GuildChannel, Target,
    TargetKind,
};
