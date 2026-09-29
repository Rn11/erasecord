//! Saving a message's attachments before it is deleted, since their links
//! stop working once the message is gone.
//!
//! Files go to `<folder>/attachments/<channel ID>/<message ID>_<n>_<name>`.
//! Only Discord's own file servers are contacted (and, for testing against
//! a fake API, that API's own server), and without the token. Files are
//! fetched one at a time with a pause before each (see [`crate::pace`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use reqwest::{StatusCode, Url};
use tokio::io::AsyncWriteExt;

use crate::client::Client;
use crate::models::Message;
use crate::pace::Pace;

/// Hosts that serve Discord attachments. Anything else is never fetched.
const ATTACHMENT_HOSTS: [&str; 3] = [
    "cdn.discordapp.com",
    "media.discordapp.net",
    "cdn.discord.com",
];
const MAX_ATTEMPTS: u32 = 3;
/// The longest a "too many requests" answer is waited out.
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(60);

pub struct Backup {
    folder: PathBuf,
    /// A plain client: the token must never reach the file servers.
    http: reqwest::Client,
    /// How long to wait between downloads; none without it.
    pace: Option<Arc<Pace>>,
    /// Something was downloaded already, so the next one waits first.
    fetched: AtomicBool,
}

impl Backup {
    /// Prepares `folder`, creating it if needed.
    pub fn new(folder: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(folder.join("attachments"))?;
        Self::downloader(folder)
    }

    /// Only downloads; nothing is written to `folder`.
    pub(crate) fn downloader(folder: &Path) -> std::io::Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(std::io::Error::other)?;
        Ok(Backup {
            folder: folder.to_owned(),
            http,
            pace: None,
            fetched: AtomicBool::new(false),
        })
    }

    /// Waits between downloads, and longer after being told to slow down.
    pub(crate) fn set_pace(&mut self, pace: Arc<Pace>) {
        self.pace = Some(pace);
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Saves every attachment of `message` and returns their paths relative
    /// to the backup folder. Files saved by an earlier run are kept. Any
    /// failure is returned as a message; the caller must then keep the
    /// message rather than lose the files.
    pub async fn save(&self, client: &Client, message: &Message) -> Result<Vec<String>, String> {
        let mut saved = Vec::with_capacity(message.attachments.len());
        for (index, attachment) in message.attachments.iter().enumerate() {
            let (url, name, relative) = attachment_path(message, index, attachment)?;
            let path = self.folder.join(&relative);
            if tokio::fs::try_exists(&path).await.unwrap_or(false) {
                saved.push(relative);
                continue;
            }
            self.download(client, &url, Dest::File(&path))
                .await
                .map_err(|err| format!("could not save {name}: {err}"))?;
            saved.push(relative);
        }
        Ok(saved)
    }

    /// Downloads an attachment into memory, for the encrypted backup.
    pub(crate) async fn download_bytes(
        &self,
        client: &Client,
        url: &str,
    ) -> Result<Vec<u8>, String> {
        let mut data = Vec::new();
        self.download(client, url, Dest::Memory(&mut data)).await?;
        Ok(data)
    }

    async fn download(&self, client: &Client, url: &str, mut dest: Dest<'_>) -> Result<(), String> {
        check_host(url, client.api_base())?;
        let api = client.api_base();
        let mut url = url.to_owned();
        let mut refreshed = false;
        let mut attempt = 0;
        loop {
            attempt += 1;
            if let Some(pace) = &self.pace {
                if self.fetched.swap(true, Ordering::Relaxed) {
                    tokio::time::sleep(pace.before_download()).await;
                }
            }
            match self.fetch(&url, &mut dest).await {
                Ok(()) => return Ok(()),
                // Expired link (typical for data packages): ask for a fresh one.
                Err(Fetch::Status(StatusCode::NOT_FOUND | StatusCode::FORBIDDEN)) if !refreshed => {
                    refreshed = true;
                    let fresh = client
                        .refresh_attachment_urls(std::slice::from_ref(&url))
                        .await
                        .map_err(|err| {
                            format!("the link has expired and could not be renewed: {err}")
                        })?;
                    let fresh = fresh.into_iter().next().unwrap_or_default();
                    if fresh == url {
                        return Err("the file is no longer available".into());
                    }
                    check_host(&fresh, api)?;
                    url = fresh;
                }
                Err(Fetch::TooManyRequests(wait)) if attempt < MAX_ATTEMPTS => {
                    if let Some(pace) = &self.pace {
                        pace.slow_down();
                    }
                    tokio::time::sleep(wait).await;
                }
                Err(Fetch::Status(status))
                    if status.is_server_error() && attempt < MAX_ATTEMPTS =>
                {
                    tokio::time::sleep(Duration::from_secs(2 * u64::from(attempt))).await;
                }
                Err(Fetch::Network(_)) if attempt < MAX_ATTEMPTS => {
                    tokio::time::sleep(Duration::from_secs(2 * u64::from(attempt))).await;
                }
                Err(err) => return Err(err.to_string()),
            }
        }
    }

    /// Downloads into memory, or into a temporary file next to the target
    /// that is then renamed, so a half-written file never looks complete.
    async fn fetch(&self, url: &str, dest: &mut Dest<'_>) -> Result<(), Fetch> {
        let mut response = self.http.get(url).send().await.map_err(Fetch::Network)?;
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            let wait = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|secs| secs.is_finite() && *secs >= 0.0)
                .map_or(Duration::from_secs(5), |secs| {
                    Duration::from_secs_f64(secs.min(MAX_RATE_LIMIT_WAIT.as_secs_f64()))
                });
            return Err(Fetch::TooManyRequests(wait));
        }
        if !response.status().is_success() {
            return Err(Fetch::Status(response.status()));
        }
        let path = match dest {
            Dest::File(path) => *path,
            Dest::Memory(data) => {
                data.clear();
                while let Some(chunk) = response.chunk().await.map_err(Fetch::Network)? {
                    data.extend_from_slice(&chunk);
                }
                return Ok(());
            }
        };
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(Fetch::Io)?;
        }
        let partial = path.with_extension("part");
        let mut file = tokio::fs::File::create(&partial).await.map_err(Fetch::Io)?;
        let result: Result<(), Fetch> = async {
            while let Some(chunk) = response.chunk().await.map_err(Fetch::Network)? {
                file.write_all(&chunk).await.map_err(Fetch::Io)?;
            }
            file.flush().await.map_err(Fetch::Io)?;
            Ok(())
        }
        .await;
        drop(file);
        match result {
            Ok(()) => tokio::fs::rename(&partial, path).await.map_err(Fetch::Io),
            Err(err) => {
                let _ = tokio::fs::remove_file(&partial).await;
                Err(err)
            }
        }
    }
}

