#[allow(dead_code)]
mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use common::*;
use kebbie_hub::RateLimit;
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::{MachineState, join};
use kebbie_hub_fakes::{HubClient, HubError};
use kebbie_hub_protocol::{
    DeviceCert, DeviceKey, DeviceKind,
    Header, Id, Kind, Payload, Prefix, RunRoutine, SecretKeys, seal,
};
use serde_json::Value;

async fn reader(test: &TestHub) -> (Browser, MachineState) {
    let mut person = member(test).await;
    let machine = connected_with_board(test, &mut person).await;
    (person, machine)
}

#[tokio::test]
async fn the_account_opens_the_board_its_machine_posted() {
    let test = start_hub(|_| {}).await;
    let (mut person, mut machine) = reader(&test).await;
    let rev = machine.post_board(board(MARKER)).await.unwrap();
    let boards = person.boards().await.unwrap();
    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0].machine, machine.machine);
    assert_eq!(boards[0].rev, Some(rev));
    let opened = boards[0].board.as_ref().unwrap();
    let mut expected = board(MARKER);
    expected.rev = rev;
    expected.accepts = Some(machine.accepts.clone());
    assert_eq!(opened, &expected);

    let inbox = machine.poll_once(0).await.unwrap();
    assert!(inbox.messages.is_empty());
    assert_eq!(inbox.board_rev, Some(rev));
}

#[tokio::test]
async fn the_hub_refuses_a_board_older_than_the_one_it_holds() {
    let test = start_hub(|_| {}).await;
    let (mut person, mut machine) = reader(&test).await;

    let at = kebbie_hub::now();
    let five = machine.seal_board(board("five"), 5, at).unwrap();
    machine.upload_board(5, five).await.unwrap();
    for (rev, signed_at, refusal) in [
        (4, at, "stale-revision"),
        (5, at, "stale-revision"),
        (9, at - 1, "replayed-envelope"),
    ] {
        let envelope = machine.seal_board(board("stale"), rev, signed_at).unwrap();
        assert_eq!(
            hub_refusal(machine.upload_board(rev, envelope).await),
            (409, refusal.to_string()),
            "rev {rev} signed at {signed_at}"
        );
    }
    let six = machine.seal_board(board("six"), 6, at).unwrap();
    assert_eq!(machine.upload_board(6, six).await.unwrap(), 6);
    let boards = person.boards().await.unwrap();
    assert_eq!(boards[0].rev, Some(6));
    assert_eq!(
        boards[0].board.as_ref().unwrap().items[0].summary,
        "six summary"
    );
}

fn hub_refusal<T: std::fmt::Debug>(result: Result<T, kebbie_hub_fakes::Error>) -> (u16, String) {
    match result {
        Err(kebbie_hub_fakes::Error::Hub(HubError::Status { status, code })) => (status, code),
        other => panic!("the hub did not refuse: {other:?}"),
    }
}

#[tokio::test]
async fn a_replayed_board_with_a_huge_rev_is_refused_and_the_machine_carries_on() {
    let test = start_hub(|_| {}).await;
    let (mut person, mut machine) = reader(&test).await;
    let at = kebbie_hub::now();
    let older = machine.seal_board(board("older"), 1, at - 60).unwrap();
    machine.upload_board(1, older.clone()).await.unwrap();
    let current = machine.seal_board(board("current"), 2, at).unwrap();
    machine.upload_board(2, current.clone()).await.unwrap();

    let huge = i64::MAX as u64;
    for replay in [older, current] {
        assert_eq!(
            hub_refusal(machine.upload_board(huge, replay).await),
            (409, "replayed-envelope".to_string())
        );
    }
    let next = machine.seal_board(board("next"), 3, at).unwrap();
    assert_eq!(machine.upload_board(3, next).await.unwrap(), 3);
    let boards = person.boards().await.unwrap();
    assert_eq!(
        (boards[0].rev, boards[0].board.as_ref().unwrap().rev),
        (Some(3), 3)
    );
}

#[tokio::test]
async fn an_envelope_posted_ahead_of_the_machine_with_a_huge_rev_does_not_freeze_it() {
    let test = start_hub(|_| {}).await;
    let (mut person, mut machine) = reader(&test).await;
    let at = kebbie_hub::now();
    let caught = machine.seal_board(board("caught"), 1, at - 5).unwrap();
    machine.upload_board(i64::MAX as u64, caught).await.unwrap();
    let next = machine.seal_board(board("next"), 2, at).unwrap();
    assert_eq!(machine.upload_board(2, next).await.unwrap(), 2);
    let boards = person.boards().await.unwrap();
    assert_eq!(boards[0].rev, Some(2));
    assert_eq!(
        boards[0].board.as_ref().unwrap().items[0].summary,
        "next summary"
    );
}

