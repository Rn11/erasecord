//! A look inside a data package that reveals nothing personal: which files
//! it has, which fields they contain, how many records and what kinds of
//! values, but never a value itself. Meant for bug reports and for
//! supporting new package formats without seeing anyone's data.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::package::parse_csv;

/// Larger files that are not JSON lines are not read into memory.
pub(crate) const MAX_WHOLE_FILE: u64 = 512 << 20;
/// Fields listed per group of files; the rest is only counted.
const MAX_FIELDS: usize = 300;
/// An object with more keys than this is a lookup table keyed by IDs or
/// names, not a record: its keys are not listed.
pub(crate) const MAP_KEYS: usize = 50;

/// The structure of a data package.
#[derive(Debug, Default, Serialize)]
pub struct Report {
    /// By path, with IDs, numbers and unusual names replaced by placeholders,
    /// so `messages/c123/messages.json` and `messages/c456/…` form one group.
    pub groups: BTreeMap<String, FileGroup>,
}

#[derive(Debug, Default, Serialize)]
pub struct FileGroup {
    pub files: u64,
    pub bytes: u64,
    /// `json`, `json lines`, `csv`, or the kind of other files (`png`, …).
    pub formats: BTreeSet<&'static str>,
    /// Items of a JSON array, lines of JSON lines, rows of a CSV file.
    pub records: u64,
    /// Files or lines that could not be read.
    pub unreadable: u64,
    /// By field path, e.g. `[].Contents` or `.guild.name`.
    pub fields: BTreeMap<String, Field>,
    /// Fields beyond the listed ones.
    pub more_fields: u64,
    /// Discord's names of activity events, e.g. `guild_viewed`.
    pub event_types: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Serialize)]
pub struct Field {
    pub count: u64,
    /// `id`, `timestamp`, `url`, `text`, `empty`, `number`, `bool`, `null`,
    /// `object` or `array`.
    pub kinds: BTreeSet<&'static str>,
}

/// Reads every file of the package (`.zip` or folder) and describes it.
pub fn inspect(path: &Path) -> Result<Report> {
    let mut report = Report::default();
    walk(path, |name, size, reader| {
        let group = report.groups.entry(path_pattern(name)).or_default();
        group.files += 1;
        group.bytes += size;
        match extension(name).as_str() {
            "json" => inspect_json(group, size, reader),
            "csv" => inspect_csv(group, size, reader),
            other => {
                group.formats.insert(other_format(other));
                Ok(())
            }
        }
    })?;
    Ok(report)
}

fn inspect_json(group: &mut FileGroup, size: u64, reader: &mut dyn Read) -> std::io::Result<()> {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line)?;
    // One object per line ("JSON lines"), as in the activity files: read
    // line by line, however large the file.
    if let Ok(first @ Value::Object(_)) = serde_json::from_slice::<Value>(&line) {
        record(group, &first, "");
        let mut lines = 1;
        loop {
            line.clear();
            if reader.read_until(b'\n', &mut line)? == 0 {
                break;
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<Value>(&line) {
                Ok(value) => {
                    record(group, &value, "");
                    lines += 1;
                }
                Err(_) => group.unreadable += 1,
            }
        }
        group
            .formats
            .insert(if lines > 1 { "json lines" } else { "json" });
        return Ok(());
    }
    group.formats.insert("json");
    if size > MAX_WHOLE_FILE {
        group.unreadable += 1;
        return Ok(());
    }
    let mut data = line;
    reader.read_to_end(&mut data)?;
    match serde_json::from_slice::<Value>(&data) {
        Ok(Value::Array(items)) => {
            for item in &items {
                record(group, item, "[]");
            }
        }
        Ok(value) => record(group, &value, ""),
        Err(_) => group.unreadable += 1,
    }
    Ok(())
}

fn inspect_csv(group: &mut FileGroup, size: u64, reader: &mut dyn Read) -> std::io::Result<()> {
    group.formats.insert("csv");
    if size > MAX_WHOLE_FILE {
        group.unreadable += 1;
        return Ok(());
    }
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;
    let text = String::from_utf8_lossy(&data);
    let mut rows = parse_csv(text.strip_prefix('\u{feff}').unwrap_or(&text)).into_iter();
    let Some(header) = rows.next() else {
        return Ok(());
    };
    let columns: Vec<String> = header
        .iter()
        .map(|h| format!(".{}", key_pattern(h.trim(), false)))
        .collect();
    for row in rows {
        group.records += 1;
        for (column, cell) in columns.iter().zip(&row) {
            add_field(group, column, string_kind(cell));
        }
    }
    Ok(())
}

