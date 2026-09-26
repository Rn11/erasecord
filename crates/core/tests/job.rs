mod common;

use std::time::Duration;

use common::*;
use purgecord_core::package::{PackageChannel, PackageMessage};
use purgecord_core::{
    job, Event, Filter, Has, JobControl, JobOptions, Package, SkipReason, Snowflake, Summary,
    Target, TargetKind,
};
use tokio::sync::mpsc;
use wiremock::matchers::any;
use wiremock::{Mock, ResponseTemplate};

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

    tokio::time::sleep(Duration::from_millis(100)).await;
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
    state.delete_delay = Duration::from_millis(300);
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

    // The first delete has reached the server but not answered yet.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(fake.delete_calls(), 1);
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
            }),
        }
    }
    for channel in &mut channels {
        channel.messages.sort_by_key(|m| std::cmp::Reverse(m.id));
    }
    Package { channels }
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
