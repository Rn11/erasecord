mod common;

use std::time::Duration;

use std::sync::Arc;

use common::*;
use erasecord_core::package::{PackageChannel, PackageMessage};
use erasecord_core::scan::{self, MessageCache, ScanEvent};
use erasecord_core::vault::{self, EncryptedBackupSettings, KeySlot};
use erasecord_core::{
    job, Checkpoint, Event, Filter, Has, JobControl, JobOptions, Package, SkipReason, Snowflake,
    Summary, Target, TargetKind,
};
use tokio::sync::mpsc;
use wiremock::matchers::any;
use wiremock::{Mock, ResponseTemplate};

/// Waits until `done` holds, instead of guessing how long a slow CI
/// machine needs.
async fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting until {what}"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

fn fast() -> JobOptions {
    JobOptions {
        delete_delay_ms: 0,
        search_delay_ms: 0,
        ..Default::default()
    }
}

async fn run_job(
    fake: &FakeDiscord,
    targets: &[Target],
    filter: Filter,
    options: JobOptions,
) -> (Summary, Vec<Event>) {
    run_job_within(fake, targets, filter, options, 10).await
}

/// Like [`run_job`], with more time: sealing an encrypted backup derives
/// its key twice with scrypt, which is slow on purpose (and slower still on
/// a busy CI machine).
async fn run_job_within(
    fake: &FakeDiscord,
    targets: &[Target],
    filter: Filter,
    options: JobOptions,
    seconds: u64,
) -> (Summary, Vec<Event>) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (client, control) = (fake.client(), JobControl::new());
    let run = job::run(
        &client,
        Snowflake(ME),
        targets,
        &filter,
        &options,
        &control,
        tx,
    );
    let summary = tokio::time::timeout(Duration::from_secs(seconds), run)
        .await
        .expect("job did not finish");
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    (summary, events)
}

#[tokio::test]
async fn deletes_only_own_messages_inside_the_range() {
    let mut messages = Vec::new();
    for minute in 0..30 {
        messages.push(FakeMessage::in_guild(minute, 0, ME));
        messages.push(FakeMessage::in_guild(minute, 1, OTHER));
    }
    let fake = FakeDiscord::start(State::with_messages(messages.clone())).await;
    let filter = Filter {
        after: Some(at(10)),
        before: Some(at(20)),
        ..Default::default()
    };

    let (summary, events) = run_job(&fake, &[guild_target()], filter, fast()).await;

    assert_eq!(summary.stats.deleted, 10);
    assert_eq!((summary.cancelled, summary.error), (false, None));
    let mut expected: Vec<u64> = messages
        .iter()
        .filter(|m| !(m.author_id == ME && (10..20).any(|min| m.id == id_at(min, 0))))
        .map(|m| m.id)
        .collect();
    expected.sort_unstable();
    assert_eq!(fake.remaining(), expected);
    assert!(matches!(events.last(), Some(Event::Finished(_))));
}

#[tokio::test]
async fn pages_through_more_than_one_search_page() {
    let messages = (0..60)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let fake = FakeDiscord::start(State::with_messages(messages)).await;

    let (summary, _) = run_job(&fake, &[dm_target()], Filter::default(), fast()).await;

    assert_eq!(summary.stats.deleted, 60);
    assert_eq!(fake.delete_calls(), 60);
    assert!(fake.remaining().is_empty());
}

#[tokio::test]
async fn skips_pinned_system_and_forbidden_messages() {
    let normal = FakeMessage::in_guild(0, 0, ME);
    let pinned = FakeMessage {
        pinned: true,
        ..FakeMessage::in_guild(1, 0, ME)
    };
    let system = FakeMessage {
        kind: 7,
        ..FakeMessage::in_guild(2, 0, ME)
    };
    let forbidden = FakeMessage::in_guild(3, 0, ME);
    let archived = FakeMessage::in_guild(4, 0, ME);
    let mut state = State::with_messages(vec![
        normal.clone(),
        pinned.clone(),
        system.clone(),
        forbidden.clone(),
        archived.clone(),
    ]);
    state.delete_errors.insert(forbidden.id, (403, 50013));
    state.delete_errors.insert(archived.id, (400, 50083));
    let fake = FakeDiscord::start(state).await;
    let filter = Filter {
        skip_pinned: true,
        ..Default::default()
    };

    let (summary, events) = run_job(&fake, &[guild_target()], filter, fast()).await;

    assert_eq!(
        (
            summary.stats.deleted,
            summary.stats.skipped,
            summary.stats.failed
        ),
        (1, 4, 0)
    );
    // Pinned and system messages are never even attempted.
    assert_eq!(fake.delete_calls(), 3);
    let reasons: Vec<SkipReason> = events
        .iter()
        .filter_map(|e| match e {
            Event::Skipped { reason, .. } => Some(*reason),
            _ => None,
        })
        .collect();
    assert_eq!(
        reasons,
        [
            SkipReason::ArchivedThread,
            SkipReason::NoPermission,
            SkipReason::SystemMessage,
            SkipReason::Pinned,
        ]
    );
}

#[tokio::test]
async fn searches_again_for_messages_the_index_returned_late() {
    let mut messages: Vec<FakeMessage> =
        (0..5).map(|minute| FakeMessage::in_dm(minute, 0)).collect();
    for minute in 5..7 {
        messages.push(FakeMessage {
            hidden_for_searches: 2,
            ..FakeMessage::in_dm(minute, 0)
        });
    }
    let fake = FakeDiscord::start(State::with_messages(messages)).await;

    let (summary, _) = run_job(&fake, &[dm_target()], Filter::default(), fast()).await;

    assert_eq!(summary.stats.deleted, 7);
    assert!(fake.remaining().is_empty());
}

#[tokio::test]
async fn stale_search_results_do_not_cause_repeated_deletes() {
    let messages = (0..30)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let mut state = State::with_messages(messages);
    state.stale_index = true;
    let fake = FakeDiscord::start(state).await;

    let (summary, _) = run_job(&fake, &[dm_target()], Filter::default(), fast()).await;

    assert_eq!(summary.stats.deleted, 30);
    assert_eq!(fake.delete_calls(), 30);
}

#[tokio::test]
async fn dry_run_deletes_nothing() {
    let messages = (0..10)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let options = JobOptions {
        dry_run: true,
        ..fast()
    };

    let (summary, events) = run_job(&fake, &[dm_target()], Filter::default(), options).await;

    assert_eq!(summary.stats.deleted, 10);
    assert_eq!(fake.delete_calls(), 0);
    assert_eq!(fake.remaining().len(), 10);
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Deleted { dry_run: true, .. })));
}