fn record(group: &mut FileGroup, value: &Value, root: &str) {
    group.records += 1;
    if let Some(Value::String(event)) = value.get("event_type") {
        if is_enum(event) {
            *group.event_types.entry(event.clone()).or_default() += 1;
        }
    }
    let mut path = root.to_owned();
    visit(group, &mut path, value, 0);
}

fn visit(group: &mut FileGroup, path: &mut String, value: &Value, depth: usize) {
    add_field(group, path, value_kind(value));
    if depth >= 12 {
        return;
    }
    let len = path.len();
    match value {
        Value::Object(map) => {
            let lookup = map.len() > MAP_KEYS;
            for (key, item) in map {
                path.push('.');
                path.push_str(&key_pattern(key, lookup));
                visit(group, path, item, depth + 1);
                path.truncate(len);
            }
        }
        Value::Array(items) => {
            path.push_str("[]");
            for item in items {
                visit(group, path, item, depth + 1);
            }
            path.truncate(len);
        }
        _ => {}
    }
}

fn add_field(group: &mut FileGroup, path: &str, kind: &'static str) {
    let path = if path.is_empty() { "(record)" } else { path };
    if let Some(field) = group.fields.get_mut(path) {
        field.count += 1;
        field.kinds.insert(kind);
    } else if group.fields.len() < MAX_FIELDS {
        group.fields.insert(
            path.to_owned(),
            Field {
                count: 1,
                kinds: BTreeSet::from([kind]),
            },
        );
    } else {
        group.more_fields += 1;
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.as_u64().is_some_and(|n| n >= 100_000_000_000_000) => "id",
        Value::Number(_) => "number",
        Value::String(s) => string_kind(s),
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

pub(crate) fn string_kind(s: &str) -> &'static str {
    let s = s.trim();
    if s.is_empty() {
        "empty"
    } else if is_id(s) {
        "id"
    } else if is_timestamp(s) {
        "timestamp"
    } else if s.starts_with("https://") || s.starts_with("http://") {
        "url"
    } else {
        "text"
    }
}

/// A Discord ID (snowflake) written as text.
pub(crate) fn is_id(s: &str) -> bool {
    (15..=21).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

/// Starts like `2024-01-31`.
pub(crate) fn is_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 10
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
}

/// A name from Discord's own vocabulary, like `guild_viewed` or `DM`.
pub(crate) fn is_enum(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.starts_with(|c: char| c.is_ascii_alphabetic())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A key as it may be shown: field names like `Contents` stay, IDs and
/// anything that could be a name become placeholders.
pub(crate) fn key_pattern(key: &str, lookup: bool) -> String {
    if !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) {
        "<id>".into()
    } else if lookup || !is_field_name(key) {
        "<key>".into()
    } else {
        key.into()
    }
}

pub(crate) fn is_field_name(key: &str) -> bool {
    (1..=40).contains(&key.len())
        && key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        && longest_digit_run(key) < 3
}

fn longest_digit_run(s: &str) -> usize {
    s.split(|c: char| !c.is_ascii_digit())
        .map(str::len)
        .max()
        .unwrap_or(0)
}

/// `messages/c123/messages.json` → `messages/c<n>/messages.json`. Parts
/// that are not plain words become `<name>`.
pub fn path_pattern(path: &str) -> String {
    path.split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .map(|part| {
            let mut out = String::new();
            let mut digits = false;
            for c in part.chars() {
                if c.is_ascii_digit() {
                    if !digits {
                        out.push_str("<n>");
                    }
                    digits = true;
                } else {
                    out.push(c);
                    digits = false;
                }
            }
            let plain = out.len() <= 60
                && out
                    .chars()
                    .all(|c| c.is_ascii_alphabetic() || "<>_.-".contains(c));
            if plain {
                out
            } else {
                "<name>".into()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn extension(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match file.rsplit_once('.') {
        Some((_, ext)) => ext.to_ascii_lowercase(),
        None => String::new(),
    }
}

fn other_format(ext: &str) -> &'static str {
    const KNOWN: [&str; 18] = [
        "png", "jpg", "jpeg", "gif", "webp", "svg", "txt", "html", "htm", "md", "mp4", "webm",
        "mp3", "ogg", "wav", "pdf", "zip", "ics",
    ];
    KNOWN.into_iter().find(|k| *k == ext).unwrap_or("other")
}

/// Calls `visit` with the path inside the package, the size and the
/// contents of every file, from a `.zip` file or a folder.
pub(crate) fn walk(
    path: &Path,
    mut visit: impl FnMut(&str, u64, &mut dyn Read) -> std::io::Result<()>,
) -> Result<()> {
    let io = |err: std::io::Error| Error::Package(format!("{}: {err}", path.display()));
    if path.is_dir() {
        let mut files = Vec::new();
        list_files(path, path, 0, &mut files).map_err(io)?;
        files.sort();
        for (name, full) in files {
            let file = File::open(&full).map_err(io)?;
            let size = file.metadata().map_err(io)?.len();
            visit(&name, size, &mut BufReader::new(file)).map_err(io)?;
        }
    } else {
        let file = File::open(path).map_err(io)?;
        let mut zip = zip::ZipArchive::new(BufReader::new(file))
            .map_err(|err| Error::Package(format!("not a zip file: {err}")))?;
        for index in 0..zip.len() {
            let mut entry = zip
                .by_index(index)
                .map_err(|err| Error::Package(format!("damaged zip file: {err}")))?;
            if !entry.is_file() {
                continue;
            }
            let name = entry.name().to_owned();
            let size = entry.size();
            visit(&name, size, &mut entry).map_err(io)?;
        }
    }
    Ok(())
}

fn list_files(
    root: &Path,
    dir: &Path,
    depth: usize,
    files: &mut Vec<(String, std::path::PathBuf)>,
) -> std::io::Result<()> {
    if depth > 8 {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            list_files(root, &path, depth + 1, files)?;
        } else {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let name = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.push((name, path));
        }
    }
    Ok(())
}

impl Report {
    /// A readable version, safe to share: it contains no values.
    pub fn to_text(&self) -> String {
        let mut out = String::from(
            "Structure of a Discord data package, made by `erasecord inspect-package`.\n\
             It lists file and field names, counts and the kinds of values, but no\n\
             messages, names, IDs, dates or other values.\n",
        );
        for (pattern, group) in &self.groups {
            let formats: Vec<&str> = group.formats.iter().copied().collect();
            let _ = write!(
                out,
                "\n{pattern}\n  {} file{}, {}, {}",
                group.files,
                if group.files == 1 { "" } else { "s" },
                human_size(group.bytes),
                formats.join(" / "),
            );
            if group.records > 0 {
                let _ = write!(out, ", {} records", group.records);
            }
            if group.unreadable > 0 {
                let _ = write!(out, ", {} unreadable", group.unreadable);
            }
            out.push('\n');
            let width = group.fields.keys().map(String::len).max().unwrap_or(0);
            for (path, field) in &group.fields {
                let kinds: Vec<&str> = field.kinds.iter().copied().collect();
                let _ = writeln!(
                    out,
                    "    {path:<width$}  {:>9}  {}",
                    field.count,
                    kinds.join(", ")
                );
            }
            if group.more_fields > 0 {
                let _ = writeln!(
                    out,
                    "    … {} more values in fields not listed",
                    group.more_fields
                );
            }
            if !group.event_types.is_empty() {
                out.push_str("  event types:\n");
                let mut events: Vec<_> = group.event_types.iter().collect();
                events.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
                for (event, count) in events {
                    let _ = writeln!(out, "    {event:<48} {count:>9}");
                }
            }
        }
        out
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64 / 1024.0;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A package in both layouts, with an activity file and an image, full
    /// of values that must never show up in a report.
    pub(crate) fn sample_package(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("package.zip");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, content: &str| {
            zip.start_file(name, options).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        };
        add(
            "account/user.json",
            r#"{"id": "111111111111111111", "username": "secret_alice", "email": "alice@example.org",
                "ip": "203.0.113.9", "relationships": [{"id": "222222222222222222", "nickname": "Bobby Secret"}]}"#,
        );
        add(
            "messages/index.json",
            r#"{"300000000000000001": "Direct Message with secret_bob#0", "300000000000000002": "general in Secret Server"}"#,
        );
        add(
            "messages/c300000000000000001/channel.json",
            r#"{"id": "300000000000000001", "type": 1, "recipients": ["111111111111111111", "222222222222222222"]}"#,
        );
        add(
            "messages/c300000000000000001/messages.csv",
            "ID,Timestamp,Contents,Attachments\n\
             900000000000000001,2024-01-01 10:00:00.000000+00:00,my secret plan <@222222222222222222> https://private.example/x,\n\
             900000000000000002,2024-01-02 11:00:00,\"quoted, secret\",https://cdn.discordapp.com/attachments/1/2/secret.png\n",
        );
        add(
            "Messages/c300000000000000002/channel.json",
            r#"{"id": "300000000000000002", "type": "GUILD_TEXT", "name": "secret-channel",
                "guild": {"id": "400000000000000001", "name": "Secret Server"}}"#,
        );
        add(
            "Messages/c300000000000000002/messages.json",
            r#"[{"ID": 900000000000000003, "Timestamp": "2024-02-03T04:05:06+00:00", "Contents": "ok", "Attachments": ""}]"#,
        );
        add(
            "activity/analytics/events-2024-00000-of-00001.json",
            "{\"event_type\": \"guild_viewed\", \"user_id\": \"111111111111111111\", \"ip\": \"198.51.100.7\"}\n\
             {\"event_type\": \"guild_viewed\", \"os\": \"Secret OS\"}\n\
             {\"event_type\": \"join_voice_channel\", \"channel\": \"300000000000000002\"}\n",
        );
        add("account/avatar.png", "\u{89}PNG secret pixels");
        add(
            "servers/Secret Folder/guild.json",
            r#"{"name": "Secret Server"}"#,
        );
        zip.finish().unwrap();
        path
    }

    pub(crate) const SECRETS: [&str; 12] = [
        "secret",
        "Secret",
        "alice",
        "203.0.113",
        "198.51.100",
        "111111111111111111",
        "222222222222222222",
        "300000000000000001",
        "900000000000000003",
        "2024-01-01",
        "private.example",
        "Bobby",
    ];

    #[test]
    fn reports_the_structure_but_no_values() {
        let dir = tempfile::tempdir().unwrap();
        let report = inspect(&sample_package(dir.path())).unwrap();
        let text = report.to_text();
        let json = serde_json::to_string(&report).unwrap();
        for secret in SECRETS {
            assert!(!text.contains(secret), "{secret} leaked:\n{text}");
            assert!(!json.contains(secret), "{secret} leaked: {json}");
        }

        let csv = &report.groups["messages/c<n>/messages.csv"];
        assert_eq!((csv.files, csv.records), (1, 2));
        assert!(csv.fields[".Contents"].kinds.contains("text"));
        assert!(csv.fields[".ID"].kinds.contains("id"));
        assert!(csv.fields[".Timestamp"].kinds.contains("timestamp"));

        let json_messages = &report.groups["Messages/c<n>/messages.json"];
        assert_eq!(json_messages.records, 1);
        assert!(json_messages.fields["[].ID"].kinds.contains("id"));

        let events = &report.groups["activity/analytics/events-<n>-<n>-of-<n>.json"];
        assert!(events.formats.contains("json lines"));
        assert_eq!(events.records, 3);
        assert_eq!(events.event_types["guild_viewed"], 2);
        assert_eq!(events.event_types["join_voice_channel"], 1);

        assert!(report.groups["messages/index.json"]
            .fields
            .contains_key(".<id>"));
        assert!(report.groups["account/avatar.png"].formats.contains("png"));
        assert!(report.groups.contains_key("servers/<name>/guild.json"));
    }

    #[test]
    fn keys_that_may_be_names_are_hidden() {
        assert_eq!(key_pattern("Contents", false), "Contents");
        assert_eq!(key_pattern("123", false), "<id>");
        assert_eq!(key_pattern("John Smith", false), "<key>");
        assert_eq!(key_pattern("user_12345", false), "<key>");
        assert_eq!(key_pattern("anything", true), "<key>");
        assert_eq!(path_pattern("a/c123/x-2024.json"), "a/c<n>/x-<n>.json");
        assert_eq!(
            path_pattern("servers/Jöns/guild.json"),
            "servers/<name>/guild.json"
        );
    }
}
