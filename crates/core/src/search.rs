//! Discord's message search: the only way to find a user's own messages
//! without reading every channel.

use serde::{Deserialize, Serialize};

use crate::filter::Has;
use crate::models::{Message, SearchResponse};
use crate::snowflake::Snowflake;

/// Where to search: a whole server, or a single (DM) channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Scope {
    Guild(Snowflake),
    Channel(Snowflake),
}

impl Scope {
    pub(crate) fn search_path(self) -> String {
        match self {
            Scope::Guild(id) => format!("/guilds/{id}/messages/search"),
            Scope::Channel(id) => format!("/channels/{id}/messages/search"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub author_id: Option<Snowflake>,
    /// Only messages with a larger ID, i.e. sent later.
    pub min_id: Option<Snowflake>,
    /// Only messages with a smaller ID, i.e. sent earlier.
    pub max_id: Option<Snowflake>,
    /// Words the text must contain.
    pub content: Option<String>,
    /// Only messages with any of these.
    pub has: Vec<Has>,
}

impl SearchQuery {
    pub(crate) fn params(&self) -> Vec<(&'static str, String)> {
        let mut params = vec![
            ("sort_by", "timestamp".to_owned()),
            ("sort_order", "desc".to_owned()),
            ("include_nsfw", "true".to_owned()),
        ];
        let ids = [
            ("author_id", self.author_id),
            ("min_id", self.min_id),
            ("max_id", self.max_id),
        ];
        for (name, id) in ids {
            if let Some(id) = id {
                params.push((name, id.to_string()));
            }
        }
        if let Some(content) = &self.content {
            params.push(("content", content.clone()));
        }
        for has in &self.has {
            params.push(("has", has.as_str().to_owned()));
        }
        params
    }
}

impl SearchResponse {
    /// The matching messages, newest first.
    ///
    /// Older API versions return every hit surrounded by context messages;
    /// only the hit itself is kept.
    pub fn into_hits(self) -> Vec<Message> {
        let mut hits: Vec<Message> = self
            .messages
            .into_iter()
            .filter_map(|group| {
                let single = group.len() == 1;
                group.into_iter().find(|m| single || m.hit == Some(true))
            })
            .collect();
        hits.sort_by_key(|m| std::cmp::Reverse(m.id));
        hits.dedup_by_key(|m| m.id);
        hits
    }
}
