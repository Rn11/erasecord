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
    /// Answers searches and deletes at random with errors, rate limits,
    /// "index not ready" and broken bodies, this share of the time.
    pub chaos: Option<(rand::rngs::StdRng, f64)>,
    /// Search ignores `max_id` and always answers the first page.
    pub ignore_cursor: bool,
    stale: Vec<FakeMessage>,
    /// Message ID -> (HTTP status, Discord error code) returned on delete.
    pub delete_errors: HashMap<u64, (u16, u64)>,
    /// Search ignores `channel_id`, like a misbehaving index.
    pub ignore_channel_filter: bool,
    /// Only the old, unpaged pins endpoint exists (at most 50 pins).
    pub legacy_pins: bool,
    /// Base URL of the fake file server; set by [`FakeDiscord::start`].
    pub files_base: String,
    /// Attachment links sent to the refresh endpoint.
    pub refreshed: Vec<String>,
    /// Attachment downloads, by file name, in the order they arrived.
    pub downloads: Vec<String>,
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
        let mut state = state;
        state.files_base = format!("{}/files", server.uri());
        let state = Arc::new(Mutex::new(state));
        Mock::given(method("GET"))
            .and(path_regex(r"^/files/\d+/[^/]+$"))
            .respond_with(FileResponder(state.clone()))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/v9/attachments/refresh-urls"))
            .respond_with(RefreshResponder(state.clone()))
            .mount(&server)
            .await;
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
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/v9/channels/\d+/messages/pins$"))
            .respond_with(PagedPinsResponder(state.clone()))
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

/// `files` is where the fake serves attachments, see [`FileResponder`].
fn message_json(m: &FakeMessage, files: &str) -> Value {
    json!({
        "id": m.id.to_string(),
        "channel_id": m.channel_id.to_string(),
        "type": m.kind,
        "content": m.text(),
        "author": user_json(m.author_id, "someone"),
        "pinned": m.pinned,
        "attachments": m
            .files
            .iter()
            .map(|f| json!({ "filename": f, "url": format!("{files}/{}/{f}", m.id) }))
            .collect::<Vec<_>>(),
        "hit": true,
    })
}

fn error_json(status: u16, code: u64, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "message": message, "code": code }))
}

/// Serves "contents of <name>". Names starting with `expired-` only answer
/// once their link was refreshed (`?fresh=1`); `missing-` never answer;
/// `busy-` first answer 429 Too Many Requests.
struct FileResponder(Arc<Mutex<State>>);

impl Respond for FileResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let name = request.url.path().rsplit('/').next().unwrap().to_owned();
        let fresh = request.url.query().is_some_and(|q| q.contains("fresh=1"));
        let mut state = self.0.lock().unwrap();
        let first = !state.downloads.contains(&name);
        state.downloads.push(name.clone());
        if name.starts_with("busy-") && first {
            return ResponseTemplate::new(429).insert_header("retry-after", "0.05");
        }
        if name.starts_with("missing-") || (name.starts_with("expired-") && !fresh) {
            return ResponseTemplate::new(404);
        }
        ResponseTemplate::new(200).set_body_string(format!("contents of {name}"))
    }
}

/// Discord's link refresh: appends `?fresh=1` to every link.
struct RefreshResponder(Arc<Mutex<State>>);

impl Respond for RefreshResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let urls: Vec<String> = body["attachment_urls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u.as_str().unwrap().to_owned())
            .collect();
        self.0
            .lock()
            .unwrap()
            .refreshed
            .extend(urls.iter().cloned());
        let refreshed: Vec<Value> = urls
            .iter()
            .map(|u| json!({ "original": u, "refreshed": format!("{u}?fresh=1") }))
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({ "refreshed_urls": refreshed }))
    }
}

struct SearchResponder(Arc<Mutex<State>>);

impl Respond for SearchResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let mut state = self.0.lock().unwrap();
        state.search_calls += 1;
        let calls = state.search_calls;
        if let Some(chaos) = chaos(&mut state) {
            return chaos;
        }

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
            id_param("max_id").filter(|_| !state.ignore_cursor),
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
            .map(|m| json!([message_json(m, &state.files_base)]))
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
        if let Some(chaos) = chaos(&mut state) {
            return chaos;
        }
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
            .take(50)
            .map(|m| message_json(m, &state.files_base))
            .collect();
        ResponseTemplate::new(200).set_body_json(pins)
    }
}

/// Pins newest first; `pinned_at` is faked from the message ID's time.
struct PagedPinsResponder(Arc<Mutex<State>>);

impl Respond for PagedPinsResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let state = self.0.lock().unwrap();
        if state.legacy_pins {
            return error_json(404, 0, "404: Not Found");
        }
        let id: u64 = request
            .url
            .path()
            .split('/')
            .nth(4)
            .unwrap()
            .parse()
            .unwrap();
        let query: HashMap<String, String> = request.url.query_pairs().into_owned().collect();
        let limit: usize = query["limit"].parse().unwrap();
        let before = query.get("before").cloned();
        let mut pins: Vec<&FakeMessage> = state
            .messages
            .iter()
            .filter(|m| m.channel_id == id && m.pinned)
            .collect();
        pins.sort_by_key(|m| std::cmp::Reverse(m.id));
        let pinned_at = |m: &FakeMessage| Snowflake(m.id).created_at().to_rfc3339();
        let rest: Vec<&FakeMessage> = pins
            .into_iter()
            .filter(|m| before.as_ref().is_none_or(|b| pinned_at(m) < *b))
            .collect();
        let items: Vec<Value> = rest
            .iter()
            .take(limit)
            .map(|m| json!({ "pinned_at": pinned_at(m), "message": message_json(m, &state.files_base) }))
            .collect();
        ResponseTemplate::new(200)
            .set_body_json(json!({ "items": items, "has_more": rest.len() > limit }))
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
            Some(m) => ResponseTemplate::new(200).set_body_json(message_json(m, &state.files_base)),
            None => error_json(404, 10008, "Unknown Message"),
        }
    }
}

/// A random failure, if chaos is on and it is time for one.
fn chaos(state: &mut State) -> Option<ResponseTemplate> {
    use rand::RngExt;
    let (rng, rate) = state.chaos.as_mut()?;
    if !rng.random_bool(*rate) {
        return None;
    }
    Some(match rng.random_range(0..5) {
        0 => error_json(500, 0, "Internal Server Error"),
        1 => ResponseTemplate::new(429)
            .set_body_json(json!({ "message": "You are being rate limited.", "retry_after": 0.001, "global": false })),
        2 => ResponseTemplate::new(202)
            .set_body_json(json!({ "message": "Index not yet available.", "code": 110000, "retry_after": 0.001 })),
        3 => ResponseTemplate::new(502).set_body_string("<html>Bad Gateway</html>"),
        _ => ResponseTemplate::new(200).set_body_string("{\"total_results\": 3, \"messages\": [[{\"id\""),
    })
}
