//! Which messages to delete: time range, text and content type.
//!
//! Discord's search narrows the candidates down; every message it returns is
//! then checked again here, so a fuzzy or misbehaving search can never make
//! purgecord delete a message the filter does not describe.

use chrono::{DateTime, Utc};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::models::Message;
use crate::search::SearchQuery;
use crate::snowflake::Snowflake;

/// Kinds of content a message can contain, named like Discord's `has:`
/// search operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Has {
    Link,
    /// Any attachment.
    File,
    Image,
    Video,
    Sound,
    Embed,
    Sticker,
}

impl Has {
    pub const ALL: [Has; 7] = [
        Has::Link,
        Has::File,
        Has::Image,
        Has::Video,
        Has::Sound,
        Has::Embed,
        Has::Sticker,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Has::Link => "link",
            Has::File => "file",
            Has::Image => "image",
            Has::Video => "video",
            Has::Sound => "sound",
            Has::Embed => "embed",
            Has::Sticker => "sticker",
        }
    }

    /// Whether `message` contains this kind of content. Errs on the side of
    /// "yes", because it also decides which messages a user wants to keep.
    pub fn found_in(self, message: &Message) -> bool {
        let attachment = |kind: &str| {
            message
                .attachments
                .iter()
                .any(|a| media_kind(a) == Some(kind))
        };
        let embed = |types: &[&str], field: &str| {
            message.embeds.iter().any(|e| {
                e["type"].as_str().is_some_and(|t| types.contains(&t)) || e.get(field).is_some()
            })
        };
        match self {
            Has::Link => contains_link(&message.content),
            Has::File => !message.attachments.is_empty(),
            Has::Image => attachment("image") || embed(&["image", "gifv"], "image"),
            Has::Video => attachment("video") || embed(&["video", "gifv"], "video"),
            Has::Sound => attachment("audio"),
            Has::Embed => !message.embeds.is_empty(),
            Has::Sticker => !message.sticker_items.is_empty(),
        }
    }
}

impl std::str::FromStr for Has {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Has::ALL
            .into_iter()
            .find(|has| has.as_str().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| {
                let names: Vec<&str> = Has::ALL.iter().map(|h| h.as_str()).collect();
                format!("expected one of {}, got {s:?}", names.join(", "))
            })
    }
}

/// Which messages to delete. Every condition that is set must hold.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Filter {
    /// Only messages sent at or after this time.
    pub after: Option<DateTime<Utc>>,
    /// Only messages sent before this time.
    pub before: Option<DateTime<Utc>>,
    /// Leave pinned messages alone.
    pub skip_pinned: bool,
    /// Only messages whose text contains all of these words, in any case.
    /// Passed on to Discord's search.
    pub content: Option<String>,
    /// Only messages whose text matches this regular expression (any case).
    /// Discord cannot search for it, so it is only checked here.
    pub pattern: Option<String>,
    /// Only messages with at least one of these.
    pub has: Vec<Has>,
    /// Keep messages with any of these. Only checked here.
    pub without: Vec<Has>,
}

impl Filter {
    pub(crate) fn search_query(&self, author: Snowflake) -> SearchQuery {
        let words = self.words();
        SearchQuery {
            author_id: Some(author),
            // One below the bound, so it works whether Discord treats min_id as
            // inclusive or exclusive; `contains` drops anything too early.
            min_id: self
                .after
                .map(|t| Snowflake(Snowflake::from_datetime(t).0.saturating_sub(1))),
            max_id: self.before.map(Snowflake::from_datetime),
            content: (!words.is_empty()).then(|| words.join(" ")),
            has: self.has.clone(),
            channel_ids: Vec::new(),
        }
    }

    fn words(&self) -> Vec<String> {
        self.content
            .as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_lowercase)
            .collect()
    }

    /// Whether a message with this ID was sent inside the time range.
    pub fn contains(&self, id: Snowflake) -> bool {
        let sent = id.created_at();
        self.after.is_none_or(|after| sent >= after)
            && self.before.is_none_or(|before| sent < before)
    }

    /// Whether some conditions are only checked by purgecord, so Discord's
    /// counts can be higher than what will actually be deleted.
    pub fn checks_locally(&self) -> bool {
        self.pattern
            .as_deref()
            .is_some_and(|p| !p.trim().is_empty())
            || !self.without.is_empty()
    }

    /// Validates the filter and prepares the text checks.
    pub fn compile(&self) -> Result<Matcher> {
        if let (Some(after), Some(before)) = (self.after, self.before) {
            if after >= before {
                return Err(Error::InvalidFilter(
                    "the start of the time range is not before its end".into(),
                ));
            }
        }
        let pattern = match self.pattern.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(pattern) => Some(
                RegexBuilder::new(pattern)
                    .case_insensitive(true)
                    .size_limit(1 << 20)
                    .build()
                    .map_err(|err| {
                        Error::InvalidFilter(format!("invalid regular expression: {err}"))
                    })?,
            ),
        };
        Ok(Matcher {
            words: self.words(),
            pattern,
            has: self.has.clone(),
            without: self.without.clone(),
        })
    }
}

/// The text and content checks of a [`Filter`], ready to use.
#[derive(Debug, Clone)]
pub struct Matcher {
    words: Vec<String>,
    pattern: Option<Regex>,
    has: Vec<Has>,
    without: Vec<Has>,
}

