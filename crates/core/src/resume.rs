//! Continuing a clean-up that was stopped, or cut short by a crash or a lost
//! connection, instead of searching everything again.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::filter::Filter;
use crate::job::{Event, JobOptions, Stats};
use crate::snowflake::Snowflake;
use crate::targets::Target;

/// How far a run got, built from its [`Event`]s with [`Checkpoint::observe`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Checkpoint {
    /// Servers and DMs that were finished completely.
    pub finished: Vec<Snowflake>,
    /// Per server or DM: the oldest message dealt with. Search results are
    /// newest first, so only older messages are left.
    pub cursors: HashMap<Snowflake, Snowflake>,
    /// Messages that were deleted or deliberately kept. Failed ones are not
    /// listed, so they are tried again.
    pub done: HashSet<Snowflake>,
    /// Totals of all runs so far.
    pub stats: Stats,
}

impl Checkpoint {
    pub fn observe(&mut self, event: &Event) {
        match event {
            Event::Deleted {
                target_id,
                message_id,
                dry_run: false,
                ..
            } => {
                self.stats.deleted += 1;
                self.handled(*target_id, *message_id);
            }
            Event::Skipped {
                target_id,
                message_id,
                ..
            } => {
                self.stats.skipped += 1;
                self.handled(*target_id, *message_id);
            }
            Event::Failed { .. } => self.stats.failed += 1,
            Event::ChannelUnreachable { messages, .. } => self.stats.skipped += messages,
            Event::TargetFinished {
                target_id,
                complete: true,
                ..
            } if !self.finished.contains(target_id) => self.finished.push(*target_id),
            _ => {}
        }
    }

    fn handled(&mut self, target: Snowflake, message: Snowflake) {
        self.done.insert(message);
        let cursor = self.cursors.entry(target).or_insert(message);
        *cursor = (*cursor).min(message);
    }

    pub fn is_finished(&self, target: Snowflake) -> bool {
        self.finished.contains(&target)
    }
}

/// Everything needed to continue a clean-up later: exactly what was asked
/// for, so "older than 30 days" still means the same moment tomorrow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedRun {
    pub targets: Vec<Target>,
    pub filter: Filter,
    /// Without `resume`; the checkpoint is kept next to it.
    pub options: JobOptions,
    /// The data package the messages came from, if any.
    #[serde(default)]
    pub package: Option<PathBuf>,
    pub checkpoint: Checkpoint,
}

impl SavedRun {
    pub fn new(
        targets: &[Target],
        filter: &Filter,
        options: &JobOptions,
        package: Option<&Path>,
    ) -> Self {
        SavedRun {
            targets: targets.to_vec(),
            filter: filter.clone(),
            options: JobOptions {
                resume: None,
                ..options.clone()
            },
            package: package.map(Path::to_owned),
            checkpoint: Checkpoint::default(),
        }
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let data = std::fs::read(path)?;
        serde_json::from_slice(&data).map_err(std::io::Error::other)
    }

    /// Writes to a temporary file first, so a crash never leaves a broken one.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let partial = path.with_extension("partial");
        std::fs::write(
            &partial,
            serde_json::to_vec(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(&partial, path)
    }

    /// The options for continuing, with the checkpoint attached.
    pub fn resume_options(&self) -> JobOptions {
        JobOptions {
            resume: Some(self.checkpoint.clone()),
            ..self.options.clone()
        }
    }

    /// The servers and DMs that still have work left.
    pub fn remaining(&self) -> usize {
        self.targets
            .iter()
            .filter(|t| !self.checkpoint.is_finished(t.id))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::TargetKind;
    use chrono::Utc;

    fn deleted(target: u64, message: u64) -> Event {
        Event::Deleted {
            target_id: Snowflake(target),
            channel_id: Snowflake(target),
            message_id: Snowflake(message),
            sent_at: Utc::now(),
            preview: String::new(),
            content: String::new(),
            attachments: Vec::new(),
            saved: Vec::new(),
            dry_run: false,
        }
    }

    #[test]
    fn follows_the_events() {
        let mut checkpoint = Checkpoint::default();
        for event in [
            deleted(1, 50),
            deleted(1, 40),
            Event::Failed {
                target_id: Snowflake(1),
                message_id: Snowflake(30),
                error: "x".into(),
            },
            Event::TargetFinished {
                target_id: Snowflake(1),
                stats: Stats::default(),
                complete: true,
            },
            deleted(2, 90),
            Event::TargetFinished {
                target_id: Snowflake(2),
                stats: Stats::default(),
                complete: false,
            },
        ] {
            checkpoint.observe(&event);
        }
        assert_eq!(checkpoint.finished, [Snowflake(1)]);
        assert_eq!(checkpoint.cursors[&Snowflake(1)], Snowflake(40));
        assert_eq!(checkpoint.cursors[&Snowflake(2)], Snowflake(90));
        assert!(
            checkpoint.done.contains(&Snowflake(50)) && !checkpoint.done.contains(&Snowflake(30))
        );
        assert_eq!((checkpoint.stats.deleted, checkpoint.stats.failed), (3, 1));
    }

    #[test]
    fn survives_a_round_trip_through_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        let target = Target {
            kind: TargetKind::Dm,
            id: Snowflake(7),
            name: "Ann".into(),
            icon_url: None,
            channels: Vec::new(),
        };
        let mut run = SavedRun::new(&[target], &Filter::default(), &JobOptions::default(), None);
        run.checkpoint.observe(&deleted(7, 3));
        run.save(&path).unwrap();
        let loaded = SavedRun::load(&path).unwrap();
        assert_eq!(loaded, run);
        assert_eq!(loaded.remaining(), 1);
        assert!(loaded.resume_options().resume.is_some());
    }
}