enum Dest<'a> {
    File(&'a Path),
    Memory(&'a mut Vec<u8>),
}

enum Fetch {
    Status(StatusCode),
    /// How long the file server asked to wait.
    TooManyRequests(Duration),
    Network(reqwest::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for Fetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fetch::Status(status) => write!(f, "the file server answered {status}"),
            Fetch::TooManyRequests(_) => write!(f, "the file server kept asking to slow down"),
            Fetch::Network(err) => write!(f, "network error: {err}"),
            Fetch::Io(err) => write!(f, "{err}"),
        }
    }
}

/// Discord's file servers over HTTPS, or the configured API server itself
/// (which is discord.com in production and a fake server in tests).
/// An attachment's link, file name and path in the backup:
/// `attachments/<channel ID>/<message ID>_<n>_<name>`.
pub(crate) fn attachment_path(
    message: &Message,
    index: usize,
    attachment: &serde_json::Value,
) -> Result<(String, String, String), String> {
    let url = attachment["url"]
        .as_str()
        .ok_or("an attachment has no link")?
        .to_owned();
    let name = attachment["filename"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| file_name_of(&url));
    let relative = format!(
        "attachments/{}/{}_{}_{}",
        message.channel_id,
        message.id,
        index + 1,
        safe_file_name(&name)
    );
    Ok((url, name, relative))
}

fn check_host(url: &str, api_base: &str) -> Result<(), String> {
    let parsed = Url::parse(url).map_err(|_| format!("not a valid link: {url}"))?;
    let discord = parsed.scheme() == "https"
        && parsed
            .host_str()
            .is_some_and(|host| ATTACHMENT_HOSTS.contains(&host));
    let same_server = Url::parse(api_base).is_ok_and(|api| api.origin() == parsed.origin());
    let allowed = discord || same_server;
    if allowed {
        Ok(())
    } else {
        Err(format!("not a Discord attachment link: {url}"))
    }
}

fn file_name_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/').next().unwrap_or("file").to_owned()
}

/// A file name that is valid everywhere: no path separators or characters
/// Windows rejects, not empty, at most 100 characters.
fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    let cleaned = if cleaned.is_empty() { "file" } else { cleaned };
    if cleaned.chars().count() <= 100 {
        return cleaned.to_owned();
    }
    // Keep the extension when shortening.
    match cleaned.rsplit_once('.') {
        Some((stem, ext)) if ext.chars().count() <= 10 => {
            let stem: String = stem.chars().take(100 - ext.chars().count() - 1).collect();
            format!("{stem}.{ext}")
        }
        _ => cleaned.chars().take(100).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_discord_file_servers_are_contacted() {
        let api = crate::client::DEFAULT_API_BASE;
        let ok = |url: &str| check_host(url, api).is_ok();
        assert!(ok("https://cdn.discordapp.com/attachments/1/2/a.png?ex=1"));
        assert!(ok("https://media.discordapp.net/attachments/1/2/a.png"));
        assert!(!ok("http://cdn.discordapp.com/a.png"));
        assert!(!ok("https://cdn.discordapp.com.evil.example/a.png"));
        assert!(!ok("https://example.com/a.png"));
        assert!(!ok("not a url"));
        // A fake API server in tests may serve its own files, nothing else.
        let fake = "http://127.0.0.1:8765/api/v9";
        assert!(check_host("http://127.0.0.1:8765/files/a.png", fake).is_ok());
        assert!(check_host("http://127.0.0.1:9999/files/a.png", fake).is_err());
    }

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(safe_file_name("cat.png"), "cat.png");
        assert_eq!(safe_file_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_file_name("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(safe_file_name("..."), "file");
        let long = format!("{}.jpeg", "x".repeat(300));
        let short = safe_file_name(&long);
        assert_eq!(short.chars().count(), 100);
        assert!(short.ends_with(".jpeg"));
        assert_eq!(
            file_name_of("https://cdn/x/y/photo.jpg?ex=1&is=2"),
            "photo.jpg"
        );
    }
}