#[tokio::test]
async fn a_failing_target_does_not_stop_the_others() {
    let mut state = State::with_messages(vec![
        FakeMessage::in_guild(0, 0, ME),
        FakeMessage::in_dm(1, 0),
    ]);
    state.forbidden_guilds.push(GUILD);
    let fake = FakeDiscord::start(state).await;

    let (summary, events) = run_job(
        &fake,
        &[guild_target(), dm_target()],
        Filter::default(),
        fast(),
    )
    .await;

    assert_eq!(summary.stats.deleted, 1);
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::TargetFailed { target_id, .. } if target_id.0 == GUILD)));
    assert_eq!(fake.remaining(), vec![id_at(0, 0)]);
}

#[tokio::test]
async fn rejected_token_stops_the_job() {
    let fake = FakeDiscord::start(State::with_messages(vec![FakeMessage::in_dm(0, 0)])).await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(401))
        .with_priority(1)
        .mount(&fake.server)
        .await;

    let (summary, _) = run_job(
        &fake,
        &[dm_target(), guild_target()],
        Filter::default(),
        fast(),
    )
    .await;

    assert!(summary.error.unwrap().contains("401"));
    assert_eq!(fake.state.lock().unwrap().search_calls, 0);
}

#[tokio::test]
async fn cancel_stops_after_the_current_message() {
    let messages = (0..50)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let control = JobControl::new();
    let (tx, _rx) = mpsc::unbounded_channel();
    let options = JobOptions {
        delete_delay_ms: 20,
        ..fast()
    };
    let client = fake.client();
    let task = {
        let control = control.clone();
        tokio::spawn(async move {
            job::run(
                &client,
                Snowflake(ME),
                &[dm_target()],
                &Filter::default(),
                &options,
                &control,
                tx,
            )
            .await
        })
    };

    wait_until("something was deleted", || fake.delete_calls() > 0).await;
    control.cancel();
    let summary = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();

    assert!(summary.cancelled);
    assert!(summary.stats.deleted > 0 && summary.stats.deleted < 50);
    assert_eq!(fake.remaining().len() as u64, 50 - summary.stats.deleted);
}

#[tokio::test]
async fn pause_holds_the_job_until_resumed() {
    let messages = (0..3).map(|minute| FakeMessage::in_dm(minute, 0)).collect();
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let control = JobControl::new();
    control.pause();
    let (tx, _rx) = mpsc::unbounded_channel();
    let client = fake.client();
    let task = {
        let control = control.clone();
        tokio::spawn(async move {
            job::run(
                &client,
                Snowflake(ME),
                &[dm_target()],
                &Filter::default(),
                &fast(),
                &control,
                tx,
            )
            .await
        })
    };

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(fake.state.lock().unwrap().search_calls, 0);
    control.resume();
    let summary = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(summary.stats.deleted, 3);
}

#[tokio::test]
async fn preview_counts_matching_messages_per_target() {
    let mut messages: Vec<FakeMessage> = (0..8)
        .map(|minute| FakeMessage::in_guild(minute, 0, ME))
        .collect();
    messages.extend((0..8).map(|minute| FakeMessage::in_guild(minute, 1, OTHER)));
    messages.extend((0..4).map(|minute| FakeMessage::in_dm(minute, 2)));
    let mut state = State::with_messages(messages);
    state.forbidden_guilds.push(99);
    let fake = FakeDiscord::start(state).await;
    let forbidden = Target {
        id: Snowflake(99),
        ..guild_target()
    };
    let filter = Filter {
        after: Some(at(2)),
        ..Default::default()
    };

    let mut progress = Vec::new();
    let entries = job::preview(
        &fake.client(),
        Snowflake(ME),
        &[guild_target(), dm_target(), forbidden],
        &filter,
        &fast(),
        &JobControl::new(),
        |index, _| progress.push(index),
    )
    .await
    .unwrap();

    let counts: Vec<Option<u64>> = entries.iter().map(|e| e.count).collect();
    assert_eq!(counts, [Some(6), Some(2), None]);
    assert!(entries[2]
        .error
        .as_deref()
        .unwrap()
        .contains("Missing Access"));
    assert_eq!(progress, [0, 1, 2]);
    assert_eq!(fake.delete_calls(), 0);
}

fn skip_reasons(events: &[Event]) -> Vec<SkipReason> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Skipped { reason, .. } => Some(*reason),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn keyword_is_searched_and_checked_again() {
    let keep = FakeMessage::in_dm(0, 0).with_text("secret meeting later");
    // The fake search is fuzzy like Discord's and returns this one as well.
    let fuzzy = FakeMessage::in_dm(1, 0).with_text("the plan");
    let hit = FakeMessage::in_dm(2, 0).with_text("Secret PLAN, don't tell");
    let other = FakeMessage::in_dm(3, 0).with_text("hello");
    let fake = FakeDiscord::start(State::with_messages(vec![
        keep.clone(),
        fuzzy.clone(),
        hit.clone(),
        other.clone(),
    ]))
    .await;
    let filter = Filter {
        content: Some("secret plan".into()),
        ..Default::default()
    };

    let (summary, events) = run_job(&fake, &[dm_target()], filter, fast()).await;

    assert_eq!((summary.stats.deleted, summary.stats.skipped), (1, 2));
    assert_eq!(skip_reasons(&events), [SkipReason::Excluded; 2]);
    let mut expected = vec![keep.id, fuzzy.id, other.id];
    expected.sort_unstable();
    assert_eq!(fake.remaining(), expected);
}

#[tokio::test]
async fn pattern_and_without_keep_messages() {
    let photo = FakeMessage {
        files: vec!["cat.png".into()],
        ..FakeMessage::in_dm(0, 0).with_text("lol look")
    };
    let lol = FakeMessage::in_dm(1, 0).with_text("LOL");
    let other = FakeMessage::in_dm(2, 0).with_text("lollipop");
    let fake = FakeDiscord::start(State::with_messages(vec![
        photo.clone(),
        lol.clone(),
        other.clone(),
    ]))
    .await;
    let filter = Filter {
        pattern: Some(r"\blol\b".into()),
        without: vec![Has::File],
        ..Default::default()
    };

    let (summary, _) = run_job(&fake, &[dm_target()], filter, fast()).await;

    assert_eq!((summary.stats.deleted, summary.stats.skipped), (1, 2));
    let mut expected = vec![photo.id, other.id];
    expected.sort_unstable();
    assert_eq!(fake.remaining(), expected);
}

