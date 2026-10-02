//! The servers and DMs a user can clean up.

use serde::{Deserialize, Serialize};

use chrono::{DateTime, Utc};

use crate::client::Client;
use crate::error::{Error, Result};
use crate::job::JobControl;
use crate::models::{channel_type, Channel, Guild, Relationship, Thread, User};
use crate::pace::Pace;
use crate::search::{Scope, SearchQuery};
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
    /// For servers: only clean up these channels. Empty means all of them.
    #[serde(default)]
    pub channels: Vec<Snowflake>,
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
            channels: Vec::new(),
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
            TargetKind::Dm => channel.recipients.first().and_then(avatar_url),
            _ => channel
                .icon
                .map(|hash| format!("{CDN}/channel-icons/{}/{hash}.png?size=64", channel.id)),
        };
        Some(Target {
            kind,
            id: channel.id,
            name,
            icon_url,
            channels: Vec::new(),
        })
    }

    /// Whether a message in `channel_id` belongs to what this target covers.
    pub fn covers_channel(&self, channel_id: Snowflake) -> bool {
        self.channels.is_empty() || self.channels.contains(&channel_id)
    }
}

/// A friend whose DM is closed, so it does not show up in the DM list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Friend {
    pub user_id: Snowflake,
    pub name: String,
    #[serde(default)]
    pub icon_url: Option<String>,
}

fn avatar_url(user: &User) -> Option<String> {
    let hash = user.avatar.as_ref()?;
    Some(format!("{CDN}/avatars/{}/{hash}.png?size=64", user.id))
}

/// Friends without an open DM, sorted by name. Their conversations can be
/// reached with [`open_dm`].
pub async fn friends_without_dm(client: &Client) -> Result<Vec<Friend>> {
    let relationships = client.relationships().await?;
    let open: Vec<Snowflake> = client
        .private_channels()
        .await?
        .into_iter()
        .filter(|c| c.kind == channel_type::DM)
        .flat_map(|c| c.recipients.into_iter().map(|u| u.id))
        .collect();
    let mut friends: Vec<Friend> = relationships
        .into_iter()
        .filter(|r| r.kind == Relationship::FRIEND && !open.contains(&r.id))
        .map(|r| Friend {
            user_id: r.id,
            name: r.user.display_name().to_owned(),
            icon_url: avatar_url(&r.user),
        })
        .collect();
    friends.sort_by_key(|f| f.name.to_lowercase());
    Ok(friends)
}

/// Opens the DM with a user so it can be cleaned up like any open DM.
pub async fn open_dm(client: &Client, user_id: Snowflake) -> Result<Target> {
    let channel = client.open_dm(user_id).await?;
    Target::from_channel(channel).ok_or_else(|| Error::Api {
        status: 200,
        code: None,
        message: "Discord did not return a DM channel".into(),
    })
}

/// A channel of a server that can hold messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildChannel {
    pub id: Snowflake,
    pub name: String,
    /// Discord's channel type (text, voice, announcement, stage).
    pub kind: u8,
    /// Name of the category the channel is in.
    pub category: Option<String>,
}

/// The channels of a server that can hold messages, in the order Discord
/// shows them: uncategorised first, then category by category.
///
/// Threads and forum posts are separate channels and are not listed; they
/// are added to a clean-up of their channel by [`with_threads`].
pub async fn list_channels(client: &Client, guild_id: Snowflake) -> Result<Vec<GuildChannel>> {
    Ok(sort_channels(client.guild_channels(guild_id).await?))
}

fn sort_channels(channels: Vec<Channel>) -> Vec<GuildChannel> {
    use channel_type::*;
    let categories: Vec<&Channel> = channels.iter().filter(|c| c.kind == CATEGORY).collect();
    let category = |id: Option<Snowflake>| {
        let found = categories.iter().find(|c| Some(c.id) == id)?;
        Some((
            found.position.unwrap_or(0),
            found.id,
            found.name.clone().unwrap_or_default(),
        ))
    };
    let mut listed: Vec<_> = channels
        .iter()
        .filter(|c| matches!(c.kind, TEXT | VOICE | ANNOUNCEMENT | STAGE))
        .map(|c| {
            let parent = category(c.parent_id);
            // Voice and stage channels come after text channels in Discord.
            let voice = matches!(c.kind, VOICE | STAGE);
            let key = (
                parent.as_ref().map(|(position, id, _)| (*position, *id)),
                voice,
                c.position.unwrap_or(0),
                c.id,
            );
            let item = GuildChannel {
                id: c.id,
                name: c.name.clone().unwrap_or_else(|| c.id.to_string()),
                kind: c.kind,
                category: parent.map(|(_, _, name)| name),
            };
            (key, item)
        })
        .collect();
    listed.sort_by_key(|a| a.0);
    listed.into_iter().map(|(_, item)| item).collect()
}

