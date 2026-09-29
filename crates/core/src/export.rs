//! A record of what a run deleted (or, in a dry run, would delete): one row
//! per message with its text and attachment links, as CSV or JSON. Messages
//! of others whose attachments were saved get a row too, with their author.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::job::Event;
use crate::models::Message;
use crate::snowflake::Snowflake;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// Comma-separated, UTF-8 with a byte order mark so spreadsheet programs
    /// read non-English text correctly.
    Csv,
    /// A JSON array of objects.
    Json,
    /// One JSON object per line; stays readable when cut off.
    JsonLines,
}

impl ExportFormat {
    /// JSON for `.json` files, CSV for anything else.
    pub fn for_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("json") => ExportFormat::Json,
            Some(ext) if ext.eq_ignore_ascii_case("jsonl") => ExportFormat::JsonLines,
            _ => ExportFormat::Csv,
        }
    }
}

/// One exported message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Row<'a> {
    place: &'a str,
    place_id: String,
    channel_id: String,
    message_id: String,
    sent_at: String,
    status: &'static str,
    content: &'a str,
    attachments: &'a [String],
    /// Paths of the backed-up attachments, relative to the backup folder.
    saved_files: &'a [String],
    /// Who sent it, for messages of others; empty for the user's own.
    author: &'a str,
}

const CSV_HEADER: &str =
    "place,place_id,channel_id,message_id,sent_at,status,content,attachments,saved_files,author\r\n";

/// Writes [`Event::Deleted`] and [`Event::SavedFromOthers`] events as they
/// arrive; other events only supply
/// the names of servers and DMs. Call [`ExportWriter::finish`] at the end.
pub struct ExportWriter<W: Write> {
    out: W,
    format: ExportFormat,
    names: HashMap<Snowflake, String>,
    rows: u64,
}

impl<W: Write> ExportWriter<W> {
    pub fn new(mut out: W, format: ExportFormat) -> io::Result<Self> {
        match format {
            ExportFormat::Csv => write!(out, "\u{feff}{CSV_HEADER}")?,
            ExportFormat::Json => write!(out, "[")?,
            ExportFormat::JsonLines => {}
        }
        Ok(ExportWriter {
            out,
            format,
            names: HashMap::new(),
            rows: 0,
        })
    }

    /// Messages written so far.
    pub fn rows(&self) -> u64 {
        self.rows
    }

    pub fn observe(&mut self, event: &Event) -> io::Result<()> {
        match event {
            Event::TargetStarted {
                target_id, name, ..
            } => {
                self.names.insert(*target_id, name.clone());
            }
            Event::Deleted {
                target_id,
                channel_id,
                message_id,
                sent_at,
                content,
                attachments,
                saved,
                dry_run,
                ..
            } => {
                let place = self.names.get(target_id).cloned().unwrap_or_default();
                let row = Row {
                    place: &place,
                    place_id: target_id.to_string(),
                    channel_id: channel_id.to_string(),
                    message_id: message_id.to_string(),
                    sent_at: sent_at.to_rfc3339(),
                    status: if *dry_run { "would_delete" } else { "deleted" },
                    content,
                    attachments,
                    saved_files: saved,
                    author: "",
                };
                self.write_row(&row)?;
                self.rows += 1;
            }
            Event::SavedFromOthers {
                target_id,
                channel_id,
                message_id,
                sent_at,
                author,
                content,
                attachments,
                saved,
            } => {
                let place = self.names.get(target_id).cloned().unwrap_or_default();
                let row = Row {
                    place: &place,
                    place_id: target_id.to_string(),
                    channel_id: channel_id.to_string(),
                    message_id: message_id.to_string(),
                    sent_at: sent_at.to_rfc3339(),
                    status: "saved_from_others",
                    content,
                    attachments,
                    saved_files: saved,
                    author,
                };
                self.write_row(&row)?;
                self.rows += 1;
            }
            _ => {}
        }
        Ok(())
    }

    /// Writes a message found before deleting (status `found`).
    pub fn found(&mut self, target: &crate::targets::Target, message: &Message) -> io::Result<()> {
        let attachments: Vec<String> = message
            .attachments
            .iter()
            .filter_map(|a| a["url"].as_str().map(str::to_owned))
            .collect();
        let row = Row {
            place: &target.name,
            place_id: target.id.to_string(),
            channel_id: message.channel_id.to_string(),
            message_id: message.id.to_string(),
            sent_at: message.id.created_at().to_rfc3339(),
            status: "found",
            content: &message.content,
            attachments: &attachments,
            saved_files: &[],
            author: "",
        };
        self.write_row(&row)?;
        self.rows += 1;
        Ok(())
    }