#[tokio::test]
async fn has_file_only_deletes_messages_with_attachments() {
    let photo = FakeMessage {
        files: vec!["cat.png".into()],
        ..FakeMessage::in_dm(0, 0)
    };
    let text = FakeMessage::in_dm(1, 0);
    let fake = FakeDiscord::start(State::with_messages(vec![photo.clone(), text.clone()])).await;
    let filter = Filter {
        has: vec![Has::File],
        ..Default::default()
    };

    let (summary, _) = run_job(&fake, &[dm_target()], filter, fast()).await;

    assert_eq!(summary.stats.deleted, 1);
    assert_eq!(fake.remaining(), vec![text.id]);
}

#[tokio::test]
async fn invalid_pattern_stops_before_any_request() {
    let fake = FakeDiscord::start(State::with_messages(vec![FakeMessage::in_dm(0, 0)])).await;
    let filter = Filter {
        pattern: Some("([".into()),
        ..Default::default()
    };

    let (summary, events) = run_job(&fake, &[dm_target()], filter, fast()).await;

    assert!(summary.error.unwrap().contains("regular expression"));
    assert_eq!(events.len(), 1);
    assert_eq!(fake.state.lock().unwrap().search_calls, 0);
}

#[tokio::test]
async fn only_selected_channels_are_cleaned_up() {
    for ignore_channel_filter in [false, true] {
        let first = FakeMessage::in_guild(0, 0, ME);
        let second = FakeMessage {
            channel_id: GUILD_CHANNEL_2,
            ..FakeMessage::in_guild(1, 0, ME)
        };
        let mut state = State::with_messages(vec![first.clone(), second.clone()]);
        state.ignore_channel_filter = ignore_channel_filter;
        let fake = FakeDiscord::start(state).await;
        let target = Target {
            channels: vec![Snowflake(GUILD_CHANNEL_2)],
            ..guild_target()
        };

        let (summary, _) = run_job(&fake, &[target], Filter::default(), fast()).await;

        assert_eq!(summary.stats.deleted, 1, "ignore: {ignore_channel_filter}");
        assert_eq!(fake.remaining(), vec![first.id]);
    }
}

#[tokio::test]
async fn overwrites_before_deleting() {
    let messages = vec![FakeMessage::in_dm(0, 0), FakeMessage::in_dm(1, 0)];
    let ids: Vec<u64> = messages.iter().map(|m| m.id).collect();
    let fake = FakeDiscord::start(State::with_messages(messages)).await;

    for (text, random) in [("", true), ("deleted", false)] {
        fake.state.lock().unwrap().edits.clear();
        let options = JobOptions {
            overwrite: Some(text.into()),
            dry_run: false,
            ..fast()
        };
        let (summary, _) = run_job(&fake, &[dm_target()], Filter::default(), options).await;
        if random {
            assert_eq!(summary.stats.deleted, 2);
            let state = fake.state.lock().unwrap();
            // Newest first: edit, then delete, message by message.
            let expected: Vec<String> = ids
                .iter()
                .rev()
                .flat_map(|id| [format!("PATCH {id}"), format!("DELETE {id}")])
                .collect();
            assert_eq!(state.log, expected);
            for (_, body) in &state.edits {
                assert!(!body["content"].as_str().unwrap().is_empty());
                assert_eq!(body["attachments"], serde_json::json!([]));
            }
        } else {
            // Everything is gone already; nothing to edit.
            assert_eq!(summary.stats.deleted, 0);
            assert!(fake.state.lock().unwrap().edits.is_empty());
        }
    }
    assert!(fake.remaining().is_empty());
}

#[tokio::test]
async fn stopping_during_a_delete_still_counts_it() {
    let messages = (0..5).map(|minute| FakeMessage::in_dm(minute, 0)).collect();
    let mut state = State::with_messages(messages);
    state.delete_delay = Duration::from_millis(500);
    let fake = FakeDiscord::start(state).await;
    let control = JobControl::new();
    let (tx, _rx) = mpsc::unbounded_channel();
    let client = fake.client();
    let task = {
        let control = control.clone();
        tokio::spawn(async move {
            job::run(
                &client,
                Snowflake(ME),
                &[dm_target()],
                &Filter::default(),
                &fast(),
                &control,
                tx,
            )
            .await
        })
    };

    // Stop while the first delete has reached the server but not answered.
    wait_until("the first delete arrived", || fake.delete_calls() > 0).await;
    control.cancel();
    let summary = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();

    assert!(summary.cancelled);
    assert_eq!(summary.stats.deleted, 1);
    assert_eq!(fake.remaining().len(), 4);
}

/// A package holding the given messages, as the API fake knows them.
fn package_of(messages: &[FakeMessage]) -> Package {
    let mut channels: Vec<PackageChannel> = Vec::new();
    for m in messages {
        let message = PackageMessage {
            id: Snowflake(m.id),
            content: m.text(),
            attachments: m.files.iter().map(|f| format!("https://cdn/{f}")).collect(),
        };
        match channels.iter_mut().find(|c| c.id.0 == m.channel_id) {
            Some(channel) => channel.messages.insert(0, message),
            None => channels.push(PackageChannel {
                id: Snowflake(m.channel_id),
                kind: if m.guild_id.is_some() {
                    TargetKind::Guild
                } else {
                    TargetKind::Dm
                },
                name: format!("channel {}", m.channel_id),
                guild: m.guild_id.map(|g| (Snowflake(g), "Test server".to_owned())),
                messages: vec![message],
                recipients: Vec::new(),
            }),
        }
    }
    for channel in &mut channels {
        channel.messages.sort_by_key(|m| std::cmp::Reverse(m.id));
    }
    Package {
        owner: Some(Snowflake(ME)),
        channels,
    }
}

async fn run_package_job(
    fake: &FakeDiscord,
    package: &Package,
    targets: &[Target],
    filter: Filter,
    options: JobOptions,
) -> (Summary, Vec<Event>) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (client, control) = (fake.client(), JobControl::new());
    let run = job::run_package(
        &client,
        Snowflake(ME),
        package,
        targets,
        &filter,
        &options,
        &control,
        tx,
    );
    let summary = tokio::time::timeout(Duration::from_secs(10), run)
        .await
        .expect("job did not finish");
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    (summary, events)
}

