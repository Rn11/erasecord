//! The servers and DMs a user can clean up.

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::error::Result;
use crate::models::{channel_type, Channel, Guild, User};
use crate::search::Scope;
use crate::snowflake::Snowflake;

const CDN: &str = "https://cdn.discordapp.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Guild,
    Dm,
    GroupDm,
}

/// A server or DM whose messages can be deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub kind: TargetKind,
    /// Server ID, or channel ID for DMs.
    pub id: Snowflake,
    pub name: String,
    #[serde(default)]
    pub icon_url: Option<String>,
}

impl Target {
    pub fn scope(&self) -> Scope {
        match self.kind {
            TargetKind::Guild => Scope::Guild(self.id),
            TargetKind::Dm | TargetKind::GroupDm => Scope::Channel(self.id),
        }
    }

    pub fn from_guild(guild: Guild) -> Self {
        let icon_url = guild
            .icon
            .map(|hash| format!("{CDN}/icons/{}/{hash}.png?size=64", guild.id));
        Target {
            kind: TargetKind::Guild,
            id: guild.id,
            name: guild.name,
            icon_url,
        }
    }

    /// `None` for channels that are not DMs.
    pub fn from_channel(channel: Channel) -> Option<Self> {
        let kind = match channel.kind {
            channel_type::DM => TargetKind::Dm,
            channel_type::GROUP_DM => TargetKind::GroupDm,
            _ => return None,
        };
        let name = match channel.name.filter(|name| !name.is_empty()) {
            Some(name) => name,
            None if channel.recipients.is_empty() => "Unnamed conversation".to_owned(),
            None => channel
                .recipients
                .iter()
                .map(User::display_name)
                .collect::<Vec<_>>()
                .join(", "),
        };
        let icon_url = match kind {
            TargetKind::Dm => channel.recipients.first().and_then(|user| {
                let hash = user.avatar.as_ref()?;
                Some(format!("{CDN}/avatars/{}/{hash}.png?size=64", user.id))
            }),
            _ => channel
                .icon
                .map(|hash| format!("{CDN}/channel-icons/{}/{hash}.png?size=64", channel.id)),
        };
        Some(Target {
            kind,
            id: channel.id,
            name,
            icon_url,
        })
    }
}

/// All servers (sorted by name), then all open DMs (most recent first).
pub async fn list_targets(client: &Client) -> Result<Vec<Target>> {
    let mut targets: Vec<Target> = client
        .guilds()
        .await?
        .into_iter()
        .map(Target::from_guild)
        .collect();
    targets.sort_by_key(|t| t.name.to_lowercase());

    let mut channels = client.private_channels().await?;
    channels.sort_by_key(|c| std::cmp::Reverse(c.last_message_id));
    targets.extend(channels.into_iter().filter_map(Target::from_channel));
    Ok(targets)
}