#[tokio::test]
async fn numbers_past_what_sqlite_holds_are_refused_with_400() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let envelope = machine
        .seal_board(board(MARKER), 1, kebbie_hub::now())
        .unwrap();
    assert_eq!(
        hub_refusal(machine.upload_board(u64::MAX, envelope).await),
        (400, "rev-out-of-range".to_string())
    );
    let inbox = machine
        .client()
        .get::<Value>("/v1/m/inbox", &[("after", u64::MAX.to_string())])
        .await;
    assert_eq!(
        inbox.unwrap_err(),
        HubError::Status {
            status: 400,
            code: "after-out-of-range".to_string()
        }
    );
}

#[tokio::test]
async fn behind_cloudflare_the_limit_follows_the_connecting_ip_and_its_64() {
    let test = start_hub(|config| {
        config.trusted_proxy = Some(kebbie_hub::TrustedProxy::Cloudflare);
        config.rate = RateLimit {
            burst: 3,
            per_minute: 1,
        };
    })
    .await;
    let machine = joined_machine(&test).await;
    let forged = HubClient::new(&test.url, Some("khm_guess"));
    let from = |address: &str| forged.with_header("cf-connecting-ip", address);
    for address in ["198.51.100.1", "2001:db8:1:2::1"] {
        let statuses = statuses_of(&from(address), 4).await;
        assert_eq!(statuses, [401, 401, 401, 429], "{address}");
    }
    assert_eq!(statuses_of(&from("198.51.100.2"), 1).await, [401]);
    assert_eq!(statuses_of(&from("2001:db8:1:2:ffff::9"), 1).await, [429]);
    assert_eq!(statuses_of(&from("2001:db8:1:3::1"), 1).await, [401]);

    let flooded = machine
        .client()
        .with_header("cf-connecting-ip", "198.51.100.1");
    assert_eq!(
        status_of(flooded.get::<Value>("/v1/m/inbox", &[]).await),
        429
    );
    let elsewhere = machine
        .client()
        .with_header("cf-connecting-ip", "198.51.100.3");
    elsewhere.get::<Value>("/v1/m/inbox", &[]).await.unwrap();
}

#[tokio::test]
async fn without_a_trusted_proxy_the_cloudflare_header_is_ignored() {
    let test = start_hub(|config| {
        config.rate = RateLimit {
            burst: 3,
            per_minute: 1,
        };
    })
    .await;
    let forged = HubClient::new(&test.url, Some("khm_guess"));
    let mut statuses = Vec::new();
    for address in [
        "198.51.100.1",
        "198.51.100.2",
        "198.51.100.3",
        "198.51.100.4",
    ] {
        statuses.extend(statuses_of(&forged.with_header("cf-connecting-ip", address), 1).await);
    }
    assert_eq!(statuses, [401, 401, 401, 429]);
}

#[tokio::test]
async fn another_account_sees_no_board_of_my_machine() {
    let test = start_hub(|_| {}).await;
    let (mut person, mut machine) = reader(&test).await;
    machine.post_board(board(MARKER)).await.unwrap();
    assert_eq!(person.boards().await.unwrap().len(), 1);
    let mut stranger = member(&test).await;
    let raw: Value = stranger.get("/v1/a/boards").await.unwrap();
    assert_eq!(raw, serde_json::json!({ "boards": [], "list": null }));
}

#[tokio::test]
async fn a_board_sealed_before_the_account_was_pinned_is_listed_but_not_readable() {
    let test = start_hub(|_| {}).await;
    let (mut person, mut machine) = reader(&test).await;
    let pinned = machine.account.take();
    machine.post_board(board(MARKER)).await.unwrap();
    let boards = person.boards().await.unwrap();
    assert!(matches!(
        boards[0].board,
        Err(kebbie_hub_fakes::browser::BoardProblem::NotSealedToTheAccount)
    ));
    machine.account = pinned;
    machine.post_board(board(MARKER)).await.unwrap();
    assert!(person.boards().await.unwrap()[0].board.is_ok());
}