#[tokio::test]
async fn deletes_from_a_data_package_without_searching() {
    let in_range = FakeMessage::in_guild(5, 0, ME).with_text("party tonight");
    let other_channel = FakeMessage {
        channel_id: GUILD_CHANNEL_2,
        ..FakeMessage::in_guild(6, 0, ME).with_text("party!")
    };
    let pinned = FakeMessage {
        pinned: true,
        ..FakeMessage::in_guild(7, 0, ME).with_text("party rules")
    };
    let no_match = FakeMessage::in_guild(8, 0, ME).with_text("hello");
    let too_old = FakeMessage::in_guild(0, 0, ME).with_text("party");
    let messages = vec![
        in_range.clone(),
        other_channel.clone(),
        pinned.clone(),
        no_match.clone(),
        too_old.clone(),
    ];
    let package = package_of(&messages);
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let targets: Vec<Target> = package.targets().into_iter().map(|t| t.target).collect();
    let filter = Filter {
        after: Some(at(1)),
        content: Some("party".into()),
        skip_pinned: true,
        ..Default::default()
    };

    let preview = job::preview_package(&package, Snowflake(ME), &targets, &filter).unwrap();
    assert_eq!(preview[0].count, Some(3));

    let only_first_channel = Target {
        channels: vec![Snowflake(GUILD_CHANNEL)],
        ..targets[0].clone()
    };
    let (summary, events) =
        run_package_job(&fake, &package, &[only_first_channel], filter, fast()).await;

    assert_eq!((summary.stats.deleted, summary.stats.skipped), (1, 1));
    assert_eq!(skip_reasons(&events), [SkipReason::Pinned]);
    assert_eq!(fake.state.lock().unwrap().search_calls, 0);
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::TargetEstimate { total: 2, .. })));
    let mut expected = vec![other_channel.id, pinned.id, no_match.id, too_old.id];
    expected.sort_unstable();
    assert_eq!(fake.remaining(), expected);
}

#[tokio::test]
async fn unreachable_package_channels_cost_one_request() {
    let messages: Vec<FakeMessage> = (0..20)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let package = package_of(&messages);
    let mut state = State::with_messages(messages);
    state.gone_channels.push(DM_CHANNEL);
    let fake = FakeDiscord::start(state).await;
    let targets: Vec<Target> = package.targets().into_iter().map(|t| t.target).collect();

    let (summary, events) =
        run_package_job(&fake, &package, &targets, Filter::default(), fast()).await;

    assert_eq!((summary.stats.deleted, summary.stats.skipped), (0, 20));
    assert_eq!(fake.delete_calls(), 0);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::ChannelUnreachable { messages: 20, error, .. } if error.contains("Unknown Channel")
    )));
}

#[tokio::test]
async fn package_runs_reject_filters_the_package_cannot_answer() {
    let package = package_of(&[FakeMessage::in_dm(0, 0)]);
    let targets: Vec<Target> = package.targets().into_iter().map(|t| t.target).collect();
    let filter = Filter {
        without: vec![Has::Sticker],
        ..Default::default()
    };
    assert!(job::preview_package(&package, Snowflake(ME), &targets, &filter).is_err());
}

#[tokio::test]
async fn refuses_a_package_of_another_account() {
    let messages = vec![FakeMessage::in_dm(0, 0)];
    let package = Package {
        owner: Some(Snowflake(OTHER)),
        ..package_of(&messages)
    };
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let targets: Vec<Target> = package.targets().into_iter().map(|t| t.target).collect();

    let (summary, _) = run_package_job(&fake, &package, &targets, Filter::default(), fast()).await;

    assert!(summary.error.unwrap().contains("another account"));
    assert_eq!(fake.delete_calls(), 0);
}

#[tokio::test]
async fn keeps_every_pin_of_a_channel_with_many() {
    for legacy_pins in [false, true] {
        // 120 pinned messages, one minute apart, and one that is not pinned.
        let mut messages: Vec<FakeMessage> = (0..120)
            .map(|minute| FakeMessage {
                pinned: true,
                ..FakeMessage::in_dm(minute, 0)
            })
            .collect();
        messages.push(FakeMessage::in_dm(500, 0));
        let package = package_of(&messages);
        let mut state = State::with_messages(messages);
        state.legacy_pins = legacy_pins;
        let fake = FakeDiscord::start(state).await;
        let targets: Vec<Target> = package.targets().into_iter().map(|t| t.target).collect();
        let filter = Filter {
            skip_pinned: true,
            ..Default::default()
        };

        let (summary, events) = run_package_job(&fake, &package, &targets, filter, fast()).await;

        if legacy_pins {
            // The old endpoint cannot list them all, so nothing is touched.
            assert_eq!((summary.stats.deleted, summary.stats.skipped), (0, 121));
            assert!(events.iter().any(
                |e| matches!(e, Event::ChannelUnreachable { error, .. } if error.contains("pinned"))
            ));
        } else {
            assert_eq!((summary.stats.deleted, summary.stats.skipped), (1, 120));
        }
        assert_eq!(fake.remaining().len(), if legacy_pins { 121 } else { 120 });
    }
}

