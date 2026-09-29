//! A copy of a data package with every value replaced, so EraseCord can be
//! tested with realistic data without anyone handing over their messages.
//!
//! What stays: the files and field names, the number of messages per
//! conversation, which messages have links, mentions or attachments, weekday
//! and time of day. What changes: every text becomes random words, every
//! name a placeholder, every ID and link a new one (the same one wherever it
//! appears), other numbers random ones, and all dates move back by the same
//! random number of weeks. Files other than JSON and CSV are left out.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::sync::LazyLock;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use rand::rngs::{StdRng, SysRng};
use rand::{Rng, RngExt, SeedableRng};
use regex::Regex;
use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::inspect::{
    extension, is_enum, is_field_name, is_id, is_timestamp, walk, MAP_KEYS, MAX_WHOLE_FILE,
};
use crate::package::parse_csv;
use crate::snowflake::{Snowflake, DISCORD_EPOCH_MS};

/// What was written.
#[derive(Debug, Default)]
pub struct Summary {
    pub files: u64,
    pub records: u64,
    /// Files left out, by kind (`png`, `html`, `too large`, …).
    pub omitted: BTreeMap<String, u64>,
}

/// Writes an anonymized copy of the package at `input` (`.zip` or folder)
/// to the new `.zip` file `output`.
pub fn anonymize(input: &Path, output: &Path) -> Result<Summary> {
    let rng = StdRng::try_from_rng(&mut SysRng)
        .map_err(|err| Error::Package(format!("no random numbers: {err}")))?;
    anonymize_with(input, output, rng)
}

pub(crate) fn anonymize_with<R: Rng>(input: &Path, output: &Path, rng: R) -> Result<Summary> {
    if output.exists() {
        return Err(Error::Package(format!(
            "{} exists already; choose a new file",
            output.display()
        )));
    }
    let partial = output.with_extension("zip.partial");
    let result = write_copy(input, &partial, Anonymizer::new(rng));
    match result {
        Ok(summary) => {
            std::fs::rename(&partial, output)
                .map_err(|err| Error::Package(format!("{}: {err}", output.display())))?;
            Ok(summary)
        }
        Err(err) => {
            let _ = std::fs::remove_file(&partial);
            Err(err)
        }
    }
}

fn write_copy<R: Rng>(input: &Path, output: &Path, mut anon: Anonymizer<R>) -> Result<Summary> {
    let io = |err: std::io::Error| Error::Package(format!("{}: {err}", output.display()));
    let file = File::create(output).map_err(io)?;
    let mut zip = zip::ZipWriter::new(BufWriter::new(file));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let mut summary = Summary::default();
    walk(input, |name, size, reader| {
        let ext = extension(name);
        if ext != "json" && ext != "csv" {
            let kind = if ext.is_empty() {
                "other"
            } else {
                ext.as_str()
            };
            *summary.omitted.entry(kind.to_owned()).or_default() += 1;
            return Ok(());
        }
        let is_index = name
            .to_ascii_lowercase()
            .trim_end_matches('/')
            .ends_with("messages/index.json");
        let mut out = Vec::new();
        let records = if ext == "csv" {
            anon.csv(size, reader, &mut out)?
        } else {
            anon.json(size, reader, &mut out, is_index)?
        };
        let Some(records) = records else {
            *summary
                .omitted
                .entry("unreadable or too large".into())
                .or_default() += 1;
            return Ok(());
        };
        zip.start_file(anon.path(name), options)
            .map_err(std::io::Error::other)?;
        zip.write_all(&out)?;
        summary.files += 1;
        summary.records += records;
        Ok(())
    })?;
    zip.finish()
        .map_err(|err| Error::Package(err.to_string()))?
        .flush()
        .map_err(io)?;
    Ok(summary)
}