impl Matcher {
    /// Whether the message passes the text and content conditions. Author,
    /// time range and pinned state are checked elsewhere.
    pub fn matches(&self, message: &Message) -> bool {
        let text = message.content.to_lowercase();
        self.words.iter().all(|word| text.contains(word.as_str()))
            && self
                .pattern
                .as_ref()
                .is_none_or(|re| re.is_match(&message.content))
            && (self.has.is_empty() || self.has.iter().any(|h| h.found_in(message)))
            && !self.without.iter().any(|h| h.found_in(message))
    }
}

fn contains_link(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("http://") || lower.contains("https://")
}

/// "image", "video" or "audio" for an attachment, from its MIME type or,
/// failing that, its file name.
fn media_kind(attachment: &Value) -> Option<&'static str> {
    if let Some(mime) = attachment["content_type"].as_str() {
        for kind in ["image", "video", "audio"] {
            if mime.starts_with(kind) && mime[kind.len()..].starts_with('/') {
                return Some(kind);
            }
        }
    }
    let name = attachment["filename"]
        .as_str()
        .or_else(|| attachment["url"].as_str())?;
    let name = name.split(['?', '#']).next().unwrap_or(name);
    let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "heic" | "svg" => Some("image"),
        "mp4" | "mov" | "webm" | "mkv" | "avi" | "m4v" => Some("video"),
        "mp3" | "ogg" | "wav" | "flac" | "m4a" | "aac" | "opus" => Some("audio"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::User;
    use serde_json::json;

    fn message(content: &str) -> Message {
        Message {
            id: Snowflake(1),
            channel_id: Snowflake(2),
            kind: 0,
            content: content.into(),
            author: User {
                id: Snowflake(3),
                username: "me".into(),
                global_name: None,
                avatar: None,
            },
            pinned: false,
            attachments: vec![],
            embeds: vec![],
            sticker_items: vec![],
            hit: None,
        }
    }

    fn matcher(filter: Filter) -> Matcher {
        filter.compile().unwrap()
    }

    #[test]
    fn words_must_all_appear_in_any_case() {
        let m = matcher(Filter {
            content: Some("  Secret   PLAN ".into()),
            ..Default::default()
        });
        assert!(m.matches(&message("the plan is secret")));
        assert!(m.matches(&message("SECRETPLANS")));
        assert!(!m.matches(&message("the plan")));
    }

    #[test]
    fn empty_text_conditions_match_everything() {
        let m = matcher(Filter {
            content: Some("  ".into()),
            pattern: Some(" ".into()),
            ..Default::default()
        });
        assert!(m.matches(&message("")));
        let query = Filter {
            content: Some("  ".into()),
            ..Default::default()
        }
        .search_query(Snowflake(1));
        assert_eq!(query.content, None);
    }

    #[test]
    fn content_is_sent_to_the_search() {
        let query = Filter {
            content: Some(" Hello  World ".into()),
            has: vec![Has::Link, Has::Image],
            ..Default::default()
        }
        .search_query(Snowflake(1));
        assert_eq!(query.content.as_deref(), Some("hello world"));
        assert_eq!(query.has, [Has::Link, Has::Image]);
    }

    #[test]
    fn pattern_is_a_case_insensitive_regex() {
        let m = matcher(Filter {
            pattern: Some(r"^gg\b".into()),
            ..Default::default()
        });
        assert!(m.matches(&message("GG well played")));
        assert!(!m.matches(&message("eggs")));
        let bad = Filter {
            pattern: Some("(".into()),
            ..Default::default()
        };
        assert!(matches!(bad.compile(), Err(Error::InvalidFilter(_))));
    }

    #[test]
    fn has_and_without() {
        let mut photo = message("look");
        photo.attachments = vec![json!({"filename": "cat.JPG", "content_type": null})];
        let mut voice = message("");
        voice.attachments =
            vec![json!({"filename": "voice-message.ogg", "content_type": "audio/ogg"})];
        let link = message("see https://example.com");
        let mut gif = message("https://tenor.com/x");
        gif.embeds = vec![json!({"type": "gifv", "video": {"url": "x"}})];

        let only_media = matcher(Filter {
            has: vec![Has::Image, Has::Sound],
            ..Default::default()
        });
        assert!(only_media.matches(&photo));
        assert!(only_media.matches(&voice));
        assert!(only_media.matches(&gif));
        assert!(!only_media.matches(&link));

        let keep_files = matcher(Filter {
            without: vec![Has::File, Has::Video],
            ..Default::default()
        });
        assert!(!keep_files.matches(&photo));
        assert!(!keep_files.matches(&gif));
        assert!(keep_files.matches(&link));
        assert!(Has::Link.found_in(&link) && !Has::Link.found_in(&voice));
    }

    #[test]
    fn rejects_an_empty_time_range() {
        let t = Utc::now();
        let filter = Filter {
            after: Some(t),
            before: Some(t),
            ..Default::default()
        };
        assert!(filter.compile().is_err());
    }

    #[test]
    fn parses_has_names() {
        assert_eq!(" Image ".parse::<Has>(), Ok(Has::Image));
        assert!("pictures".parse::<Has>().is_err());
    }
}
