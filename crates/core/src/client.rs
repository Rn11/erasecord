//! HTTP client for the parts of the Discord API that purgecord needs.

use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::{Method, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::models::{Channel, Guild, Relationship, SearchResponse, User};
use crate::ratelimit::{self, RateLimiter};
use crate::search::{Scope, SearchQuery};
use crate::snowflake::Snowflake;

pub const DEFAULT_API_BASE: &str = "https://discord.com/api/v9";

/// Requests with a user token normally come from a browser or the official
/// client, so purgecord presents itself as a browser.
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

/// Discord returns at most this many servers per request.
const GUILD_PAGE_SIZE: usize = 200;

// 202 and 429 answers normally stop after a few seconds; these caps only keep
// a misbehaving server from stalling a job forever.
const MAX_INDEX_WAITS: u32 = 30;
const MAX_RATE_LIMIT_WAITS: u32 = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientConfig {
    pub api_base: String,
    pub user_agent: String,
    /// How often a request is retried after network errors or 5xx answers.
    pub max_retries: u32,
    /// First wait after a network error or 5xx answer; doubles each attempt.
    pub backoff_base: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            api_base: DEFAULT_API_BASE.to_owned(),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            max_retries: 5,
            backoff_base: Duration::from_secs(2),
        }
    }
}

/// Reported while the client waits, so a UI can explain why nothing happens.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Notice {
    RateLimited {
        wait_ms: u64,
        global: bool,
    },
    /// Discord is still building the search index for a server or DM.
    IndexNotReady {
        wait_ms: u64,
    },
    Retrying {
        reason: String,
        attempt: u32,
        wait_ms: u64,
    },
}

pub type NoticeSink = Arc<dyn Fn(Notice) + Send + Sync>;

/// A Discord API client authenticated with a user token. Cheap to clone.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    config: ClientConfig,
    limiter: RateLimiter,
    notices: RwLock<Option<NoticeSink>>,
}

impl fmt::Debug for Client {
    // Deliberately leaves out the token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("api_base", &self.inner.config.api_base)
            .finish_non_exhaustive()
    }
}

impl Client {
    pub fn new(token: &str) -> Result<Self> {
        Self::with_config(token, ClientConfig::default())
    }