fn saved_files(events: &[Event]) -> Vec<Vec<String>> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Deleted { saved, .. } => Some(saved.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn backs_up_attachments_before_deleting() {
    let photo = FakeMessage {
        files: vec!["cat.png".into(), "notes.txt".into()],
        ..FakeMessage::in_dm(0, 0)
    };
    // Its link has expired and has to be refreshed first.
    let old = FakeMessage {
        files: vec!["expired-dog.png".into()],
        ..FakeMessage::in_dm(1, 0)
    };
    let text = FakeMessage::in_dm(2, 0);
    let fake =
        FakeDiscord::start(State::with_messages(vec![photo.clone(), old.clone(), text])).await;
    let dir = tempfile::tempdir().unwrap();
    let options = JobOptions {
        backup_dir: Some(dir.path().to_owned()),
        ..fast()
    };

    let (summary, events) = run_job(&fake, &[dm_target()], Filter::default(), options).await;

    assert_eq!((summary.stats.deleted, summary.stats.failed), (3, 0));
    assert!(fake.remaining().is_empty());
    let read = |relative: &str| std::fs::read_to_string(dir.path().join(relative)).unwrap();
    let cat = format!("attachments/{DM_CHANNEL}/{}_1_cat.png", photo.id);
    let notes = format!("attachments/{DM_CHANNEL}/{}_2_notes.txt", photo.id);
    let dog = format!("attachments/{DM_CHANNEL}/{}_1_expired-dog.png", old.id);
    assert_eq!(read(&cat), "contents of cat.png");
    assert_eq!(read(&notes), "contents of notes.txt");
    assert_eq!(read(&dog), "contents of expired-dog.png");
    // Newest first; the text message has nothing to save.
    assert_eq!(saved_files(&events), [vec![], vec![dog], vec![cat, notes]]);
    assert_eq!(fake.state.lock().unwrap().refreshed.len(), 1);
}

#[tokio::test]
async fn downloads_wait_between_files_and_when_asked_to() {
    let messages: Vec<FakeMessage> = (0..3)
        .map(|minute| FakeMessage {
            files: vec![format!("busy-{minute}.png"), format!("b{minute}.png")],
            ..FakeMessage::in_dm(minute, 0)
        })
        .collect();
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let dir = tempfile::tempdir().unwrap();
    // A dry run: no pauses between deletions, only between downloads
    // (40 % of 100 ms, give or take a quarter).
    let options = JobOptions {
        backup_dir: Some(dir.path().to_owned()),
        dry_run: true,
        delete_delay_ms: 100,
        ..fast()
    };

    let started = std::time::Instant::now();
    let (summary, _) = run_job(&fake, &[dm_target()], Filter::default(), options).await;

    assert_eq!((summary.stats.deleted, summary.stats.failed), (3, 0));
    assert_eq!(files_below(dir.path()).len(), 6);
    // Six files, three of them asked twice: eight pauses of at least 30 ms,
    // plus the waits the file server asked for.
    assert_eq!(fake.state.lock().unwrap().downloads.len(), 9);
    assert!(started.elapsed() >= Duration::from_millis(8 * 30 + 3 * 50));
}

#[tokio::test]
async fn saves_what_others_sent_in_a_dm_without_touching_it() {
    let mine = FakeMessage {
        files: vec!["mine.png".into()],
        ..FakeMessage::in_dm(10, 0)
    };
    let theirs = FakeMessage {
        author_id: OTHER,
        files: vec!["theirs.jpg".into(), "clip.mp4".into()],
        ..FakeMessage::in_dm(20, 0)
    };
    let their_text = FakeMessage {
        author_id: OTHER,
        ..FakeMessage::in_dm(30, 0)
    };
    let too_old = FakeMessage {
        author_id: OTHER,
        files: vec!["old.png".into()],
        ..FakeMessage::in_dm(1, 0)
    };
    let fake = FakeDiscord::start(State::with_messages(vec![
        mine.clone(),
        theirs.clone(),
        their_text.clone(),
        too_old.clone(),
    ]))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let options = JobOptions {
        backup_dir: Some(dir.path().to_owned()),
        backup_others: true,
        ..fast()
    };
    let filter = Filter {
        after: Some(at(5)),
        ..Filter::default()
    };

    let (summary, events) = run_job(&fake, &[dm_target()], filter, options).await;

    assert_eq!(summary.stats.deleted, 1);
    assert_eq!(summary.stats.saved_from_others, 1);
    assert_eq!(fake.remaining(), {
        let mut ids = vec![theirs.id, their_text.id, too_old.id];
        ids.sort_unstable();
        ids
    });
    let read = |relative: &str| std::fs::read_to_string(dir.path().join(relative)).unwrap();
    let photo = format!("attachments/{DM_CHANNEL}/{}_1_theirs.jpg", theirs.id);
    let clip = format!("attachments/{DM_CHANNEL}/{}_2_clip.mp4", theirs.id);
    assert_eq!(read(&photo), "contents of theirs.jpg");
    assert_eq!(read(&clip), "contents of clip.mp4");
    assert_eq!(files_below(dir.path()).len(), 3);
    let saved: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::SavedFromOthers {
                message_id,
                author,
                saved,
                ..
            } => Some((message_id.0, author.clone(), saved.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        saved,
        [(theirs.id, "someone".to_owned(), vec![photo, clip])]
    );
}

#[tokio::test]
async fn never_saves_what_others_sent_on_a_server() {
    let theirs = FakeMessage {
        files: vec!["theirs.jpg".into()],
        ..FakeMessage::in_guild(20, 0, OTHER)
    };
    let mine = FakeMessage {
        files: vec!["mine.png".into()],
        ..FakeMessage::in_guild(10, 0, ME)
    };
    let fake = FakeDiscord::start(State::with_messages(vec![theirs, mine])).await;
    let dir = tempfile::tempdir().unwrap();
    let options = JobOptions {
        backup_dir: Some(dir.path().to_owned()),
        backup_others: true,
        ..fast()
    };

    let (summary, events) = run_job(&fake, &[guild_target()], Filter::default(), options).await;

    assert_eq!(
        (summary.stats.deleted, summary.stats.saved_from_others),
        (1, 0)
    );
    assert_eq!(files_below(dir.path()).len(), 1);
    assert_eq!(fake.state.lock().unwrap().downloads, ["mine.png"]);
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::SavedFromOthers { .. })));
}

#[tokio::test]
async fn others_files_go_into_the_encrypted_archive_too() {
    let mut messages = photos(3);
    messages.push(FakeMessage {
        author_id: OTHER,
        files: vec!["gift.png".into()],
        ..FakeMessage::in_dm(10, 0)
    });
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let dir = tempfile::tempdir().unwrap();
    let mut options = encrypted_options(dir.path(), "pass");
    options.backup_others = true;

    let (summary, events) =
        run_job_within(&fake, &[dm_target()], Filter::default(), options, 120).await;

    assert_eq!(
        (summary.stats.deleted, summary.stats.saved_from_others),
        (3, 1)
    );
    let archive = events
        .iter()
        .find_map(|e| match e {
            Event::BackupSealed { archive, files, .. } => {
                assert_eq!(*files, 4);
                Some(archive.clone())
            }
            _ => None,
        })
        .expect("the backup was sealed");
    let out = tempfile::tempdir().unwrap();
    vault::open(&archive, &vault::secret("pass"), out.path()).unwrap();
    let list: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(out.path().join("messages.json")).unwrap()).unwrap();
    let others: Vec<_> = list
        .iter()
        .filter(|row| row["status"] == "saved_from_others")
        .collect();
    assert_eq!(others.len(), 1);
    assert_eq!(others[0]["author"], "someone");
}

#[tokio::test]
async fn keeps_messages_whose_attachments_cannot_be_saved() {
    let gone = FakeMessage {
        files: vec!["missing-video.mp4".into()],
        ..FakeMessage::in_dm(0, 0)
    };
    let fine = FakeMessage {
        files: vec!["ok.png".into()],
        ..FakeMessage::in_dm(1, 0)
    };
    let fake = FakeDiscord::start(State::with_messages(vec![gone.clone(), fine])).await;
    let dir = tempfile::tempdir().unwrap();
    let options = JobOptions {
        backup_dir: Some(dir.path().to_owned()),
        ..fast()
    };

    let (summary, events) = run_job(&fake, &[dm_target()], Filter::default(), options).await;

    assert_eq!((summary.stats.deleted, summary.stats.failed), (1, 1));
    assert_eq!(fake.remaining(), vec![gone.id]);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Failed { message_id, error, .. }
            if message_id.0 == gone.id && error.contains("could not be backed up")
    )));
}