    fn write_row(&mut self, row: &Row<'_>) -> io::Result<()> {
        match self.format {
            ExportFormat::Csv => {
                let attachments = row.attachments.join(" ");
                let saved = row.saved_files.join(" ");
                let cells = [
                    row.place,
                    &row.place_id,
                    &row.channel_id,
                    &row.message_id,
                    &row.sent_at,
                    row.status,
                    row.content,
                    &attachments,
                    &saved,
                    row.author,
                ];
                let line: Vec<String> = cells.iter().map(|c| csv_cell(c)).collect();
                write!(self.out, "{}\r\n", line.join(","))?;
            }
            ExportFormat::Json => {
                let separator = if self.rows == 0 { "\n  " } else { ",\n  " };
                write!(self.out, "{separator}")?;
                serde_json::to_writer(&mut self.out, row)?;
            }
            ExportFormat::JsonLines => {
                serde_json::to_writer(&mut self.out, row)?;
                writeln!(self.out)?;
            }
        }
        self.out.flush()
    }

    /// Completes the file and returns the writer.
    pub fn finish(mut self) -> io::Result<W> {
        if self.format == ExportFormat::Json {
            writeln!(self.out, "{}]", if self.rows == 0 { "" } else { "\n" })?;
        }
        self.out.flush()?;
        Ok(self.out)
    }
}

/// Quotes a CSV cell when needed. Cells starting with `=`, `+`, `-` or `@`
/// get a leading apostrophe, so a spreadsheet does not run message text as
/// a formula.
fn csv_cell(value: &str) -> String {
    let value = if value.starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn deleted(id: u64, content: &str, dry_run: bool) -> Event {
        Event::Deleted {
            target_id: Snowflake(10),
            channel_id: Snowflake(11),
            message_id: Snowflake(id),
            sent_at: Utc.with_ymd_and_hms(2024, 5, 1, 12, 0, 0).unwrap(),
            preview: String::new(),
            content: content.into(),
            attachments: vec!["https://cdn/a.png".into(), "https://cdn/b.txt".into()],
            saved: vec!["attachments/11/1_1_a.png".into()],
            dry_run,
        }
    }

    fn started() -> Event {
        Event::TargetStarted {
            index: 0,
            target_id: Snowflake(10),
            name: "Rust, Enjoyers".into(),
        }
    }

    #[test]
    fn writes_csv() {
        let mut writer = ExportWriter::new(Vec::new(), ExportFormat::Csv).unwrap();
        for event in [
            started(),
            deleted(1, "hi \"you\"\nthere", false),
            deleted(2, "=1+1", true),
        ] {
            writer.observe(&event).unwrap();
        }
        assert_eq!(writer.rows(), 2);
        let text = String::from_utf8(writer.finish().unwrap()).unwrap();
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(lines[0], format!("\u{feff}{}", CSV_HEADER.trim_end()));
        assert_eq!(
            lines[1],
            "\"Rust, Enjoyers\",10,11,1,2024-05-01T12:00:00+00:00,deleted,\"hi \"\"you\"\"\nthere\",https://cdn/a.png https://cdn/b.txt,attachments/11/1_1_a.png,"
        );
        assert!(lines[2].contains(",would_delete,'=1+1,"));
    }

    #[test]
    fn marks_the_others_messages_with_their_author() {
        let mut writer = ExportWriter::new(Vec::new(), ExportFormat::JsonLines).unwrap();
        let other = Event::SavedFromOthers {
            target_id: Snowflake(10),
            channel_id: Snowflake(10),
            message_id: Snowflake(3),
            sent_at: Utc.with_ymd_and_hms(2024, 5, 1, 12, 0, 0).unwrap(),
            author: "Friend".into(),
            content: "look".into(),
            attachments: vec!["https://cdn/c.jpg".into()],
            saved: vec!["attachments/10/3_1_c.jpg".into()],
        };
        for event in [started(), other, deleted(1, "mine", false)] {
            writer.observe(&event).unwrap();
        }
        let text = String::from_utf8(writer.finish().unwrap()).unwrap();
        let rows: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(rows[0]["status"], "saved_from_others");
        assert_eq!(rows[0]["author"], "Friend");
        assert_eq!(rows[0]["saved_files"][0], "attachments/10/3_1_c.jpg");
        assert_eq!(rows[1]["author"], "");
    }

    #[test]
    fn writes_json() {
        let mut writer = ExportWriter::new(Vec::new(), ExportFormat::Json).unwrap();
        for event in [started(), deleted(1, "ü", false), deleted(2, "b", true)] {
            writer.observe(&event).unwrap();
        }
        let value: serde_json::Value = serde_json::from_slice(&writer.finish().unwrap()).unwrap();
        assert_eq!(value[0]["place"], "Rust, Enjoyers");
        assert_eq!(value[0]["content"], "ü");
        assert_eq!(value[0]["message_id"], "1");
        assert_eq!(value[1]["status"], "would_delete");
        assert_eq!(value[1]["attachments"][0], "https://cdn/a.png");

        let empty = ExportWriter::new(Vec::new(), ExportFormat::Json).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&empty.finish().unwrap()).unwrap();
        assert_eq!(value, serde_json::json!([]));
    }

    #[test]
    fn picks_the_format_from_the_extension() {
        assert_eq!(
            ExportFormat::for_path(Path::new("a/b.JSON")),
            ExportFormat::Json
        );
        assert_eq!(
            ExportFormat::for_path(Path::new("b.csv")),
            ExportFormat::Csv
        );
        assert_eq!(ExportFormat::for_path(Path::new("b")), ExportFormat::Csv);
    }
}
