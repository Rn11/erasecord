//! A small in-memory Discord that answers search and delete requests the way
//! the real API does, including its quirks (lagging index, errors).

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use erasecord_core::{Client, ClientConfig, Snowflake, Target, TargetKind};
use serde_json::{json, Value};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

pub const ME: u64 = 1000;
pub const OTHER: u64 = 2000;
pub const GUILD: u64 = 10;
pub const GUILD_CHANNEL: u64 = 11;
pub const GUILD_CHANNEL_2: u64 = 12;
pub const DM_CHANNEL: u64 = 20;
pub const TOKEN: &str = "test-token";

#[derive(Clone, Debug)]
pub struct FakeMessage {
    pub id: u64,
    pub channel_id: u64,
    pub guild_id: Option<u64>,
    pub author_id: u64,
    pub kind: u8,
    pub pinned: bool,
    /// Text; `None` means "message <id>".
    pub content: Option<String>,
    /// File names of attachments.
    pub files: Vec<String>,
    /// Only shows up in search results after this many searches.
    pub hidden_for_searches: usize,
}

impl FakeMessage {
    pub fn in_guild(minutes: i64, seq: u64, author_id: u64) -> Self {
        FakeMessage {
            id: id_at(minutes, seq),
            channel_id: GUILD_CHANNEL,
            guild_id: Some(GUILD),
            author_id,
            kind: 0,
            pinned: false,
            content: None,
            files: Vec::new(),
            hidden_for_searches: 0,
        }
    }

    pub fn text(&self) -> String {
        self.content
            .clone()
            .unwrap_or_else(|| format!("message {}", self.id))
    }

    pub fn with_text(self, content: &str) -> Self {
        FakeMessage {
            content: Some(content.into()),
            ..self
        }
    }

    pub fn in_dm(minutes: i64, seq: u64) -> Self {
        FakeMessage {
            channel_id: DM_CHANNEL,
            guild_id: None,
            ..Self::in_guild(minutes, seq, ME)
        }
    }
}

#[derive(Default)]
pub struct State {
    pub messages: Vec<FakeMessage>,
    /// Deleted messages keep appearing in search results.
    pub stale_index: bool,
    stale: Vec<FakeMessage>,
    /// Message ID -> (HTTP status, Discord error code) returned on delete.
    pub delete_errors: HashMap<u64, (u16, u64)>,
    /// Search ignores `channel_id`, like a misbehaving index.
    pub ignore_channel_filter: bool,
    /// Channels that answer 404 Unknown Channel.
    pub gone_channels: Vec<u64>,
    /// Servers whose search answers 403 Missing Access.
    pub forbidden_guilds: Vec<u64>,
    pub deleted: Vec<u64>,
    /// Every edit: message ID and the JSON body.
    pub edits: Vec<(u64, Value)>,
    /// Requests in the order they arrived, e.g. "PATCH 12" or "DELETE 12".
    pub log: Vec<String>,
    /// How long a delete takes to answer.
    pub delete_delay: Duration,
    pub search_calls: usize,
    pub delete_calls: usize,
}

impl State {
    pub fn with_messages(messages: Vec<FakeMessage>) -> Self {
        State {
            messages,
            ..Default::default()
        }
    }
}

pub struct FakeDiscord {
    pub server: MockServer,
    pub state: Arc<Mutex<State>>,
}