/// Keys whose values are Discord's own names (`DM`, `guild_viewed`) and stay.
const ENUM_KEYS: [&str; 3] = ["type", "event_type", "channel_type"];
/// Keys whose values are names: replaced by the same placeholder everywhere.
const NAME_KEYS: [&str; 7] = [
    "name",
    "username",
    "global_name",
    "nickname",
    "nick",
    "display_name",
    "owner",
];
/// Keys whose values are written text.
const TEXT_KEYS: [&str; 11] = [
    "content",
    "contents",
    "message",
    "text",
    "body",
    "description",
    "bio",
    "about_me",
    "note",
    "topic",
    "attachments",
];
/// Link targets that say nothing personal; their paths are still replaced.
const PUBLIC_HOSTS: [&str; 14] = [
    "tenor.com",
    "giphy.com",
    "media.giphy.com",
    "youtube.com",
    "www.youtube.com",
    "youtu.be",
    "twitter.com",
    "x.com",
    "github.com",
    "www.reddit.com",
    "reddit.com",
    "imgur.com",
    "open.spotify.com",
    "www.twitch.tv",
];
const FILE_HOSTS: [&str; 3] = [
    "cdn.discordapp.com",
    "media.discordapp.net",
    "cdn.discord.com",
];
const FILE_TYPES: [&str; 14] = [
    "png", "jpg", "jpeg", "gif", "webp", "mp4", "mov", "webm", "mp3", "ogg", "wav", "pdf", "txt",
    "zip",
];