/// Discord's search takes a limited number of channels at once, so a server
/// is searched in at most this many (picked channels and their threads).
const MAX_SEARCH_CHANNELS: usize = 100;

/// `targets` with the threads and forum posts of their picked channels
/// added, because Discord's search treats every thread as a channel of its
/// own. Servers searched as a whole already include them.
///
/// Only threads that can hold matching messages are added: none created
/// after the time range, none archived before it (the archived lists are
/// read newest first, so paging stops there). The newest threads come first
/// when there are more than [`MAX_SEARCH_CHANNELS`]. A list that cannot be
/// read (no access, network error) is skipped; its threads stay untouched.
pub(crate) async fn with_threads(
    client: &Client,
    targets: &[Target],
    query: &SearchQuery,
    pace: &Pace,
    control: &JobControl,
) -> Vec<Target> {
    let mut first = true;
    let mut next = || {
        let wait = if first {
            None
        } else {
            Some(pace.before_download())
        };
        first = false;
        wait
    };
    let mut expanded = targets.to_vec();
    for target in &mut expanded {
        if target.kind != TargetKind::Guild || target.channels.is_empty() {
            continue;
        }
        let mut threads = Vec::new();
        if let Some(Ok(active)) = request(control, next(), client.active_threads(target.id)).await {
            threads.extend(
                active
                    .into_iter()
                    .filter(|t| t.parent_id.is_some_and(|p| target.channels.contains(&p))),
            );
        }
        for &channel in target.channels.clone().iter() {
            for private in [false, true] {
                let mut before: Option<String> = None;
                loop {
                    let page = client.archived_threads(channel, private, before.as_deref());
                    let Some(Ok(page)) = request(control, next(), page).await else {
                        break;
                    };
                    let mut past_range = false;
                    for thread in page.threads {
                        let archived = archived_at(&thread);
                        if archived
                            .is_some_and(|at| query.min_id.is_some_and(|min| at < min.created_at()))
                        {
                            past_range = true;
                            break;
                        }
                        before = thread
                            .thread_metadata
                            .as_ref()
                            .and_then(|m| m.archive_timestamp.clone());
                        threads.push(thread);
                    }
                    if past_range || !page.has_more || before.is_none() {
                        break;
                    }
                }
            }
        }
        // A thread's messages are all newer than the thread itself.
        threads.retain(|t| query.max_id.is_none_or(|max| t.id < max));
        threads.sort_by_key(|t| std::cmp::Reverse(t.id));
        for thread in threads {
            if target.channels.len() >= MAX_SEARCH_CHANNELS {
                break;
            }
            if !target.channels.contains(&thread.id) {
                target.channels.push(thread.id);
            }
        }
    }
    expanded
}

/// Runs `future` after `wait`; `None` if the job was stopped meanwhile.
async fn request<T>(
    control: &JobControl,
    wait: Option<std::time::Duration>,
    future: impl std::future::Future<Output = T>,
) -> Option<T> {
    if let Some(wait) = wait {
        control.sleep_for(wait).await.ok()?;
    }
    control.checkpoint().await.ok()?;
    control.guard(future).await.ok()
}

fn archived_at(thread: &Thread) -> Option<DateTime<Utc>> {
    let at = thread
        .thread_metadata
        .as_ref()?
        .archive_timestamp
        .as_deref()?;
    DateTime::parse_from_rfc3339(at)
        .ok()
        .map(|at| at.with_timezone(&Utc))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(id: u64, kind: u8, name: &str, parent: Option<u64>, position: i64) -> Channel {
        Channel {
            id: Snowflake(id),
            kind,
            name: Some(name.into()),
            icon: None,
            recipients: vec![],
            last_message_id: None,
            guild_id: Some(Snowflake(1)),
            parent_id: parent.map(Snowflake),
            position: Some(position),
        }
    }

    #[test]
    fn channels_are_sorted_like_discord_shows_them() {
        use channel_type::*;
        let sorted = sort_channels(vec![
            channel(10, CATEGORY, "Second", None, 1),
            channel(11, CATEGORY, "First", None, 0),
            channel(20, TEXT, "later", Some(10), 0),
            channel(21, VOICE, "voice", Some(11), 0),
            channel(22, TEXT, "general", Some(11), 5),
            channel(23, TEXT, "rules", None, 3),
            channel(24, 15, "forum", Some(11), 1),
        ]);
        let names: Vec<(&str, Option<&str>)> = sorted
            .iter()
            .map(|c| (c.name.as_str(), c.category.as_deref()))
            .collect();
        assert_eq!(
            names,
            [
                ("rules", None),
                ("general", Some("First")),
                ("voice", Some("First")),
                ("later", Some("Second")),
            ]
        );
    }
}