#[tokio::test]
async fn dry_run_with_a_backup_only_saves() {
    let photo = FakeMessage {
        files: vec!["cat.png".into()],
        ..FakeMessage::in_dm(0, 0)
    };
    let fake = FakeDiscord::start(State::with_messages(vec![photo.clone()])).await;
    let dir = tempfile::tempdir().unwrap();
    let options = JobOptions {
        backup_dir: Some(dir.path().to_owned()),
        dry_run: true,
        ..fast()
    };

    let (summary, _) = run_job(&fake, &[dm_target()], Filter::default(), options).await;

    assert_eq!(summary.stats.deleted, 1);
    assert_eq!(fake.delete_calls(), 0);
    let saved = dir
        .path()
        .join(format!("attachments/{DM_CHANNEL}/{}_1_cat.png", photo.id));
    assert!(saved.exists());
}

/// Starts a run, stops it once `stop_after` messages were deleted and
/// returns its events.
async fn stopped_run(
    fake: &FakeDiscord,
    package: Option<&Package>,
    targets: &[Target],
    filter: &Filter,
    stop_after: usize,
) -> Vec<Event> {
    let control = JobControl::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let options = JobOptions {
        delete_delay_ms: 30,
        ..fast()
    };
    let (client, targets, filter) = (fake.client(), targets.to_vec(), filter.clone());
    let package = package.cloned();
    let task = {
        let control = control.clone();
        tokio::spawn(async move {
            match package {
                Some(package) => {
                    job::run_package(
                        &client,
                        Snowflake(ME),
                        &package,
                        &targets,
                        &filter,
                        &options,
                        &control,
                        tx,
                    )
                    .await
                }
                None => {
                    job::run(
                        &client,
                        Snowflake(ME),
                        &targets,
                        &filter,
                        &options,
                        &control,
                        tx,
                    )
                    .await
                }
            }
        })
    };
    wait_until("enough was deleted", || fake.delete_calls() >= stop_after).await;
    control.cancel();
    let summary = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert!(summary.cancelled);
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

fn checkpoint_of(events: &[Event]) -> Checkpoint {
    let mut checkpoint = Checkpoint::default();
    for event in events {
        checkpoint.observe(event);
    }
    checkpoint
}

#[tokio::test]
async fn a_stopped_run_can_be_continued() {
    let mut messages: Vec<FakeMessage> = (0..40)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    for minute in [5, 20, 35] {
        messages.push(FakeMessage {
            pinned: true,
            ..FakeMessage::in_dm(minute, 1)
        });
    }
    // Message IDs are unique across Discord, so not the same as a DM message.
    messages.push(FakeMessage::in_guild(0, 9, ME));
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let filter = Filter {
        skip_pinned: true,
        ..Default::default()
    };
    let targets = [guild_target(), dm_target()];

    let first = stopped_run(&fake, None, &targets, &filter, 12).await;
    let mut checkpoint = checkpoint_of(&first);
    // The server was done before the stop; the DM was interrupted.
    assert_eq!(checkpoint.finished, [Snowflake(GUILD)]);
    let deleted_before = checkpoint.stats.deleted;
    let skipped_before = checkpoint.stats.skipped;

    let options = JobOptions {
        resume: Some(checkpoint.clone()),
        ..fast()
    };
    let (summary, events) = run_job(&fake, &targets, filter, options).await;
    for event in &events {
        checkpoint.observe(event);
    }

    assert!(!summary.cancelled && summary.error.is_none());
    // The finished server is not searched again.
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::TargetStarted { target_id, .. } if target_id.0 == GUILD)));
    // Every message was deleted exactly once, every pin reported once.
    assert_eq!(checkpoint.stats.deleted, 41);
    assert_eq!(checkpoint.stats.skipped, 3);
    assert_eq!(summary.stats.deleted, 41 - deleted_before);
    assert_eq!(summary.stats.skipped, 3 - skipped_before);
    assert_eq!(fake.delete_calls(), 41);
    assert_eq!(fake.remaining().len(), 3);
}

/// A run stopped after its last message (e.g. during the pause after it, or
/// while searching again from the top) still retries the ones that failed.
#[tokio::test]
async fn a_continued_run_retries_failed_messages_above_where_it_stopped() {
    let messages: Vec<FakeMessage> = (0..10)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let failing = messages[7].id;
    let mut state = State::with_messages(messages);
    state.delete_errors.insert(failing, (400, 0));
    let fake = FakeDiscord::start(state).await;
    let targets = [dm_target()];
    let options = JobOptions {
        max_rounds: 1,
        ..fast()
    };
    let (_, first) = run_job(&fake, &targets, Filter::default(), options).await;
    // Everything was dealt with, but the DM did not count as finished.
    let checkpoint = checkpoint_of(
        &first
            .into_iter()
            .filter(|e| !matches!(e, Event::TargetFinished { .. }))
            .collect::<Vec<_>>(),
    );
    assert_eq!(checkpoint.stats.failed, 1);
    assert_eq!(fake.remaining(), [failing]);
    fake.state.lock().unwrap().delete_errors.clear();

    let options = JobOptions {
        resume: Some(checkpoint),
        ..fast()
    };
    let (summary, _) = run_job(&fake, &targets, Filter::default(), options).await;
    assert_eq!(summary.stats.deleted, 1);
    assert!(fake.remaining().is_empty());
}

#[tokio::test]
async fn a_stopped_package_run_can_be_continued() {
    let messages: Vec<FakeMessage> = (0..30)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let package = package_of(&messages);
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let targets: Vec<Target> = package.targets().into_iter().map(|t| t.target).collect();

    let first = stopped_run(&fake, Some(&package), &targets, &Filter::default(), 10).await;
    let checkpoint = checkpoint_of(&first);
    let options = JobOptions {
        resume: Some(checkpoint.clone()),
        ..fast()
    };
    let (summary, events) =
        run_package_job(&fake, &package, &targets, Filter::default(), options).await;

    assert_eq!(checkpoint.stats.deleted + summary.stats.deleted, 30);
    // Already deleted messages are not tried again (they would count twice).
    assert_eq!(fake.delete_calls(), 30);
    assert!(fake.remaining().is_empty());
    let remaining = 30 - checkpoint.stats.deleted;
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::TargetEstimate { total, .. } if *total == remaining)));
}

fn encrypted_options(dir: &std::path::Path, passphrase: &str) -> JobOptions {
    let (settings, keys) =
        EncryptedBackupSettings::create(dir, vault::secret(passphrase)).expect("backup created");
    JobOptions {
        backup_dir: Some(dir.to_owned()),
        backup_encryption: Some(settings),
        backup_keys: KeySlot(Some(Arc::new(keys))),
        ..fast()
    }
}

fn photos(count: i64) -> Vec<FakeMessage> {
    (0..count)
        .map(|minute| FakeMessage {
            files: vec![format!("photo{minute}.png")],
            ..FakeMessage::in_dm(minute, 0)
        })
        .collect()
}

/// Every file below `dir`, recursively.
fn files_below(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files_below(&path));
        } else {
            found.push(path);
        }
    }
    found
}

