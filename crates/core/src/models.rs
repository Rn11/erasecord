//! The subset of Discord's API objects that EraseCord reads.

use serde::{Deserialize, Serialize};

use crate::snowflake::Snowflake;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: Snowflake,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
}

impl User {
    pub fn display_name(&self) -> &str {
        self.global_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.username)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guild {
    pub id: Snowflake,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
}

/// Values of [`Channel::kind`] that EraseCord cares about.
pub mod channel_type {
    pub const TEXT: u8 = 0;
    pub const DM: u8 = 1;
    pub const VOICE: u8 = 2;
    pub const GROUP_DM: u8 = 3;
    pub const CATEGORY: u8 = 4;
    pub const ANNOUNCEMENT: u8 = 5;
    pub const STAGE: u8 = 13;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    pub id: Snowflake,
    #[serde(rename = "type")]
    pub kind: u8,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub recipients: Vec<User>,
    #[serde(default)]
    pub last_message_id: Option<Snowflake>,
    #[serde(default)]
    pub guild_id: Option<Snowflake>,
    /// The category a server channel is in.
    #[serde(default)]
    pub parent_id: Option<Snowflake>,
    #[serde(default)]
    pub position: Option<i64>,
}

/// A thread or forum post: a channel of its own inside a server channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: Snowflake,
    /// The channel the thread belongs to.
    #[serde(default)]
    pub parent_id: Option<Snowflake>,
    #[serde(default)]
    pub thread_metadata: Option<ThreadMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadMetadata {
    /// When the thread was last archived or unarchived (ISO 8601).
    #[serde(default)]
    pub archive_timestamp: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadList {
    #[serde(default)]
    pub threads: Vec<Thread>,
    #[serde(default)]
    pub has_more: bool,
}

/// An entry of the friend list (or a block, or a pending request).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Relationship {
    /// The other user's ID.
    pub id: Snowflake,
    /// 1 = friend, 2 = blocked, 3/4 = incoming/outgoing request.
    #[serde(rename = "type")]
    pub kind: u8,
    pub user: User,
}

impl Relationship {
    pub const FRIEND: u8 = 1;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: Snowflake,
    pub channel_id: Snowflake,
    #[serde(rename = "type")]
    pub kind: u8,
    #[serde(default)]
    pub content: String,
    pub author: User,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub attachments: Vec<serde_json::Value>,
    #[serde(default)]
    pub embeds: Vec<serde_json::Value>,
    #[serde(default)]
    pub sticker_items: Vec<serde_json::Value>,
    /// Set on search results: marks the message that matched, as opposed to
    /// context messages around it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    #[serde(default)]
    pub total_results: u64,
    /// One group per match; see [`SearchResponse::into_hits`].
    #[serde(default)]
    pub messages: Vec<Vec<Message>>,
}