impl FakeDiscord {
    pub async fn start(state: State) -> Self {
        let server = MockServer::start().await;
        let state = Arc::new(Mutex::new(state));
        Mock::given(method("GET"))
            .and(path("/api/v9/users/@me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(user_json(ME, "me")))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(
                r"^/api/v9/(guilds|channels)/\d+/messages/search$",
            ))
            .respond_with(SearchResponder(state.clone()))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path_regex(r"^/api/v9/channels/\d+/messages/\d+$"))
            .respond_with(DeleteResponder(state.clone()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/v9/channels/\d+$"))
            .respond_with(ChannelResponder(state.clone()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/v9/channels/\d+/pins$"))
            .respond_with(PinsResponder(state.clone()))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path_regex(r"^/api/v9/channels/\d+/messages/\d+$"))
            .respond_with(EditResponder(state.clone()))
            .mount(&server)
            .await;
        FakeDiscord { server, state }
    }

    pub fn client(&self) -> Client {
        client_for(&self.server)
    }

    /// IDs of messages that still exist.
    pub fn remaining(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .state
            .lock()
            .unwrap()
            .messages
            .iter()
            .map(|m| m.id)
            .collect();
        ids.sort_unstable();
        ids
    }

    pub fn delete_calls(&self) -> usize {
        self.state.lock().unwrap().delete_calls
    }
}

pub fn client_for(server: &MockServer) -> Client {
    let config = ClientConfig {
        api_base: format!("{}/api/v9", server.uri()),
        backoff_base: Duration::from_millis(5),
        ..Default::default()
    };
    Client::with_config(TOKEN, config).unwrap()
}

pub fn guild_target() -> Target {
    Target {
        kind: TargetKind::Guild,
        id: Snowflake(GUILD),
        name: "Test server".into(),
        icon_url: None,
        channels: Vec::new(),
    }
}

pub fn dm_target() -> Target {
    Target {
        kind: TargetKind::Dm,
        id: Snowflake(DM_CHANNEL),
        name: "Friend".into(),
        icon_url: None,
        channels: Vec::new(),
    }
}

/// `minutes` after 2024-01-01T00:00:00Z.
pub fn at(minutes: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap() + TimeDelta::minutes(minutes)
}

/// ID of a message sent `minutes` after 2024-01-01; `seq` keeps IDs unique.
pub fn id_at(minutes: i64, seq: u64) -> u64 {
    Snowflake::from_datetime(at(minutes)).0 + seq
}

pub fn user_json(id: u64, name: &str) -> Value {
    json!({ "id": id.to_string(), "username": name, "global_name": null, "avatar": null })
}

fn message_json(m: &FakeMessage) -> Value {
    json!({
        "id": m.id.to_string(),
        "channel_id": m.channel_id.to_string(),
        "type": m.kind,
        "content": m.text(),
        "author": user_json(m.author_id, "someone"),
        "pinned": m.pinned,
        "attachments": m.files.iter().map(|f| json!({ "filename": f })).collect::<Vec<_>>(),
        "hit": true,
    })
}

fn error_json(status: u16, code: u64, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "message": message, "code": code }))
}

struct SearchResponder(Arc<Mutex<State>>);

impl Respond for SearchResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = self.0.lock().unwrap();
        state.search_calls += 1;
        let calls = state.search_calls;

        let segments: Vec<&str> = request.url.path().split('/').collect();
        let (scope, scope_id) = (segments[3], segments[4].parse::<u64>().unwrap());
        if scope == "guilds" && state.forbidden_guilds.contains(&scope_id) {
            return error_json(403, 50001, "Missing Access");
        }
        let query: HashMap<String, String> = request.url.query_pairs().into_owned().collect();
        let id_param = |name: &str| query.get(name).map(|v| v.parse::<u64>().unwrap());
        let content = query.get("content").map(|c| c.to_lowercase());
        let channels: Vec<u64> = request
            .url
            .query_pairs()
            .filter(|(k, _)| k == "channel_id" && !state.ignore_channel_filter)
            .map(|(_, v)| v.parse().unwrap())
            .collect();
        let has: Vec<String> = request
            .url
            .query_pairs()
            .filter(|(k, _)| k == "has")
            .map(|(_, v)| v.into_owned())
            .collect();
        let (author, min, max) = (
            id_param("author_id"),
            id_param("min_id"),
            id_param("max_id"),
        );

        let mut found: Vec<&FakeMessage> = state
            .messages
            .iter()
            .chain(state.stale.iter())
            .filter(|m| calls > m.hidden_for_searches)
            .filter(|m| match scope {
                "guilds" => m.guild_id == Some(scope_id),
                _ => m.channel_id == scope_id,
            })
            .filter(|m| author.is_none_or(|a| m.author_id == a))
            .filter(|m| channels.is_empty() || channels.contains(&m.channel_id))
            .filter(|m| min.is_none_or(|min| m.id > min) && max.is_none_or(|max| m.id < max))
            // Like Discord's search: word-based and fuzzy, so looser than EraseCord.
            .filter(|m| {
                content.as_deref().is_none_or(|c| {
                    let text = m.text().to_lowercase();
                    c.split_whitespace().any(|w| text.contains(w))
                })
            })
            .filter(|m| has.is_empty() || (has.iter().any(|h| h == "file") && !m.files.is_empty()))
            .collect();
        found.sort_by_key(|m| std::cmp::Reverse(m.id));
        let page: Vec<Value> = found
            .iter()
            .take(25)
            .map(|m| json!([message_json(m)]))
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({
            "total_results": found.len(),
            "messages": page,
        }))
    }
}

