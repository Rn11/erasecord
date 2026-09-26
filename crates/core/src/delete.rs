//! Deciding what can be deleted and interpreting Discord's answers.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Why a message was left alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    Pinned,
    /// Join notices, call logs and similar messages cannot be deleted.
    SystemMessage,
    NoPermission,
    /// Threads that are archived (and locked) reject deletions.
    ArchivedThread,
}

impl SkipReason {
    pub fn describe(self) -> &'static str {
        match self {
            SkipReason::Pinned => "pinned",
            SkipReason::SystemMessage => "system message",
            SkipReason::NoPermission => "no permission",
            SkipReason::ArchivedThread => "archived thread",
        }
    }
}

/// Message types a user can delete: normal messages and replies.
pub fn is_deletable_kind(kind: u8) -> bool {
    matches!(kind, 0 | 19)
}

pub(crate) enum Outcome {
    Deleted,
    /// Deleted before we got to it; counts as deleted.
    AlreadyGone,
    Skipped(SkipReason),
    Failed(Error),
}

/// Sorts the result of a delete request. Errors that should end the whole job
/// (bad token, cancellation) are passed through as `Err`.
pub(crate) fn classify(result: Result<()>) -> Result<Outcome> {
    match result {
        Ok(()) => Ok(Outcome::Deleted),
        Err(err @ (Error::Unauthorized | Error::Cancelled)) => Err(err),
        Err(Error::Api { status: 404, .. }) => Ok(Outcome::AlreadyGone),
        Err(Error::Api {
            code: Some(50083), ..
        }) => Ok(Outcome::Skipped(SkipReason::ArchivedThread)),
        Err(Error::Api {
            code: Some(50021), ..
        }) => Ok(Outcome::Skipped(SkipReason::SystemMessage)),
        Err(
            Error::Api { status: 403, .. }
            | Error::Api {
                code: Some(50001 | 50013),
                ..
            },
        ) => Ok(Outcome::Skipped(SkipReason::NoPermission)),
        Err(err) => Ok(Outcome::Failed(err)),
    }
}