#[tokio::test]
async fn requests_without_a_valid_token_get_401_and_no_cors_grant() {
    let test = start_hub(|_| {}).await;
    let machine = joined_machine(&test).await;

    let anonymous = HubClient::new(&test.url, None);
    let forged = HubClient::new(
        &test.url,
        Some("khm_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    );
    for client in [&anonymous, &forged] {
        assert_eq!(
            status_of(client.get::<Value>("/v1/m/inbox", &[]).await),
            401
        );
        assert_eq!(
            status_of(client.get::<Value>("/v1/m/pairings/none", &[]).await),
            401
        );
    }
    assert_eq!(
        status_of(HubClient::new(&test.url, None).post::<_, Value>(
            "/v1/m/join",
            &serde_json::json!({ "code": "0000-0000", "name": "x", "keys": machine.public() })
        ).await),
        401
    );

    let response = reqwest::Client::new()
        .get(format!("{}/v1/m/inbox", test.url))
        .header("origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        response
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
}

#[tokio::test]
async fn a_token_past_its_rate_limit_gets_429() {
    let test = start_hub(|config| {
        config.rate = RateLimit {
            burst: 5,
            per_minute: 1,
        }
    })
    .await;
    let mut machine = joined_machine(&test).await;
    for _ in 0..5 {
        machine.poll_once(0).await.unwrap();
    }
    match machine.poll_once(0).await {
        Err(kebbie_hub_fakes::Error::Hub(error)) => assert_eq!(error.status(), Some(429)),
        other => panic!("the sixth poll went through: {other:?}"),
    }
    let forged = HubClient::new(&test.url, Some("khm_guess"));
    let statuses: Vec<u16> = statuses_of(&forged, 6).await;
    assert!(statuses.contains(&429), "{statuses:?}");
}

async fn statuses_of(client: &HubClient, count: usize) -> Vec<u16> {
    let mut statuses = Vec::new();
    for _ in 0..count {
        statuses.push(status_of(client.get::<Value>("/v1/m/inbox", &[]).await));
    }
    statuses
}

#[tokio::test]
async fn a_board_not_signed_by_the_machine_is_refused() {
    let test = start_hub(|_| {}).await;
    let machine = joined_machine(&test).await;
    let impostor = SecretKeys::generate();
    let header = Header::new(
        Id::new(Prefix::Board).unwrap(),
        Kind::Board,
        machine.machine.clone(),
        machine.machine.clone(),
        kebbie_hub::now(),
    );
    let envelope = seal(
        header,
        &Payload::Board(board(MARKER)),
        &impostor,
        &[(machine.machine.clone(), impostor.public())],
    )
    .unwrap();
    let refused = machine
        .client()
        .put::<_, Value>(
            "/v1/m/board",
            &kebbie_hub_api::BoardUpload { rev: 1, envelope },
        )
        .await;
    assert_eq!(
        refused.unwrap_err(),
        HubError::Status {
            status: 400,
            code: "bad-signature".to_string()
        }
    );
}

#[tokio::test]
async fn the_database_never_holds_board_text() {
    let test = start_hub(|_| {}).await;
    let (mut person, mut machine) = reader(&test).await;
    machine.post_board(board(MARKER)).await.unwrap();
    let boards = person.boards().await.unwrap();
    assert!(
        boards[0].board.as_ref().unwrap().items[0]
            .summary
            .contains(MARKER)
    );

    let stored = database_bytes(&test);
    assert!(
        holds(&stored, machine.machine.as_str().as_bytes()),
        "the scan did not read the database"
    );
    let session = person.cookie.clone().unwrap();
    let session = session.split_once('=').unwrap().1;
    for leak in [
        MARKER,
        "refunds@shop",
        "Keep the old endpoint",
        "Nightly deps",
        session,
    ] {
        assert!(
            !holds(&stored, leak.as_bytes()),
            "the database holds {leak:?}"
        );
    }
    let (signing, encryption) = person.account_secret().unwrap();
    assert!(!holds(&stored, &signing) && !holds(&stored, &encryption), "the account's private keys stay out of the database");
    assert!(holds(&stored, &person.account_key().unwrap().keys.encryption), "the account's public key is kept");
}

#[tokio::test]
async fn a_join_code_works_once() {
    let test = start_hub(|_| {}).await;
    let code = test.hub.mint_join_code().unwrap();
    join(&test.url, &code, "mac").await.unwrap();
    match join(&test.url, &code, "mac").await {
        Err(kebbie_hub_fakes::Error::Hub(error)) => assert_eq!(error.status(), Some(401)),
        other => panic!(
            "the code worked twice: {:?}",
            other.map(|state| state.machine)
        ),
    }
}

#[tokio::test]
async fn oversized_bodies_are_refused() {
    let test = start_hub(|_| {}).await;
    let response = reqwest::Client::new()
        .post(format!("{}/v1/m/join", test.url))
        .header("content-type", "application/json")
        .body(format!("{{\"code\":\"{}\"}}", "A".repeat(64 * 1024)))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn the_inbox_hands_over_queued_envelopes_until_they_are_acknowledged() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let account = Id::account([7; 16]);
    let account_keys = SecretKeys::generate();
    let (machine_id, machine_keys) = (machine.machine.clone(), machine.public());
    let phone = DeviceKey::generate();
    let cert = DeviceCert::issue(&account_keys, &account, Id::new(Prefix::Device).unwrap(), "iPhone", DeviceKind::Web, phone.public(), 0).unwrap();
    let command = |at| {
        seal(
            Header::command(
                Id::new(Prefix::Command).unwrap(),
                Kind::Routine,
                cert.clone(),
                machine_id.clone(),
                at,
            ),
            &Payload::Routine(RunRoutine {
                id: "nightly-deps".into(),
            }),
            &account_keys,
            &[(machine_id.clone(), machine_keys)],
        )
        .unwrap()
    };
    let first = command(kebbie_hub::now());
    test.hub.enqueue(&machine.machine, &first).unwrap();
    let inbox = machine.poll_once(0).await.unwrap();
    assert_eq!(inbox.messages.len(), 1);
    assert_eq!(inbox.messages[0].envelope, first);
    assert_eq!(inbox.seq, 1);

    let hub = Arc::clone(&test.hub);
    let target = machine.machine.clone();
    let second = command(kebbie_hub::now());
    let queued = second.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        hub.enqueue(&target, &queued).unwrap();
    });
    let started = std::time::Instant::now();
    let inbox = machine.poll_once(5).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the long-poll did not wake"
    );
    assert_eq!(inbox.messages.len(), 1);
    assert_eq!(inbox.messages[0].envelope, second);
    assert_eq!(inbox.seq, 2);

    let inbox = machine.poll_once(0).await.unwrap();
    assert!(inbox.messages.is_empty());
    assert!(
        test.hub
            .store()
            .queued_after(&machine.machine, 0)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_machine_renames_itself_and_only_itself() {
    let test = start_hub(|_| {}).await;
    let (mut person, mut machine) = reader(&test).await;
    let other = joined_machine(&test).await;
    machine.post_board(board(MARKER)).await.unwrap();
    assert_eq!(person.boards().await.unwrap()[0].name, "mac");

    assert_eq!(machine.rename("  studio ").await.unwrap(), "studio");
    assert_eq!(person.boards().await.unwrap()[0].name, "studio");
    let (renamed_to, other_name, renamed) = {
        let store = test.hub.store();
        (
            store.machine_name(&machine.machine).unwrap(),
            store.machine_name(&other.machine).unwrap(),
            store
                .events()
                .unwrap()
                .iter()
                .filter(|(kind, _)| kind == "machine.renamed")
                .count(),
        )
    };
    assert_eq!(renamed_to.as_deref(), Some("studio"));
    assert_eq!(other_name.as_deref(), Some("mac"));
    assert_eq!(renamed, 1);
    machine.rename("studio").await.unwrap();
    let again = test
        .hub
        .store()
        .events()
        .unwrap()
        .iter()
        .filter(|(kind, _)| kind == "machine.renamed")
        .count();
    assert_eq!(again, 1, "the same name logs nothing");

    for bad in [" ".to_string(), "x".repeat(65), "a\u{7}b".to_string()] {
        assert_eq!(
            hub_refusal(machine.rename(&bad).await),
            (400, "bad-name".to_string()),
            "{bad:?}"
        );
    }
    let anonymous = kebbie_hub_fakes::HubClient::new(&test.url, None);
    let refused = anonymous
        .put::<_, Value>("/v1/m/name", &serde_json::json!({ "name": "evil" }))
        .await
        .unwrap_err();
    assert!(
        matches!(refused, HubError::Status { status: 401, .. }),
        "{refused:?}"
    );
    assert_eq!(
        test.hub
            .store()
            .machine_name(&machine.machine)
            .unwrap()
            .as_deref(),
        Some("studio")
    );
}

async fn signed_in_handle(
    State(hub): State<Arc<kebbie_hub::Hub>>,
    kebbie_hub::AccountAuth(account): kebbie_hub::AccountAuth,
) -> String {
    hub.store().account(&account).unwrap().unwrap().handle
}

#[tokio::test]
async fn an_embedding_binary_adds_routes_that_read_the_hubs_state_and_sessions() {
    let extra = axum::Router::new().route("/v1/x/handle", axum::routing::get(signed_in_handle));
    let test = start_hub_with(|_| {}, None, extra).await;
    let person = member(&test).await;
    let answer = reqwest::Client::new()
        .get(format!("{}/v1/x/handle", test.url))
        .header("cookie", person.cookie.as_deref().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(answer.status(), 200);
    assert_eq!(answer.text().await.unwrap(), person.handle);
    let anonymous = reqwest::get(format!("{}/v1/x/handle", test.url)).await.unwrap();
    assert_eq!(anonymous.status(), 401);
    let health = reqwest::get(format!("{}/healthz", test.url)).await.unwrap();
    assert_eq!(health.status(), 204, "the hub's own routes stay");
}