static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
          (?P<url>https?://[^\s<>]+)
        | <(?P<kind>@[!&]?|\#)(?P<mention>\d{15,21})>
        | <(?P<anim>a?):(?P<emoji>\w{1,32}):(?P<emoji_id>\d{15,21})>
        | (?P<word>[\p{L}\p{N}_'’]+)",
    )
    .expect("valid pattern")
});

struct Anonymizer<R> {
    rng: R,
    shift: Duration,
    ids: HashMap<u64, u64>,
    used: HashSet<u64>,
    names: HashMap<String, String>,
    folders: HashMap<String, String>,
    keys: HashMap<String, String>,
    hosts: HashMap<String, usize>,
    emoji: HashMap<String, usize>,
    files: usize,
    words: Vec<String>,
}

impl<R: Rng> Anonymizer<R> {
    fn new(mut rng: R) -> Self {
        let weeks = rng.random_range(4..=104);
        Anonymizer {
            rng,
            shift: Duration::weeks(weeks),
            ids: HashMap::new(),
            used: HashSet::new(),
            names: HashMap::new(),
            folders: HashMap::new(),
            keys: HashMap::new(),
            hosts: HashMap::new(),
            emoji: HashMap::new(),
            files: 0,
            words: vocabulary(4000),
        }
    }

    /// The same new ID for the same old one, moved back in time like dates.
    fn id(&mut self, old: u64) -> u64 {
        if let Some(&new) = self.ids.get(&old) {
            return new;
        }
        let first = Utc.timestamp_millis_opt(DISCORD_EPOCH_MS).unwrap() + Duration::days(1);
        let latest = Utc::now() + Duration::days(365);
        let created = Snowflake(old).created_at();
        let time = if created > first && created < latest {
            (created - self.shift).max(first)
        } else {
            let span = (Utc::now() - first).num_milliseconds().max(1);
            first + Duration::milliseconds(self.rng.random_range(0..span))
        };
        let base = Snowflake::from_datetime(time).0;
        let new = loop {
            let candidate = base | self.rng.random_range(0..1u64 << 22);
            if self.used.insert(candidate) {
                break candidate;
            }
        };
        self.ids.insert(old, new);
        new
    }

    fn timestamp(&self, s: &str) -> Option<String> {
        if let Ok(time) = DateTime::parse_from_rfc3339(s) {
            return Some((time - self.shift).to_rfc3339_opts(SecondsFormat::AutoSi, false));
        }
        if let Ok(time) = DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%:z") {
            return Some(
                (time - self.shift)
                    .format("%Y-%m-%d %H:%M:%S%.6f%:z")
                    .to_string(),
            );
        }
        if let Ok(time) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
            return Some((time - self.shift).format("%Y-%m-%d %H:%M:%S").to_string());
        }
        if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            return Some((date - self.shift).format("%Y-%m-%d").to_string());
        }
        None
    }

    fn string(&mut self, key: &str, s: &str) -> String {
        let t = s.trim();
        let key = key.to_ascii_lowercase();
        if t.is_empty() {
            return s.to_owned();
        }
        if is_id(t) {
            if let Ok(id) = t.parse() {
                return self.id(id).to_string();
            }
        }
        if is_timestamp(t) {
            if let Some(shifted) = self.timestamp(t) {
                return shifted;
            }
        }
        if ENUM_KEYS.contains(&key.as_str()) && is_enum(t) {
            return t.to_owned();
        }
        if t.bytes().all(|b| b.is_ascii_digit()) {
            return self.digits(t.len());
        }
        if NAME_KEYS.contains(&key.as_str()) {
            return self.name(t);
        }
        if TEXT_KEYS.contains(&key.as_str())
            || t.contains(char::is_whitespace)
            || t.chars().count() > 40
            || t.starts_with("http")
        {
            return self.text(s);
        }
        self.name(t)
    }

    /// `Direct Message with bob#0` and `general in Server`, as in
    /// `messages/index.json`, keeping their shape.
    fn label(&mut self, s: &str) -> String {
        if let Some(who) = s.strip_prefix("Direct Message with ") {
            return format!("Direct Message with {}", self.name(who));
        }
        if let Some((channel, server)) = s.rsplit_once(" in ") {
            return format!("{} in {}", self.name(channel), self.name(server));
        }
        self.name(s)
    }

    fn name(&mut self, s: &str) -> String {
        let next = self.names.len() + 1;
        self.names
            .entry(s.to_owned())
            .or_insert_with(|| format!("name{next}"))
            .clone()
    }

    fn digits(&mut self, len: usize) -> String {
        (0..len)
            .map(|_| char::from(b'0' + self.rng.random_range(0..10u8)))
            .collect()
    }

    /// Words become random words; links, mentions and custom emoji keep
    /// their shape with new targets; punctuation and emoji stay.
    fn text(&mut self, s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut last = 0;
        for caps in TOKENS.captures_iter(s) {
            let whole = caps.get(0).expect("match");
            out.push_str(&s[last..whole.start()]);
            last = whole.end();
            if let Some(url) = caps.name("url") {
                let url = self.url(url.as_str());
                out.push_str(&url);
            } else if let (Some(kind), Some(id)) = (caps.name("kind"), caps.name("mention")) {
                let id = self.id(id.as_str().parse().unwrap_or(0));
                out.push_str(&format!("<{}{id}>", kind.as_str()));
            } else if let (Some(name), Some(id)) = (caps.name("emoji"), caps.name("emoji_id")) {
                let next = self.emoji.len() + 1;
                let number = *self.emoji.entry(name.as_str().to_owned()).or_insert(next);
                let id = self.id(id.as_str().parse().unwrap_or(0));
                let animated = caps.name("anim").map_or("", |a| a.as_str());
                out.push_str(&format!("<{animated}:emoji{number}:{id}>"));
            } else if let Some(word) = caps.name("word") {
                let word = word.as_str();
                if word.chars().all(|c| c.is_ascii_digit()) {
                    let digits = self.digits(word.len());
                    out.push_str(&digits);
                } else {
                    let new = self.word();
                    if word.starts_with(char::is_uppercase) {
                        let mut chars = new.chars();
                        let first = chars.next().expect("word").to_ascii_uppercase();
                        out.push(first);
                        out.push_str(chars.as_str());
                    } else {
                        out.push_str(&new);
                    }
                }
            }
        }
        out.push_str(&s[last..]);
        out
    }

    /// A random word, common ones more often, roughly like real language.
    /// Drawn anew each time, so word frequencies reveal nothing.
    fn word(&mut self) -> String {
        let n = self.words.len() as f64;
        let u: f64 = self.rng.random();
        let index = ((u * (n + 1.0).ln()).exp() - 1.0).floor() as usize;
        self.words[index.min(self.words.len() - 1)].clone()
    }

    fn url(&mut self, url: &str) -> String {
        let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = host.to_ascii_lowercase();
        let path = path.split(['?', '#']).next().unwrap_or_default();
        if FILE_HOSTS.contains(&host.as_str()) {
            let ext = path
                .rsplit_once('.')
                .map(|(_, ext)| ext.to_ascii_lowercase())
                .filter(|ext| FILE_TYPES.contains(&ext.as_str()))
                .unwrap_or_else(|| "bin".into());
            let mut ids = path
                .split('/')
                .filter_map(|part| part.parse::<u64>().ok())
                .collect::<Vec<_>>()
                .into_iter();
            let channel = ids.next().unwrap_or_default();
            let attachment = ids.next().unwrap_or_default();
            let (channel, attachment) = (self.id(channel), self.id(attachment));
            self.files += 1;
            return format!(
                "https://cdn.discordapp.com/attachments/{channel}/{attachment}/file{}.{ext}",
                self.files
            );
        }
        let page = self.rng.random_range(1..100_000);
        if PUBLIC_HOSTS.contains(&host.as_str()) {
            return format!("https://{host}/page{page}");
        }
        let next = self.hosts.len() + 1;
        let site = *self.hosts.entry(host).or_insert(next);
        format!("https://site{site}.example/page{page}")
    }

    fn key(&mut self, key: &str, lookup: bool) -> String {
        if is_id(key) {
            if let Ok(id) = key.parse() {
                return self.id(id).to_string();
            }
        }
        if key.bytes().all(|b| b.is_ascii_digit()) || (!lookup && is_field_name(key)) {
            return key.to_owned();
        }
        let next = self.keys.len() + 1;
        self.keys
            .entry(key.to_owned())
            .or_insert_with(|| format!("key{next}"))
            .clone()
    }

    fn value(&mut self, value: &Value, key: &str, label: bool) -> Value {
        match value {
            Value::String(s) if label => Value::String(self.label(s)),
            Value::String(s) => Value::String(self.string(key, s)),
            Value::Number(n) => {
                if let Some(u) = n.as_u64() {
                    if u >= 100_000_000_000_000 {
                        Value::from(self.id(u))
                    } else if u <= 1000 {
                        value.clone()
                    } else {
                        let digits = self.digits(u.to_string().len());
                        Value::from(digits.parse::<u64>().unwrap_or(0).max(1001))
                    }
                } else if n.as_i64().is_some_and(|i| i >= -1000) {
                    value.clone()
                } else if n.is_i64() {
                    Value::from(-(self.rng.random_range(1001..1_000_000i64)))
                } else {
                    let scale = n.as_f64().unwrap_or(1.0).abs().max(1.0);
                    let random: f64 = self.rng.random();
                    Value::from((random * scale * 100.0).round() / 100.0)
                }
            }
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|item| self.value(item, key, label))
                    .collect(),
            ),
            Value::Object(map) => {
                let lookup = map.len() > MAP_KEYS;
                let mut out = Map::new();
                for (k, v) in map {
                    let new_key = self.key(k, lookup);
                    // Labels of `messages/index.json` sit right below the top.
                    let item = self.value(v, if lookup { "" } else { k }, label);
                    out.insert(new_key, item);
                }
                Value::Object(out)
            }
            Value::Null | Value::Bool(_) => value.clone(),
        }
    }

    /// JSON or JSON lines; `None` if it cannot be read.
    fn json(
        &mut self,
        size: u64,
        reader: &mut dyn Read,
        out: &mut Vec<u8>,
        index: bool,
    ) -> std::io::Result<Option<u64>> {
        let mut reader = BufReader::new(reader);
        let mut line = Vec::new();
        reader.read_until(b'\n', &mut line)?;
        if let Ok(first @ Value::Object(_)) = serde_json::from_slice::<Value>(&line) {
            let mut records = 0;
            let mut value = Some(first);
            loop {
                if let Some(v) = value.take() {
                    serde_json::to_writer(&mut *out, &self.value(&v, "", false))?;
                    out.push(b'\n');
                    records += 1;
                }
                line.clear();
                if reader.read_until(b'\n', &mut line)? == 0 {
                    break;
                }
                value = serde_json::from_slice(&line).ok();
            }
            return Ok(Some(records));
        }
        if size > MAX_WHOLE_FILE {
            return Ok(None);
        }
        let mut data = line;
        reader.read_to_end(&mut data)?;
        let Ok(value) = serde_json::from_slice::<Value>(&data) else {
            return Ok(None);
        };
        let records = match &value {
            Value::Array(items) => items.len() as u64,
            _ => 1,
        };
        let new = match (&value, index) {
            // index.json: `{ "<channel id>": "general in Server" }`.
            (Value::Object(map), true) => Value::Object(
                map.iter()
                    .map(|(k, v)| (self.key(k, true), self.value(v, "", true)))
                    .collect(),
            ),
            _ => self.value(&value, "", false),
        };
        serde_json::to_writer(&mut *out, &new)?;
        Ok(Some(records))
    }

    fn csv(
        &mut self,
        size: u64,
        reader: &mut dyn Read,
        out: &mut Vec<u8>,
    ) -> std::io::Result<Option<u64>> {
        if size > MAX_WHOLE_FILE {
            return Ok(None);
        }
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        let text = String::from_utf8_lossy(&data);
        let mut rows = parse_csv(text.strip_prefix('\u{feff}').unwrap_or(&text)).into_iter();
        let Some(header) = rows.next() else {
            return Ok(Some(0));
        };
        let header: Vec<String> = header.iter().map(|h| self.key(h.trim(), false)).collect();
        write_csv_row(out, &header);
        let mut records = 0;
        for row in rows {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, cell)| self.string(header.get(i).map_or("", String::as_str), cell))
                .collect();
            write_csv_row(out, &cells);
            records += 1;
        }
        Ok(Some(records))
    }

    /// IDs in folder names are replaced like everywhere else; parts that are
    /// not plain words become placeholders, and so do `.` and `..`, so the
    /// copy never unpacks outside its folder.
    fn path(&mut self, name: &str) -> String {
        name.split(['/', '\\'])
            .filter(|p| !p.is_empty())
            .map(|part| {
                let replaced = self.ids_in(part);
                let plain = replaced.len() <= 80
                    && replaced.chars().any(|c| c != '.')
                    && replaced
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
                if plain {
                    replaced
                } else {
                    let ext = extension(part);
                    let next = self.folders.len() + 1;
                    let folder = self
                        .folders
                        .entry(part.to_owned())
                        .or_insert_with(|| format!("folder{next}"))
                        .clone();
                    if ext.is_empty() {
                        folder
                    } else {
                        format!("{folder}.{ext}")
                    }
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    fn ids_in(&mut self, part: &str) -> String {
        let mut out = String::new();
        let mut digits = String::new();
        for c in part.chars().chain(std::iter::once('/')) {
            if c.is_ascii_digit() {
                digits.push(c);
                continue;
            }
            if is_id(&digits) {
                let id = self.id(digits.parse().unwrap_or(0));
                out.push_str(&id.to_string());
            } else {
                out.push_str(&digits);
            }
            digits.clear();
            if c != '/' {
                out.push(c);
            }
        }
        out
    }
}

fn write_csv_row(out: &mut Vec<u8>, cells: &[String]) {
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        if cell.contains([',', '"', '\n', '\r']) {
            out.push(b'"');
            out.extend_from_slice(cell.replace('"', "\"\"").as_bytes());
            out.push(b'"');
        } else {
            out.extend_from_slice(cell.as_bytes());
        }
    }
    out.push(b'\n');
}

/// Made-up words from syllables, shortest first.
fn vocabulary(size: usize) -> Vec<String> {
    const CONSONANTS: &[u8] = b"bdfgklmnprstvz";
    const VOWELS: &[u8] = b"aeiou";
    let syllables: Vec<String> = CONSONANTS
        .iter()
        .flat_map(|c| {
            VOWELS
                .iter()
                .map(move |v| String::from_utf8(vec![*c, *v]).expect("ascii"))
        })
        .collect();
    let mut words = syllables.clone();
    let mut length = 1;
    while words.len() < size {
        let previous: Vec<String> = words
            .iter()
            .filter(|w| w.len() == length * 2)
            .cloned()
            .collect();
        for word in previous {
            for syllable in &syllables {
                words.push(format!("{word}{syllable}"));
                if words.len() >= size {
                    return words;
                }
            }
        }
        length += 1;
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::tests::{sample_package, SECRETS};
    use crate::package::Package;
    use crate::targets::TargetKind;

    fn anonymized(dir: &Path) -> (std::path::PathBuf, Summary) {
        let input = sample_package(dir);
        let output = dir.join("anonymized.zip");
        let summary = anonymize_with(&input, &output, StdRng::seed_from_u64(7)).unwrap();
        (output, summary)
    }

    #[test]
    fn nothing_personal_is_left() {
        let dir = tempfile::tempdir().unwrap();
        let (output, summary) = anonymized(dir.path());
        assert_eq!(summary.omitted["png"], 1);
        let mut seen = 0;
        walk(&output, |name, _, reader| {
            let mut content = String::new();
            reader.read_to_string(&mut content)?;
            for secret in SECRETS {
                assert!(!name.contains(secret), "{secret} in {name}");
                assert!(!content.contains(secret), "{secret} in {name}: {content}");
            }
            seen += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, summary.files);
    }

    #[test]
    fn the_copy_still_works_as_a_package() {
        let dir = tempfile::tempdir().unwrap();
        let original = Package::open(&sample_package(dir.path())).unwrap();
        let (output, _) = anonymized(dir.path());
        let copy = Package::open(&output).unwrap();
        assert_eq!(copy.message_count(), original.message_count());
        let kinds = |p: &Package| {
            let mut kinds: Vec<TargetKind> = p.targets().iter().map(|t| t.target.kind).collect();
            kinds.sort_by_key(|k| format!("{k:?}"));
            kinds
        };
        assert_eq!(kinds(&copy), kinds(&original));
        assert!(copy.owner.is_some() && copy.owner != original.owner);

        // Every date moved back by the same whole number of weeks, so
        // weekdays and times of day stay.
        let times = |p: &Package| {
            let mut times: Vec<i64> = p
                .channels
                .iter()
                .flat_map(|c| &c.messages)
                .map(|m| m.id.timestamp_ms())
                .collect();
            times.sort();
            times
        };
        let week = 7 * 24 * 3600 * 1000;
        let shifts: Vec<i64> = times(&original)
            .iter()
            .zip(times(&copy))
            .map(|(old, new)| old - new)
            .collect();
        assert!(shifts.iter().all(|s| s % week == 0 && *s >= 4 * week));
        assert!(shifts.windows(2).all(|w| w[0] == w[1]));
    }

    #[test]
    fn text_keeps_its_shape() {
        let mut anon = Anonymizer::new(StdRng::seed_from_u64(1));
        let text = anon.text(
            "Hi <@222222222222222222>, look: https://cdn.discordapp.com/attachments/1/2/cat.PNG?ex=1 <:blob:333333333333333333> 😂 42!",
        );
        assert!(text.starts_with(char::is_uppercase), "{text}");
        assert!(text.contains("<@") && text.contains("😂") && text.contains('!'));
        assert!(text.contains(".png") && text.contains("<:emoji1:"));
        assert!(!text.contains("222222222222222222") && !text.contains("blob"));
        // The same person gets the same new ID each time.
        let a = anon.text("<@222222222222222222>");
        let b = anon.text("<@222222222222222222>");
        assert_eq!(a, b);
        assert_eq!(anon.label("general in Rust"), "name1 in name2");
    }

    #[test]
    fn paths_stay_inside_the_copy() {
        let mut anon = Anonymizer::new(StdRng::seed_from_u64(1));
        let path = anon.path("../../../.bashrc/./messages/index.json");
        assert!(path.split('/').all(|p| p != ".." && p != "."), "{path}");
        assert!(path.ends_with("/messages/index.json"), "{path}");
    }

    #[test]
    fn output_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (output, _) = anonymized(dir.path());
        let again = anonymize(&sample_package(dir.path()), &output);
        assert!(again.is_err());
    }
}