#[tokio::test]
async fn an_encrypted_backup_becomes_one_archive() {
    let mut messages = photos(30);
    messages.extend((40..43).map(|minute| FakeMessage::in_dm(minute, 0)));
    let fake = FakeDiscord::start(State::with_messages(messages.clone())).await;
    let dir = tempfile::tempdir().unwrap();
    let options = encrypted_options(dir.path(), "correct horse battery staple");

    let (summary, events) =
        run_job_within(&fake, &[dm_target()], Filter::default(), options, 120).await;

    assert_eq!((summary.stats.deleted, summary.stats.failed), (33, 0));
    assert!(fake.remaining().is_empty());
    let sealed = events.iter().find_map(|e| match e {
        Event::BackupSealed {
            archive,
            files,
            messages,
        } => Some((archive.clone(), *files, *messages)),
        _ => None,
    });
    let (archive, files, rows) = sealed.expect("the backup was sealed");
    assert_eq!((files, rows), (30, 33));
    // One file is left: the archive. No parts, nothing readable.
    let left = files_below(dir.path());
    assert_eq!(left, std::slice::from_ref(&archive));
    let raw = std::fs::read(&archive).unwrap();
    assert!(!raw.windows(11).any(|w| w == b"contents of"));

    let out = tempfile::tempdir().unwrap();
    let wrong = vault::open(&archive, &vault::secret("wrong"), out.path());
    assert!(wrong.is_err());
    let opened = vault::open(
        &archive,
        &vault::secret("correct horse battery staple"),
        out.path(),
    )
    .unwrap();
    assert_eq!((opened.files, opened.messages), (30, 33));
    let photo = &messages[3];
    let saved = out.path().join(format!(
        "attachments/{DM_CHANNEL}/{}_1_photo3.png",
        photo.id
    ));
    assert_eq!(
        std::fs::read_to_string(saved).unwrap(),
        "contents of photo3.png"
    );
    let list: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(out.path().join("messages.json")).unwrap()).unwrap();
    assert_eq!(list.len(), 33);
}

#[tokio::test]
async fn a_stopped_encrypted_backup_is_finished_by_continuing() {
    let fake = FakeDiscord::start(State::with_messages(photos(40))).await;
    let dir = tempfile::tempdir().unwrap();
    let mut options = encrypted_options(dir.path(), "pass");
    options.delete_delay_ms = 30;
    let settings = options.backup_encryption.clone().unwrap();

    let control = JobControl::new();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let task = {
        let (client, control, options) = (fake.client(), control.clone(), options.clone());
        tokio::spawn(async move {
            job::run(
                &client,
                Snowflake(ME),
                &[dm_target()],
                &Filter::default(),
                &options,
                &control,
                tx,
            )
            .await
        })
    };
    // The first part is sealed after 25 messages; stop in the second.
    wait_until("the first part was deleted", || fake.delete_calls() >= 26).await;
    control.cancel();
    let summary = tokio::time::timeout(Duration::from_secs(60), task)
        .await
        .unwrap()
        .unwrap();
    assert!(summary.cancelled);
    let mut first = Vec::new();
    while let Ok(event) = rx.try_recv() {
        first.push(event);
    }
    assert!(first.iter().any(|e| matches!(e, Event::BackupKept { .. })));
    assert!(settings.parts.join("key.age").exists());

    // Continuing needs the passphrase to finish the backup.
    assert!(settings.unlock(vault::secret("nope")).is_err());
    let keys = settings.unlock(vault::secret("pass")).unwrap();
    let options = JobOptions {
        resume: Some(checkpoint_of(&first)),
        backup_keys: KeySlot(Some(Arc::new(keys))),
        delete_delay_ms: 0,
        ..options
    };
    let (summary, events) =
        run_job_within(&fake, &[dm_target()], Filter::default(), options, 120).await;
    assert!(summary.error.is_none(), "{:?}", summary.error);
    assert!(fake.remaining().is_empty());
    let files = events.iter().find_map(|e| match e {
        Event::BackupSealed { files, .. } => Some(*files),
        _ => None,
    });
    // Each photo once, although some were saved by both runs.
    assert_eq!(files, Some(40));
    assert!(!settings.parts.exists());
}

#[tokio::test]
async fn a_failing_target_keeps_the_encrypted_backup_open_for_continuing() {
    let mut state = State::with_messages(photos(2));
    state.messages.push(FakeMessage::in_guild(5, 0, ME));
    state.forbidden_guilds.push(GUILD);
    let fake = FakeDiscord::start(state).await;
    let dir = tempfile::tempdir().unwrap();
    let options = encrypted_options(dir.path(), "pass");
    let settings = options.backup_encryption.clone().unwrap();

    let (summary, events) = run_job_within(
        &fake,
        &[dm_target(), guild_target()],
        Filter::default(),
        options,
        60,
    )
    .await;

    assert!(summary.error.is_none() && !summary.cancelled);
    assert_eq!(fake.remaining(), vec![id_at(5, 0)]);
    assert!(events.iter().any(|e| matches!(e, Event::BackupKept { .. })));
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::BackupSealed { .. })));
    // The key stays, so the continued run can finish the backup.
    assert!(settings.parts.join("key.age").exists());
    let checkpoint = checkpoint_of(&events);
    assert!(
        checkpoint.is_finished(Snowflake(DM_CHANNEL)) && !checkpoint.is_finished(Snowflake(GUILD))
    );
}

async fn scan_into(fake: &FakeDiscord, cache: &MessageCache, filter: &Filter) -> Vec<ScanEvent> {
    let mut events = Vec::new();
    scan::scan(
        &fake.client(),
        Snowflake(ME),
        &[dm_target()],
        filter,
        &fast(),
        &JobControl::new(),
        cache,
        |event| events.push(event),
    )
    .await
    .unwrap();
    events
}

async fn run_cached(
    fake: &FakeDiscord,
    cache: &MessageCache,
    filter: &Filter,
    options: JobOptions,
) -> Summary {
    let (tx, _rx) = mpsc::unbounded_channel();
    job::run_with_cache(
        &fake.client(),
        Snowflake(ME),
        &[dm_target()],
        filter,
        &options,
        &JobControl::new(),
        Some(cache),
        tx,
    )
    .await
}