struct DeleteResponder(Arc<Mutex<State>>);

impl Respond for DeleteResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = self.0.lock().unwrap();
        state.delete_calls += 1;
        let segments: Vec<&str> = request.url.path().split('/').collect();
        let channel_id: u64 = segments[4].parse().unwrap();
        let message_id: u64 = segments[6].parse().unwrap();
        state.log.push(format!("DELETE {message_id}"));
        let delay = state.delete_delay;

        if let Some(&(status, code)) = state.delete_errors.get(&message_id) {
            return error_json(status, code, "refused");
        }
        let Some(pos) = state
            .messages
            .iter()
            .position(|m| m.id == message_id && m.channel_id == channel_id)
        else {
            return error_json(404, 10008, "Unknown Message");
        };
        let message = state.messages.remove(pos);
        if state.stale_index {
            state.stale.push(message);
        }
        state.deleted.push(message_id);
        ResponseTemplate::new(204).set_delay(delay)
    }
}

struct ChannelResponder(Arc<Mutex<State>>);

impl Respond for ChannelResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let state = self.0.lock().unwrap();
        let id: u64 = request
            .url
            .path()
            .split('/')
            .nth(4)
            .unwrap()
            .parse()
            .unwrap();
        if state.gone_channels.contains(&id) {
            return error_json(404, 10003, "Unknown Channel");
        }
        let guild = state
            .messages
            .iter()
            .find(|m| m.channel_id == id)
            .and_then(|m| m.guild_id);
        ResponseTemplate::new(200).set_body_json(json!({
            "id": id.to_string(),
            "type": if guild.is_some() { 0 } else { 1 },
            "guild_id": guild.map(|g| g.to_string()),
        }))
    }
}

struct PinsResponder(Arc<Mutex<State>>);

impl Respond for PinsResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let state = self.0.lock().unwrap();
        let id: u64 = request
            .url
            .path()
            .split('/')
            .nth(4)
            .unwrap()
            .parse()
            .unwrap();
        let pins: Vec<Value> = state
            .messages
            .iter()
            .filter(|m| m.channel_id == id && m.pinned)
            .map(message_json)
            .collect();
        ResponseTemplate::new(200).set_body_json(pins)
    }
}

struct EditResponder(Arc<Mutex<State>>);

impl Respond for EditResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = self.0.lock().unwrap();
        let message_id: u64 = request
            .url
            .path()
            .split('/')
            .nth(6)
            .unwrap()
            .parse()
            .unwrap();
        state.log.push(format!("PATCH {message_id}"));
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        state.edits.push((message_id, body));
        match state.messages.iter().find(|m| m.id == message_id) {
            Some(m) => ResponseTemplate::new(200).set_body_json(message_json(m)),
            None => error_json(404, 10008, "Unknown Message"),
        }
    }
}
