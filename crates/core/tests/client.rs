mod common;

use std::sync::{Arc, Mutex};

use common::*;
use purgecord_core::search::{Scope, SearchQuery};
use purgecord_core::{list_targets, Client, Error, Has, Notice, Snowflake, TargetKind};
use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn collect_notices(client: &Client) -> Arc<Mutex<Vec<Notice>>> {
    let notices = Arc::new(Mutex::new(Vec::new()));
    let sink = notices.clone();
    client.set_notice_sink(Some(Arc::new(move |n| sink.lock().unwrap().push(n))));
    notices
}

#[tokio::test]
async fn sends_the_token_and_search_parameters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/api/v9/guilds/{GUILD}/messages/search")))
        .and(header("authorization", TOKEN))
        .and(query_param("author_id", ME.to_string()))
        .and(query_param("min_id", "5"))
        .and(query_param("max_id", "9"))
        .and(query_param("sort_order", "desc"))
        .and(query_param("content", "hello world"))
        .and(query_param("has", "link"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "total_results": 3, "messages": [] })),
        )
        .expect(1)
        .mount(&server)
        .await;
    let query = SearchQuery {
        author_id: Some(Snowflake(ME)),
        min_id: Some(Snowflake(5)),
        max_id: Some(Snowflake(9)),
        content: Some("hello world".into()),
        has: vec![Has::Link],
    };

    let response = client_for(&server)
        .search(Scope::Guild(Snowflake(GUILD)), &query)
        .await
        .unwrap();

    assert_eq!(response.total_results, 3);
}

#[tokio::test]
async fn waits_out_rate_limits() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(json!({ "retry_after": 0.05, "global": false })),
        )
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_for(&server);
    let notices = collect_notices(&client);

    client
        .delete_message(Snowflake(1), Snowflake(2))
        .await
        .unwrap();

    let notices = notices.lock().unwrap();
    assert_eq!(notices.len(), 2);
    assert!(matches!(
        notices[0],
        Notice::RateLimited { global: false, .. }
    ));
}

#[tokio::test]
async fn waits_while_the_search_index_is_built() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "message": "Index not yet available. Try again later",
            "code": 110000,
            "retry_after": 0.05,
        })))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({ "total_results": 7, "messages": [] })),
        )
        .mount(&server)
        .await;
    let client = client_for(&server);
    let notices = collect_notices(&client);

    let response = client
        .search(
            Scope::Channel(Snowflake(DM_CHANNEL)),
            &SearchQuery::default(),
        )
        .await
        .unwrap();

    assert_eq!(response.total_results, 7);
    assert!(matches!(
        notices.lock().unwrap()[..],
        [Notice::IndexNotReady { .. }]
    ));
}

#[tokio::test]
async fn retries_server_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(502))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(user_json(ME, "me")))
        .mount(&server)
        .await;

    let me = client_for(&server).current_user().await.unwrap();

    assert_eq!(me.id, Snowflake(ME));
}

#[tokio::test]
async fn reports_discord_error_codes() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(json!({ "message": "Missing Permissions", "code": 50013 })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let err = client_for(&server)
        .delete_message(Snowflake(1), Snowflake(2))
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        Error::Api {
            status: 403,
            code: Some(50013),
            ..
        }
    ));
}

#[tokio::test]
async fn rejected_token_is_reported_as_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({ "message": "401: Unauthorized", "code": 0 })),
        )
        .mount(&server)
        .await;

    let err = client_for(&server).current_user().await.unwrap_err();

    assert!(matches!(err, Error::Unauthorized));
}

#[test]
fn token_is_cleaned_up_and_validated() {
    assert!(matches!(Client::new("   "), Err(Error::InvalidToken)));
    assert!(matches!(Client::new("abc\ndef"), Err(Error::InvalidToken)));
    assert!(Client::new(" \"abc.def\" ").is_ok());
    assert!(!format!("{:?}", Client::new("secret-token").unwrap()).contains("secret"));
}

#[tokio::test]
async fn lists_servers_then_dms() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v9/users/@me/guilds"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "2", "name": "zeta", "icon": null },
            { "id": "1", "name": "Alpha", "icon": "abc" },
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v9/users/@me/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "id": "30", "type": 1, "last_message_id": "100",
              "recipients": [{ "id": "7", "username": "bob", "global_name": "Bob", "avatar": "av" }] },
            { "id": "31", "type": 3, "name": null, "last_message_id": "200",
              "recipients": [{ "id": "8", "username": "ann" }, { "id": "9", "username": "cy" }] },
            { "id": "32", "type": 0, "name": "not a dm" },
        ])))
        .mount(&server)
        .await;

    let targets = list_targets(&client_for(&server)).await.unwrap();

    let summary: Vec<(TargetKind, u64, &str)> = targets
        .iter()
        .map(|t| (t.kind, t.id.0, t.name.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            (TargetKind::Guild, 1, "Alpha"),
            (TargetKind::Guild, 2, "zeta"),
            (TargetKind::GroupDm, 31, "ann, cy"),
            (TargetKind::Dm, 30, "Bob"),
        ]
    );
    assert_eq!(
        targets[0].icon_url.as_deref(),
        Some("https://cdn.discordapp.com/icons/1/abc.png?size=64")
    );
    assert_eq!(
        targets[3].icon_url.as_deref(),
        Some("https://cdn.discordapp.com/avatars/7/av.png?size=64")
    );
}
