//! Discord's data package ("Request all of my data" in the privacy
//! settings): every message the user ever sent, with its channel and message
//! ID. Deleting from it needs no search, so it also reaches messages that the
//! search does not find, and conversations that are no longer listed.
//!
//! Both layouts Discord has used are understood: `messages/c<id>/` with
//! `channel.json` and `messages.csv`, and `Messages/c<id>/` with
//! `channel.json` and `messages.json`, each with an `index.json` naming the
//! conversations. The package can be the `.zip` file or an extracted folder.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::models::{Message, User};
use crate::snowflake::Snowflake;
use crate::targets::{Target, TargetKind};

/// A message from the package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageMessage {
    pub id: Snowflake,
    pub content: String,
    /// Attachment URLs.
    pub attachments: Vec<String>,
}

impl PackageMessage {
    /// The message as the API would describe it, as far as the package
    /// knows. Embeds and stickers are unknown.
    pub fn to_message(&self, channel_id: Snowflake, author: Snowflake) -> Message {
        let attachments = self
            .attachments
            .iter()
            .map(|url| {
                let path = url.split(['?', '#']).next().unwrap_or(url);
                let filename = path.rsplit('/').next().unwrap_or(path);
                serde_json::json!({ "filename": filename, "url": url })
            })
            .collect();
        Message {
            id: self.id,
            channel_id,
            kind: 0,
            content: self.content.clone(),
            author: User {
                id: author,
                username: String::new(),
                global_name: None,
                avatar: None,
            },
            pinned: false,
            attachments,
            embeds: Vec::new(),
            sticker_items: Vec::new(),
            hit: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageChannel {
    pub id: Snowflake,
    pub kind: TargetKind,
    /// Channel name for server channels, the other person for DMs.
    pub name: String,
    /// For server channels, if the package says which server.
    pub guild: Option<(Snowflake, String)>,
    /// Newest first.
    pub messages: Vec<PackageMessage>,
    /// For DMs and group DMs: the people in it, including the owner.
    pub recipients: Vec<Snowflake>,
}

/// A server or DM in the package, as a [`Target`] with some numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageTarget {
    pub target: Target,
    pub messages: u64,
    pub channels: Vec<PackageChannelInfo>,
    pub first_message: Option<DateTime<Utc>>,
    pub last_message: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageChannelInfo {
    pub id: Snowflake,
    pub name: String,
    pub messages: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Package {
    /// The account the package belongs to, from `account/user.json`.
    pub owner: Option<Snowflake>,
    pub channels: Vec<PackageChannel>,
}

impl Package {
    /// Reads a package from its `.zip` file or an extracted folder.
    pub fn open(path: &Path) -> Result<Package> {
        let io = |err: std::io::Error| Error::Package(format!("{}: {err}", path.display()));
        if path.is_dir() {
            let mut files = Vec::new();
            collect_files(path, path, 0, &mut files).map_err(io)?;
            Package::from_files(files)
        } else {
            let file = File::open(path).map_err(io)?;
            let mut zip = zip::ZipArchive::new(BufReader::new(file))
                .map_err(|err| Error::Package(format!("not a zip file: {err}")))?;
            let mut files = Vec::new();
            for index in 0..zip.len() {
                let mut entry = zip
                    .by_index(index)
                    .map_err(|err| Error::Package(format!("damaged zip file: {err}")))?;
                if !entry.is_file() || message_file(entry.name()).is_none() {
                    continue;
                }
                let name = entry.name().to_owned();
                let mut data = Vec::with_capacity(entry.size().min(64 << 20) as usize);
                entry.read_to_end(&mut data).map_err(io)?;
                files.push((name, data));
            }
            Package::from_files(files)
        }
    }

    /// Builds a package from `(path inside the package, contents)` pairs;
    /// files outside the messages folder are ignored.
    pub fn from_files(files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Result<Package> {
        let mut index: HashMap<Snowflake, String> = HashMap::new();
        let mut found_index = false;
        let mut owner = None;
        let mut channels: HashMap<Snowflake, ChannelFiles> = HashMap::new();
        for (name, data) in files {
            let Some(file) = message_file(&name) else {
                continue;
            };
            match file {
                MessageFile::Owner => {
                    let user: Value = serde_json::from_slice(&data)
                        .map_err(|err| Error::Package(format!("{name}: {err}")))?;
                    owner = id_of(&user["id"]);
                }
                MessageFile::Index => {
                    found_index = true;
                    let names: HashMap<String, Option<String>> = serde_json::from_slice(&data)
                        .map_err(|err| Error::Package(format!("{name}: {err}")))?;
                    for (id, label) in names {
                        if let (Ok(id), Some(label)) = (id.parse(), label) {
                            index.insert(Snowflake(id), label);
                        }
                    }
                }
                MessageFile::Channel(id) => channels.entry(id).or_default().channel = Some(data),
                MessageFile::Csv(id) => channels.entry(id).or_default().csv = Some(data),
                MessageFile::Json(id) => channels.entry(id).or_default().json = Some(data),
            }
        }
        if !found_index && channels.is_empty() {
            return Err(Error::Package(
                "this does not look like a Discord data package: it has no messages folder".into(),
            ));
        }

        // Channels are independent: parse them on all cores.
        let channels: Vec<(Snowflake, ChannelFiles)> = channels.into_iter().collect();
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
        let chunk = channels.len().div_ceil(workers).max(1);
        let results: Vec<Result<Vec<PackageChannel>>> = std::thread::scope(|scope| {
            let handles: Vec<_> = channels
                .chunks(chunk)
                .map(|part| {
                    let index = &index;
                    scope.spawn(move || {
                        part.iter()
                            .filter_map(|(id, files)| parse_channel(*id, files, index).transpose())
                            .collect::<Result<Vec<_>>>()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("parser thread"))
                .collect()
        });
        let mut parsed = Vec::with_capacity(channels.len());
        for result in results {
            parsed.extend(result?);
        }
        parsed.sort_by_key(|c| std::cmp::Reverse(c.messages.first().map(|m| m.id)));
        Ok(Package {
            owner,
            channels: parsed,
        })
    }

    /// Refuses a package that belongs to another account: its messages are
    /// not the logged-in user's, even though moderators could delete them.
    /// A package without `account/user.json` is accepted.
    pub fn check_owner(&self, me: Snowflake) -> Result<()> {
        match self.owner {
            Some(owner) if owner != me => Err(Error::Package(format!(
                "it belongs to another account (ID {owner}), not to the one you are logged in with ({me})"
            ))),
            _ => Ok(()),
        }
    }

    pub fn message_count(&self) -> u64 {
        self.channels.iter().map(|c| c.messages.len() as u64).sum()
    }

    /// The servers and DMs in the package: servers sorted by name, then DMs,
    /// most recently used first. Channels whose server is unknown become
    /// servers of their own.
    pub fn targets(&self) -> Vec<PackageTarget> {
        let mut servers: Vec<PackageTarget> = Vec::new();
        let mut dms: Vec<PackageTarget> = Vec::new();
        for channel in &self.channels {
            let info = PackageChannelInfo {
                id: channel.id,
                name: channel.name.clone(),
                messages: channel.messages.len() as u64,
            };
            let (group, kind, id, name) = match (&channel.guild, channel.kind) {
                (Some((guild_id, guild_name)), _) => (
                    &mut servers,
                    TargetKind::Guild,
                    *guild_id,
                    guild_name.clone(),
                ),
                (None, TargetKind::Guild) => (
                    &mut servers,
                    TargetKind::Guild,
                    channel.id,
                    format!("#{} (unknown server)", channel.name),
                ),
                (None, kind) => (&mut dms, kind, channel.id, channel.name.clone()),
            };
            let entry = match group.iter().position(|t| t.target.id == id) {
                Some(index) => &mut group[index],
                None => {
                    group.push(PackageTarget {
                        target: Target {
                            kind,
                            id,
                            name,
                            icon_url: None,
                            channels: Vec::new(),
                        },
                        messages: 0,
                        channels: Vec::new(),
                        first_message: None,
                        last_message: None,
                    });
                    group.last_mut().expect("just pushed")
                }
            };
            entry.messages += info.messages;
            entry.channels.push(info);
            let newest = channel.messages.first().map(|m| m.id.created_at());
            let oldest = channel.messages.last().map(|m| m.id.created_at());
            entry.last_message = entry.last_message.max(newest);
            entry.first_message = match (entry.first_message, oldest) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        servers.sort_by_key(|t| t.target.name.to_lowercase());
        for server in &mut servers {
            server
                .channels
                .sort_by_key(|c| (std::cmp::Reverse(c.messages), c.name.to_lowercase()));
        }
        servers.extend(dms);
        servers
    }

    /// The package's channels that belong to `target`, respecting
    /// `target.channels`.
    pub fn channels_of<'a>(
        &'a self,
        target: &'a Target,
    ) -> impl Iterator<Item = &'a PackageChannel> {
        self.channels.iter().filter(move |c| {
            let belongs = match (&c.guild, target.kind) {
                (Some((guild_id, _)), TargetKind::Guild) => *guild_id == target.id,
                (None, _) => c.id == target.id,
                _ => false,
            };
            belongs && target.covers_channel(c.id)
        })
    }
}

/// Gives the package's DMs and group DMs the names and pictures they have
/// now, taken from `live` (the open conversations, see
/// [`Target::from_channel`]). The package only knows the other people by ID,
/// so a group DM without a name would otherwise show up as "Unnamed group".
/// Conversations that are no longer open keep the name from the package.
pub fn apply_live_names(targets: &mut [PackageTarget], live: &[Target]) {
    for item in targets {
        let target = &mut item.target;
        if target.kind == TargetKind::Guild {
            continue;
        }
        if let Some(current) = live.iter().find(|t| t.id == target.id) {
            target.name.clone_from(&current.name);
            target.icon_url.clone_from(&current.icon_url);
        }
    }
}

#[derive(Default)]
struct ChannelFiles {
    channel: Option<Vec<u8>>,
    csv: Option<Vec<u8>>,
    json: Option<Vec<u8>>,
}

enum MessageFile {
    /// `account/user.json`, which names the package's owner.
    Owner,
    Index,
    Channel(Snowflake),
    Csv(Snowflake),
    Json(Snowflake),
}

/// Recognises `…/messages/index.json` and `…/messages/[c]<id>/<file>`, in
/// any case and below any number of outer folders.
fn message_file(path: &str) -> Option<MessageFile> {
    let parts: Vec<&str> = path.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if let [.., folder, file] = parts.as_slice() {
        if folder.eq_ignore_ascii_case("account") && file.eq_ignore_ascii_case("user.json") {
            return Some(MessageFile::Owner);
        }
    }
    let root = parts
        .iter()
        .rposition(|p| p.eq_ignore_ascii_case("messages"))?;
    match &parts[root + 1..] {
        [file] if file.eq_ignore_ascii_case("index.json") => Some(MessageFile::Index),
        [folder, file] => {
            let digits = folder.strip_prefix(['c', 'C']).unwrap_or(folder);
            let id = Snowflake(digits.parse().ok()?);
            match file.to_ascii_lowercase().as_str() {
                "channel.json" => Some(MessageFile::Channel(id)),
                "messages.csv" => Some(MessageFile::Csv(id)),
                "messages.json" => Some(MessageFile::Json(id)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn collect_files(
    root: &Path,
    dir: &Path,
    depth: usize,
    files: &mut Vec<(String, Vec<u8>)>,
) -> std::io::Result<()> {
    if depth > 4 {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_files(root, &path, depth + 1, files)?;
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let name = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if message_file(&name).is_some() {
            files.push((name, std::fs::read(&path)?));
        }
    }
    Ok(())
}

fn parse_channel(
    id: Snowflake,
    files: &ChannelFiles,
    index: &HashMap<Snowflake, String>,
) -> Result<Option<PackageChannel>> {
    let mut messages = match (&files.json, &files.csv) {
        (Some(json), _) => parse_json_messages(json)
            .map_err(|err| Error::Package(format!("messages of channel {id}: {err}")))?,
        (None, Some(csv)) => parse_csv_messages(csv)
            .map_err(|err| Error::Package(format!("messages of channel {id}: {err}")))?,
        (None, None) => return Ok(None),
    };
    if messages.is_empty() {
        return Ok(None);
    }
    messages.sort_by_key(|m| std::cmp::Reverse(m.id));
    messages.dedup_by_key(|m| m.id);
    let info: Value = files
        .channel
        .as_deref()
        .and_then(|data| serde_json::from_slice(data).ok())
        .unwrap_or(Value::Null);
    Ok(Some(describe_channel(id, &info, index.get(&id), messages)))
}

fn describe_channel(
    id: Snowflake,
    info: &Value,
    label: Option<&String>,
    messages: Vec<PackageMessage>,
) -> PackageChannel {
    let kind_number = info["type"].as_u64();
    let kind_name = info["type"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let guild = info["guild"]["id"].as_str().and_then(|guild_id| {
        let name = info["guild"]["name"]
            .as_str()
            .unwrap_or("Unknown server")
            .to_owned();
        Some((Snowflake(guild_id.parse().ok()?), name))
    });
    let kind = if guild.is_some() {
        TargetKind::Guild
    } else if kind_number == Some(1) || kind_name == "DM" {
        TargetKind::Dm
    } else if kind_number == Some(3) || kind_name == "GROUP_DM" {
        TargetKind::GroupDm
    } else if kind_number.is_some()
        || kind_name.starts_with("GUILD")
        || kind_name.contains("THREAD")
    {
        TargetKind::Guild
    } else {
        TargetKind::Dm
    };
    let label = label.map(|l| l.trim()).filter(|l| !l.is_empty());
    let name = match kind {
        TargetKind::Guild => info["name"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| label.map(|l| l.split(" in ").next().unwrap_or(l).to_owned()))
            .unwrap_or_else(|| id.to_string()),
        TargetKind::Dm | TargetKind::GroupDm => label
            .map(|l| l.strip_prefix("Direct Message with ").unwrap_or(l))
            .map(|l| l.strip_suffix("#0").unwrap_or(l).to_owned())
            .or_else(|| info["name"].as_str().map(str::to_owned))
            .unwrap_or_else(|| match kind {
                TargetKind::GroupDm => "Unnamed group".to_owned(),
                _ => format!("Conversation {id}"),
            }),
    };
    let recipients = info["recipients"]
        .as_array()
        .map(|list| list.iter().filter_map(id_of).collect())
        .unwrap_or_default();
    PackageChannel {
        id,
        kind,
        name,
        guild,
        messages,
        recipients,
    }
}

fn parse_attachments(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

fn id_of(value: &Value) -> Option<Snowflake> {
    match value {
        Value::Number(n) => n.as_u64().map(Snowflake),
        Value::String(s) => s.trim().parse().ok().map(Snowflake),
        _ => None,
    }
}

/// A row of `messages.json`; other fields, like `Timestamp`, are not needed
/// because the ID says when a message was sent.
#[derive(Deserialize)]
struct JsonRow {
    #[serde(alias = "id", alias = "Id", default, deserialize_with = "any_id")]
    #[serde(rename = "ID")]
    id: Option<Snowflake>,
    #[serde(alias = "contents", alias = "Content", alias = "content", default)]
    #[serde(rename = "Contents")]
    contents: Option<String>,
    #[serde(alias = "attachments", default)]
    #[serde(rename = "Attachments")]
    attachments: Option<String>,
}

/// An ID written as a number or as text.
fn any_id<'de, D: serde::Deserializer<'de>>(de: D) -> Result<Option<Snowflake>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Number(u64),
        Text(String),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Id::deserialize(de)? {
        Id::Number(n) => Some(Snowflake(n)),
        Id::Text(s) => s.trim().parse().ok().map(Snowflake),
        Id::Other(_) => None,
    })
}

fn parse_json_messages(data: &[u8]) -> Result<Vec<PackageMessage>, String> {
    let rows: Vec<JsonRow> = serde_json::from_slice(data).map_err(|err| err.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(PackageMessage {
                id: row.id?,
                content: row.contents.unwrap_or_default(),
                attachments: parse_attachments(row.attachments.as_deref().unwrap_or_default()),
            })
        })
        .collect())
}

fn parse_csv_messages(data: &[u8]) -> Result<Vec<PackageMessage>, String> {
    let text = String::from_utf8_lossy(data);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let mut rows = parse_csv(text).into_iter();
    let header = rows.next().ok_or("empty file")?;
    let column = |name: &str| {
        header
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case(name))
    };
    let id = column("id").ok_or("no ID column")?;
    let (contents, attachments) = (column("contents"), column("attachments"));
    Ok(rows
        .filter_map(|row| {
            let cell = |i: Option<usize>| i.and_then(|i| row.get(i)).map_or("", String::as_str);
            Some(PackageMessage {
                id: Snowflake(row.get(id)?.trim().parse().ok()?),
                content: cell(contents).to_owned(),
                attachments: parse_attachments(cell(attachments)),
            })
        })
        .collect())
}

/// RFC 4180 CSV: quoted fields may contain commas, newlines and `""`.
pub(crate) fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') if chars.peek() == Some(&'\n') => {}
            (false, '\n' | '\r') => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            (false, c) => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn file(name: &str, content: &str) -> (String, Vec<u8>) {
        (name.to_owned(), content.as_bytes().to_vec())
    }

    fn old_layout() -> Vec<(String, Vec<u8>)> {
        vec![
            file(
                "messages/index.json",
                r#"{"100": "Direct Message with bob#0", "200": "general in Rust", "300": null, "400": "Unknown channel"}"#,
            ),
            file("messages/c100/channel.json", r#"{"id": "100", "type": 1, "recipients": ["1", "2"]}"#),
            file(
                "messages/c100/messages.csv",
                "ID,Timestamp,Contents,Attachments\r\n5,2024-01-01 10:00:00,hi,\r\n7,2024-01-02 10:00:00,\"multi\nline, \"\"quoted\"\"\",https://cdn.discordapp.com/attachments/1/2/cat.png?ex=1 https://x/y.txt\r\n",
            ),
            file(
                "messages/c200/channel.json",
                r#"{"id": "200", "type": 0, "name": "general", "guild": {"id": "900", "name": "Rust"}}"#,
            ),
            file("messages/c200/messages.csv", "ID,Timestamp,Contents,Attachments\n9,x,hello,\n"),
            file(
                "messages/c201/channel.json",
                r#"{"id": "201", "type": 0, "name": "memes", "guild": {"id": "900", "name": "Rust"}}"#,
            ),
            file("messages/c201/messages.csv", "ID,Timestamp,Contents,Attachments\n3,x,a,\n4,x,b,\n"),
            // A channel whose server the package does not know.
            file("messages/c400/channel.json", r#"{"id": "400", "type": 0}"#),
            file("messages/c400/messages.csv", "ID,Timestamp,Contents,Attachments\n1,x,old,\n"),
            // No messages: left out.
            file("messages/c300/channel.json", r#"{"id": "300", "type": 3}"#),
            file("messages/c300/messages.csv", "ID,Timestamp,Contents,Attachments\n"),
            file("account/user.json", "{}"),
        ]
    }

    #[test]
    fn reads_the_csv_layout() {
        let package = Package::from_files(old_layout()).unwrap();
        assert_eq!(package.message_count(), 6);

        let dm = package.channels.iter().find(|c| c.id.0 == 100).unwrap();
        assert_eq!((dm.kind, dm.name.as_str()), (TargetKind::Dm, "bob"));
        let ids: Vec<u64> = dm.messages.iter().map(|m| m.id.0).collect();
        assert_eq!(ids, [7, 5]);
        assert_eq!(dm.messages[0].content, "multi\nline, \"quoted\"");
        assert_eq!(dm.messages[0].attachments.len(), 2);
        let message = dm.messages[0].to_message(dm.id, Snowflake(1));
        assert!(crate::Has::Image.found_in(&message));

        let targets = package.targets();
        let summary: Vec<(TargetKind, u64, &str, u64, usize)> = targets
            .iter()
            .map(|t| {
                (
                    t.target.kind,
                    t.target.id.0,
                    t.target.name.as_str(),
                    t.messages,
                    t.channels.len(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (
                    TargetKind::Guild,
                    400,
                    "#Unknown channel (unknown server)",
                    1,
                    1
                ),
                (TargetKind::Guild, 900, "Rust", 3, 2),
                (TargetKind::Dm, 100, "bob", 2, 1),
            ]
        );
        // Channels with most messages first.
        assert_eq!(targets[1].channels[0].name, "memes");

        let rust = &targets[1].target;
        assert_eq!(package.channels_of(rust).count(), 2);
        let only_general = Target {
            channels: vec![Snowflake(200)],
            ..rust.clone()
        };
        let ids: Vec<u64> = package.channels_of(&only_general).map(|c| c.id.0).collect();
        assert_eq!(ids, [200]);
    }

    #[test]
    fn reads_the_json_layout_below_an_outer_folder() {
        let package = Package::from_files(vec![
            file("package/Messages/index.json", r#"{"55": "Direct Message with Ann"}"#),
            file("package/Messages/c55/channel.json", r#"{"id": "55", "type": "DM"}"#),
            file(
                "package/Messages/c55/messages.json",
                r#"[{"ID": 1234567890123456789, "Timestamp": "2024-01-01 10:00:00", "Contents": "hey", "Attachments": ""},
                    {"ID": "12", "Contents": "x", "Attachments": "https://a/b.mp4"},
                    {"Timestamp": "no id"}]"#,
            ),
        ])
        .unwrap();
        let channel = &package.channels[0];
        assert_eq!(
            (channel.kind, channel.name.as_str()),
            (TargetKind::Dm, "Ann")
        );
        let ids: Vec<u64> = channel.messages.iter().map(|m| m.id.0).collect();
        assert_eq!(ids, [1234567890123456789, 12]);
    }

    #[test]
    fn knows_its_owner() {
        let mut files = old_layout();
        files.push(file(
            "package/Account/user.json",
            r#"{"id": "42", "username": "me"}"#,
        ));
        let package = Package::from_files(files).unwrap();
        assert_eq!(package.owner, Some(Snowflake(42)));
        assert!(package.check_owner(Snowflake(42)).is_ok());
        assert!(package
            .check_owner(Snowflake(7))
            .unwrap_err()
            .to_string()
            .contains("another account"));
        // Without user.json there is nothing to compare.
        assert!(Package::from_files(old_layout())
            .unwrap()
            .check_owner(Snowflake(7))
            .is_ok());
    }

    #[test]
    fn open_conversations_lend_their_names() {
        let package = Package::from_files(vec![
            file(
                "messages/index.json",
                r#"{"300": null, "100": "Direct Message with bob#0"}"#,
            ),
            file(
                "messages/c300/channel.json",
                r#"{"id": "300", "type": 3, "recipients": ["1", "2", "3"]}"#,
            ),
            file(
                "messages/c300/messages.csv",
                "ID,Timestamp,Contents,Attachments\n5,x,hi,\n",
            ),
            file("messages/c100/channel.json", r#"{"id": "100", "type": 1}"#),
            file(
                "messages/c100/messages.csv",
                "ID,Timestamp,Contents,Attachments\n6,x,yo,\n",
            ),
            // A group DM that is no longer open keeps its package name.
            file("messages/c400/channel.json", r#"{"id": "400", "type": 3}"#),
            file(
                "messages/c400/messages.csv",
                "ID,Timestamp,Contents,Attachments\n7,x,old,\n",
            ),
        ])
        .unwrap();
        let mut targets = package.targets();
        let name = |targets: &[PackageTarget], id: u64| {
            targets
                .iter()
                .find(|t| t.target.id.0 == id)
                .unwrap()
                .target
                .name
                .clone()
        };
        assert_eq!(name(&targets, 300), "Unnamed group");

        let live = |id: u64, name: &str, icon: Option<&str>| Target {
            kind: TargetKind::Dm,
            id: Snowflake(id),
            name: name.into(),
            icon_url: icon.map(str::to_owned),
            channels: Vec::new(),
        };
        apply_live_names(
            &mut targets,
            &[
                live(300, "Ann, Bob, Cy", None),
                live(100, "Bob", Some("https://cdn/bob.png")),
            ],
        );

        assert_eq!(name(&targets, 300), "Ann, Bob, Cy");
        assert_eq!(name(&targets, 100), "Bob");
        assert_eq!(name(&targets, 400), "Unnamed group");
        let bob = targets.iter().find(|t| t.target.id.0 == 100).unwrap();
        assert_eq!(bob.target.icon_url.as_deref(), Some("https://cdn/bob.png"));
    }

    #[test]
    fn rejects_other_files() {
        let err = Package::from_files(vec![file("account/user.json", "{}")]).unwrap_err();
        assert!(err.to_string().contains("data package"));
    }

    #[test]
    fn opens_zip_files_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("package.zip");
        let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in old_layout() {
            zip.start_file(&name, options).unwrap();
            zip.write_all(&data).unwrap();
        }
        zip.finish().unwrap();
        assert_eq!(Package::open(&zip_path).unwrap().message_count(), 6);

        let folder = dir.path().join("extracted");
        for (name, data) in old_layout() {
            let path = folder.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, data).unwrap();
        }
        assert_eq!(Package::open(&folder).unwrap().message_count(), 6);

        let not_zip = dir.path().join("notes.txt");
        std::fs::write(&not_zip, "hello").unwrap();
        assert!(Package::open(&not_zip).is_err());
    }

    #[test]
    fn csv_edge_cases() {
        assert_eq!(
            parse_csv("a,b\n\"x\"\"y\",\"\"\n"),
            [vec!["a", "b"], vec!["x\"y", ""]]
        );
        assert_eq!(parse_csv("a,b"), [vec!["a", "b"]]);
        assert!(parse_csv("").is_empty());
    }
}