#[tokio::test]
async fn messages_found_while_counting_are_not_searched_again() {
    let messages = (0..60)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let cache = MessageCache::new();
    let filter = Filter::default();

    let events = scan_into(&fake, &cache, &filter).await;
    let searches = fake.state.lock().unwrap().search_calls;
    assert_eq!(searches, 4, "three pages of 25 and an empty one");
    assert!(events.iter().any(|e| matches!(
        e,
        ScanEvent::Read {
            matching: 60,
            complete: true,
            ..
        }
    )));
    let stats = events
        .iter()
        .rev()
        .find_map(|e| match e {
            ScanEvent::Stats { stats } => Some(stats.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(stats.messages, 60);

    // Counting again, a dry run: no search at all.
    scan_into(&fake, &cache, &filter).await;
    let dry = run_cached(
        &fake,
        &cache,
        &filter,
        JobOptions {
            dry_run: true,
            ..fast()
        },
    )
    .await;
    assert_eq!(dry.stats.deleted, 60);
    assert_eq!(fake.state.lock().unwrap().search_calls, searches);

    // Deleting: one search to check for anything new.
    let summary = run_cached(&fake, &cache, &filter, fast()).await;
    assert_eq!(summary.stats.deleted, 60);
    assert!(fake.remaining().is_empty());
    assert_eq!(fake.state.lock().unwrap().search_calls, searches + 1);
    assert!(cache
        .lookup(&dm_target(), &filter.search_query(Snowflake(ME)))
        .unwrap()
        .messages
        .is_empty());
}

#[tokio::test]
async fn a_message_sent_after_counting_is_found_by_the_check() {
    let messages = (0..10)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let cache = MessageCache::new();
    let filter = Filter::default();
    scan_into(&fake, &cache, &filter).await;
    fake.state
        .lock()
        .unwrap()
        .messages
        .push(FakeMessage::in_dm(30, 0));

    let summary = run_cached(&fake, &cache, &filter, fast()).await;
    assert_eq!(summary.stats.deleted, 11);
    assert!(fake.remaining().is_empty());
}

#[tokio::test]
async fn a_dry_run_fills_the_cache_for_the_real_run() {
    let messages = (0..30)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let fake = FakeDiscord::start(State::with_messages(messages)).await;
    let cache = MessageCache::new();
    let filter = Filter::default();
    run_cached(
        &fake,
        &cache,
        &filter,
        JobOptions {
            dry_run: true,
            ..fast()
        },
    )
    .await;
    let searches = fake.state.lock().unwrap().search_calls;

    let summary = run_cached(&fake, &cache, &filter, fast()).await;
    assert_eq!(summary.stats.deleted, 30);
    assert_eq!(fake.state.lock().unwrap().search_calls, searches + 1);
}

#[tokio::test]
async fn deleted_messages_the_index_still_returns_do_not_come_back() {
    let messages = (0..40)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let mut state = State::with_messages(messages);
    state.stale_index = true;
    let fake = FakeDiscord::start(state).await;
    let cache = MessageCache::new();
    let filter = Filter::default();
    scan_into(&fake, &cache, &filter).await;

    let summary = run_cached(&fake, &cache, &filter, fast()).await;
    assert_eq!(summary.stats.deleted, 40);
    assert_eq!(fake.delete_calls(), 40, "nothing is deleted twice");

    // Counting again uses what is known: nothing left, although the index
    // still lists all 40.
    let events = scan_into(&fake, &cache, &filter).await;
    assert!(events.iter().any(|e| matches!(
        e,
        ScanEvent::Read {
            matching: 0,
            complete: true,
            ..
        }
    )));
}

#[tokio::test]
async fn a_search_that_ignores_the_cursor_does_not_page_forever() {
    let messages = (0..60)
        .map(|minute| FakeMessage::in_dm(minute, 0))
        .collect();
    let mut state = State::with_messages(messages);
    state.ignore_cursor = true;
    let fake = FakeDiscord::start(state).await;
    let cache = MessageCache::new();
    let filter = Filter::default();

    let scan = tokio::time::timeout(Duration::from_secs(10), scan_into(&fake, &cache, &filter));
    scan.await.expect("counting ended");
    assert!(fake.state.lock().unwrap().search_calls < 10);
    let targets = [dm_target()];
    let run = tokio::time::timeout(
        Duration::from_secs(10),
        run_job(&fake, &targets, Filter::default(), fast()),
    );
    let (summary, _) = run.await.expect("deleting ended");
    assert!(summary.error.is_none());
}

/// A Discord that fails at random, a quarter of the time. Whatever happens,
/// only own messages in the range may be deleted, the run must end, and a
/// second run on a calm Discord deletes the rest.
#[tokio::test]
async fn survives_a_flaky_discord() {
    use rand::SeedableRng;
    for seed in 0..6 {
        let mut messages = Vec::new();
        for minute in 0..80 {
            messages.push(FakeMessage::in_dm(minute, 0));
            messages.push(FakeMessage::in_guild(minute, 1, OTHER));
            messages.push(FakeMessage::in_guild(minute, 2, ME));
        }
        let mut state = State::with_messages(messages.clone());
        state.chaos = Some((rand::rngs::StdRng::seed_from_u64(seed), 0.25));
        let fake = FakeDiscord::start(state).await;
        let cache = MessageCache::new();
        let filter = Filter {
            after: Some(at(10)),
            before: Some(at(70)),
            ..Default::default()
        };
        let targets = [dm_target(), guild_target()];
        let _ = tokio::time::timeout(
            Duration::from_secs(60),
            scan::scan(
                &fake.client(),
                Snowflake(ME),
                &targets,
                &filter,
                &fast(),
                &JobControl::new(),
                &cache,
                |_| {},
            ),
        )
        .await
        .expect("counting ended");
        let (tx, _rx) = mpsc::unbounded_channel();
        let (client, options, control) = (fake.client(), fast(), JobControl::new());
        let run = job::run_with_cache(
            &client,
            Snowflake(ME),
            &targets,
            &filter,
            &options,
            &control,
            Some(&cache),
            tx,
        );
        tokio::time::timeout(Duration::from_secs(60), run)
            .await
            .expect("deleting ended");

        let in_range = |m: &FakeMessage| {
            m.author_id == ME && (10..70).any(|min| m.id >> 22 == id_at(min, 0) >> 22)
        };
        let remaining = fake.remaining();
        for m in &messages {
            if !in_range(m) {
                assert!(
                    remaining.contains(&m.id),
                    "seed {seed}: deleted a message it must not touch"
                );
            }
        }
        // Calm again: the rest goes.
        fake.state.lock().unwrap().chaos = None;
        let (tx, _rx) = mpsc::unbounded_channel();
        job::run_with_cache(
            &fake.client(),
            Snowflake(ME),
            &targets,
            &filter,
            &fast(),
            &JobControl::new(),
            Some(&cache),
            tx,
        )
        .await;
        let left: Vec<_> = fake
            .remaining()
            .into_iter()
            .filter(|id| messages.iter().any(|m| m.id == *id && in_range(m)))
            .collect();
        assert!(
            left.is_empty(),
            "seed {seed}: {} left after a calm run",
            left.len()
        );
    }
}