    pub fn with_config(token: &str, config: ClientConfig) -> Result<Self> {
        let token = normalize_token(token);
        if token.is_empty() {
            return Err(Error::InvalidToken);
        }
        let mut auth = HeaderValue::from_str(token).map_err(|_| Error::InvalidToken)?;
        auth.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth);

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .user_agent(config.user_agent.clone())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                config,
                limiter: RateLimiter::default(),
                notices: RwLock::new(None),
            }),
        })
    }

    /// Where to report waits and retries; `None` stops reporting.
    pub fn set_notice_sink(&self, sink: Option<NoticeSink>) {
        *self.inner.notices.write().unwrap() = sink;
    }

    pub async fn current_user(&self) -> Result<User> {
        self.get("/users/@me", &[]).await
    }

    pub async fn guilds(&self) -> Result<Vec<Guild>> {
        let mut guilds = Vec::new();
        let mut after: Option<Snowflake> = None;
        loop {
            let mut query = vec![("limit", GUILD_PAGE_SIZE.to_string())];
            if let Some(after) = after {
                query.push(("after", after.to_string()));
            }
            let page: Vec<Guild> = self.get("/users/@me/guilds", &query).await?;
            let full = page.len() == GUILD_PAGE_SIZE;
            after = page.iter().map(|g| g.id).max();
            guilds.extend(page);
            if !full {
                return Ok(guilds);
            }
        }
    }

    /// Open DMs and group DMs.
    pub async fn private_channels(&self) -> Result<Vec<Channel>> {
        self.get("/users/@me/channels", &[]).await
    }

    /// Friends, blocked users and friend requests.
    pub async fn relationships(&self) -> Result<Vec<Relationship>> {
        self.get("/users/@me/relationships", &[]).await
    }

    /// Opens the DM with a user, or returns it if it is open already. Only
    /// the user's own DM list changes; the other person is not notified.
    pub async fn open_dm(&self, user_id: Snowflake) -> Result<Channel> {
        let body = serde_json::json!({ "recipient_id": user_id.to_string() });
        let response = self
            .send(Method::POST, "/users/@me/channels", &[], Some(&body))
            .await?;
        Ok(serde_json::from_slice(&response.bytes().await?)?)
    }

    /// The channels of a server that the user can see.
    pub async fn guild_channels(&self, guild_id: Snowflake) -> Result<Vec<Channel>> {
        self.get(&format!("/guilds/{guild_id}/channels"), &[]).await
    }

    pub async fn channel(&self, channel_id: Snowflake) -> Result<Channel> {
        self.get(&format!("/channels/{channel_id}"), &[]).await
    }

    pub async fn search(&self, scope: Scope, query: &SearchQuery) -> Result<SearchResponse> {
        self.get(&scope.search_path(), &query.params()).await
    }

    pub async fn delete_message(&self, channel_id: Snowflake, message_id: Snowflake) -> Result<()> {
        let path = format!("/channels/{channel_id}/messages/{message_id}");
        self.send(Method::DELETE, &path, &[], None).await?;
        Ok(())
    }

    /// Replaces the text of a message and removes its attachments.
    pub async fn overwrite_message(
        &self,
        channel_id: Snowflake,
        message_id: Snowflake,
        content: &str,
    ) -> Result<()> {
        let path = format!("/channels/{channel_id}/messages/{message_id}");
        let body = serde_json::json!({ "content": content, "attachments": [] });
        self.send(Method::PATCH, &path, &[], Some(&body)).await?;
        Ok(())
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let response = self.send(Method::GET, path, query, None).await?;
        let body = response.bytes().await?;
        Ok(serde_json::from_slice(&body)?)
    }

    /// Sends a request, waiting out rate limits and retrying transient errors.
    async fn send(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
    ) -> Result<Response> {
        let url = format!("{}{}", self.inner.config.api_base, path);
        let max_retries = self.inner.config.max_retries;
        let (mut failures, mut index_waits, mut rate_limit_waits) = (0, 0, 0);
        loop {
            self.inner.limiter.ready().await;
            let mut request = self.inner.http.request(method.clone(), &url).query(query);
            if let Some(body) = body {
                request = request.json(body);
            }
            let result = request.send().await;
            let response = match result {
                Ok(response) => response,
                Err(err) if failures < max_retries && is_transient(&err) => {
                    failures += 1;
                    self.back_off(failures, err.to_string()).await;
                    continue;
                }
                Err(err) => return Err(err.into()),
            };
            self.inner.limiter.observe(response.headers());

            let status = response.status();
            match status {
                StatusCode::ACCEPTED => {
                    let body = json_body(response).await;
                    if index_waits >= MAX_INDEX_WAITS {
                        return Err(api_error(status, &body));
                    }
                    index_waits += 1;
                    let wait = ratelimit::seconds(body["retry_after"].as_f64().unwrap_or(2.0));
                    self.notify(Notice::IndexNotReady {
                        wait_ms: millis(wait),
                    });
                    tokio::time::sleep(wait).await;
                }
                StatusCode::TOO_MANY_REQUESTS => {
                    let header = ratelimit::header_f64(response.headers(), "retry-after");
                    let body = json_body(response).await;
                    if rate_limit_waits >= MAX_RATE_LIMIT_WAITS {
                        return Err(api_error(status, &body));
                    }
                    rate_limit_waits += 1;
                    let seconds = body["retry_after"].as_f64().or(header).unwrap_or(5.0);
                    let wait = ratelimit::seconds(seconds);
                    self.inner.limiter.block_for(wait);
                    self.notify(Notice::RateLimited {
                        wait_ms: millis(wait),
                        global: body["global"].as_bool().unwrap_or(false),
                    });
                }
                StatusCode::UNAUTHORIZED => return Err(Error::Unauthorized),
                s if s.is_server_error() && failures < max_retries => {
                    failures += 1;
                    self.back_off(failures, format!("HTTP {}", s.as_u16()))
                        .await;
                }
                s if s.is_success() => return Ok(response),
                _ => {
                    let body = json_body(response).await;
                    return Err(api_error(status, &body));
                }
            }
        }
    }

    async fn back_off(&self, attempt: u32, reason: String) {
        let wait = self.inner.config.backoff_base * 2u32.pow(attempt.saturating_sub(1).min(6));
        self.notify(Notice::Retrying {
            reason,
            attempt,
            wait_ms: millis(wait),
        });
        tokio::time::sleep(wait).await;
    }

    fn notify(&self, notice: Notice) {
        tracing::info!(?notice, "waiting for Discord");
        let sink = self.inner.notices.read().unwrap().clone();
        if let Some(sink) = sink {
            sink(notice);
        }
    }
}

fn is_transient(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

async fn json_body(response: Response) -> Value {
    match response.bytes().await {
        Ok(body) => serde_json::from_slice(&body).unwrap_or(Value::Null),
        Err(_) => Value::Null,
    }
}

fn api_error(status: StatusCode, body: &Value) -> Error {
    let message = body["message"]
        .as_str()
        .or(status.canonical_reason())
        .unwrap_or("unknown error");
    Error::Api {
        status: status.as_u16(),
        code: body["code"].as_u64(),
        message: message.to_owned(),
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

/// Accepts a token the way people tend to copy it: with surrounding
/// whitespace or the quotes the browser console shows.
fn normalize_token(token: &str) -> &str {
    token.trim().trim_matches('"').trim()
}
