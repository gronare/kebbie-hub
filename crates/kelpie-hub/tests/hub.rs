#[allow(dead_code)]
mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::response::Response;
use common::*;
use kelpie_hub::RateLimit;
use kelpie_hub_api::{
    Bytes32, CommitRequest, Committed, PairingFailure, PairingOpened, PairingState, PairingView,
};
use kelpie_hub_fakes::machine::join;
use kelpie_hub_fakes::phone::PhoneState;
use kelpie_hub_fakes::{HubClient, HubError, PairError};
use kelpie_hub_protocol::{
    Header, Id, Kind, Payload, Prefix, PublicKeys, RunRoutine, SecretKeys, seal,
};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[derive(Clone)]
enum Swap {
    MachineKeysToPhone(PublicKeys),
    DeviceKeysToMachine(PublicKeys),
}

#[derive(Clone)]
struct Relay {
    target: String,
    swap: Swap,
    http: reqwest::Client,
}

fn swap_keys(body: &[u8], field: &str, keys: &PublicKeys) -> Vec<u8> {
    let mut json: Value = serde_json::from_slice(body).unwrap();
    if !json[field].is_null() {
        json[field] = serde_json::to_value(keys).unwrap();
    }
    serde_json::to_vec(&json).unwrap()
}

async fn relay(State(relay): State<Relay>, request: Request) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let target = format!(
        "{}{}",
        relay.target,
        request.uri().path_and_query().map_or("", |pq| pq.as_str())
    );
    let headers = request.headers().clone();
    let mut body = to_bytes(request.into_body(), 1 << 20)
        .await
        .unwrap()
        .to_vec();
    if let Swap::DeviceKeysToMachine(keys) = &relay.swap
        && path.ends_with("/reveal")
    {
        body = swap_keys(&body, "keys", keys);
    }
    let mut forward = relay.http.request(method.clone(), target).body(body);
    for name in ["authorization", "content-type"] {
        if let Some(value) = headers.get(name) {
            forward = forward.header(name, value);
        }
    }
    let answer = forward.send().await.unwrap();
    let status = answer.status();
    let mut body = answer.bytes().await.unwrap().to_vec();
    if let Swap::MachineKeysToPhone(keys) = &relay.swap
        && method == Method::GET
        && path.starts_with("/v1/d/pairings/")
        && status.is_success()
    {
        body = swap_keys(&body, "machine_keys", keys);
    }
    Response::builder()
        .status(status.as_u16())
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

async fn start_relay(target: &str, swap: Swap) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().fallback(relay).with_state(Relay {
        target: target.to_string(),
        swap,
        http: reqwest::Client::new(),
    });
    tokio::spawn(async move { axum::serve(listener, app).await });
    url
}

#[tokio::test]
async fn a_paired_phone_opens_the_board_its_machine_posted() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let mut phone = phone(&test).await;
    let (admitted, paired) = pair_both(&mut machine, &mut phone, true).await;
    let device = admitted.unwrap();
    assert_eq!(paired.unwrap(), machine.machine);
    assert_eq!(phone.device.as_ref(), Some(&device));
    assert_eq!(machine.devices[0].keys, phone.public());
    assert_eq!(phone.machines[0].keys, machine.public());

    let rev = machine.post_board(board(MARKER)).await.unwrap();
    let boards = phone.boards().await.unwrap();
    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0].machine, machine.machine);
    assert_eq!(boards[0].rev, rev);
    let opened = boards[0].board.as_ref().unwrap();
    let mut expected = board(MARKER);
    expected.rev = rev;
    assert_eq!(opened, &expected);

    let inbox = machine.poll_once(0).await.unwrap();
    assert!(inbox.messages.is_empty());
    assert_eq!(inbox.board_rev, Some(rev));
}

#[tokio::test]
async fn a_relay_that_swaps_the_machine_key_shows_different_digits_and_the_machine_refuses() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let substitute = SecretKeys::generate().public();
    let relay = start_relay(&test.url, Swap::MachineKeysToPhone(substitute)).await;
    let mut phone = phone_at(&test, &relay).await;
    let (admitted, paired) = pair_both(&mut machine, &mut phone, true).await;
    assert!(matches!(admitted, Err(PairError::Declined)), "{admitted:?}");
    assert!(matches!(paired, Err(PairError::Declined)), "{paired:?}");
    assert!(machine.devices.is_empty());
    assert!(phone.machines.is_empty());
    assert_eq!(phone.attempts.len(), 1);
    machine.post_board(board(MARKER)).await.unwrap();
    assert!(phone.boards().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_relay_that_swaps_the_device_key_breaks_the_commitment_and_the_machine_refuses() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let substitute = SecretKeys::generate().public();
    let relay = start_relay(&test.url, Swap::DeviceKeysToMachine(substitute)).await;
    let mut phone = phone_at(&test, &relay).await;
    let (admitted, paired) = pair_both(&mut machine, &mut phone, true).await;
    assert!(matches!(admitted, Err(PairError::Mismatch)), "{admitted:?}");
    assert!(
        matches!(
            paired,
            Err(PairError::Ended {
                state: PairingState::Refused,
                reason: Some(PairingFailure::Mismatch)
            })
        ),
        "{paired:?}"
    );
    assert!(machine.devices.is_empty());
    assert!(phone.machines.is_empty());
}

#[tokio::test]
async fn the_hub_refuses_a_board_older_than_the_one_it_holds() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let mut phone = phone(&test).await;
    let (admitted, _) = pair_both(&mut machine, &mut phone, true).await;
    admitted.unwrap();

    let at = kelpie_hub::now();
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
    let boards = phone.boards().await.unwrap();
    assert_eq!(boards[0].rev, 6);
    assert_eq!(
        boards[0].board.as_ref().unwrap().items[0].summary,
        "six summary"
    );
}

fn hub_refusal<T: std::fmt::Debug>(result: Result<T, kelpie_hub_fakes::Error>) -> (u16, String) {
    match result {
        Err(kelpie_hub_fakes::Error::Hub(HubError::Status { status, code })) => (status, code),
        other => panic!("the hub did not refuse: {other:?}"),
    }
}

#[tokio::test]
async fn a_replayed_board_with_a_huge_rev_is_refused_and_the_machine_carries_on() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let mut phone = phone(&test).await;
    pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    let at = kelpie_hub::now();
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
    let boards = phone.boards().await.unwrap();
    assert_eq!(
        (boards[0].rev, boards[0].board.as_ref().unwrap().rev),
        (3, 3)
    );
}

#[tokio::test]
async fn an_envelope_posted_ahead_of_the_machine_with_a_huge_rev_does_not_freeze_it() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let mut phone = phone(&test).await;
    pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    let at = kelpie_hub::now();
    let caught = machine.seal_board(board("caught"), 1, at - 5).unwrap();
    machine.upload_board(i64::MAX as u64, caught).await.unwrap();
    let next = machine.seal_board(board("next"), 2, at).unwrap();
    assert_eq!(machine.upload_board(2, next).await.unwrap(), 2);
    let boards = phone.boards().await.unwrap();
    assert_eq!(boards[0].rev, 2);
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
        .seal_board(board(MARKER), 1, kelpie_hub::now())
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
async fn a_refused_reveal_leaves_the_device_free_to_pair_with_its_real_keys() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let substitute = SecretKeys::generate().public();
    let relay = start_relay(&test.url, Swap::DeviceKeysToMachine(substitute)).await;
    let mut phone = phone_at(&test, &relay).await;
    let (refused, _) = pair_both(&mut machine, &mut phone, true).await;
    assert!(matches!(refused, Err(PairError::Mismatch)), "{refused:?}");
    let device = phone.device.clone().unwrap();

    phone.hub = test.url.clone();
    let (admitted, paired) = pair_both(&mut machine, &mut phone, true).await;
    assert_eq!(admitted.unwrap(), device);
    assert_eq!(paired.unwrap(), machine.machine);
    machine.post_board(board(MARKER)).await.unwrap();
    assert!(phone.boards().await.unwrap()[0].board.is_ok());
}

#[tokio::test]
async fn retrying_the_same_commitment_with_the_same_token_is_idempotent() {
    let test = start_hub(|_| {}).await;
    let machine = joined_machine(&test).await;
    let registered = signed_up(&test).await.register_device().await;
    let device = device_client(&test, &registered);
    let opened: PairingOpened = machine.client().post("/v1/m/pairings", &()).await.unwrap();
    let commit = |seed: u8| CommitRequest {
        code: opened.code.clone(),
        commitment: Bytes32([seed; 32]),
    };
    let once: Committed = device.post("/v1/d/pairings", &commit(2)).await.unwrap();
    let again: Committed = device.post("/v1/d/pairings", &commit(2)).await.unwrap();
    assert_eq!(
        (once.pairing.as_str(), once.device.as_str()),
        (again.pairing.as_str(), again.device.as_str())
    );
    assert_eq!(once.device, registered.device);
    assert_eq!(once.token, None);
    let path = format!("/v1/m/pairings/{}", opened.pairing);
    let view: PairingView = machine.client().get(&path, &[]).await.unwrap();
    assert_eq!(view.state, PairingState::Committed);

    let other = device
        .post::<_, Committed>("/v1/d/pairings", &commit(3))
        .await;
    assert_eq!(status_of(other), 409);
    let view: PairingView = machine.client().get(&path, &[]).await.unwrap();
    assert_eq!(view.reason, Some(PairingFailure::SecondCommitment));
}

#[tokio::test]
async fn behind_cloudflare_the_limit_follows_the_connecting_ip_and_its_64() {
    let test = start_hub(|config| {
        config.trusted_proxy = Some(kelpie_hub::TrustedProxy::Cloudflare);
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
async fn a_phone_the_machine_did_not_admit_sees_no_board() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let mut admitted_phone = phone(&test).await;
    pair_both(&mut machine, &mut admitted_phone, true)
        .await
        .0
        .unwrap();
    let mut other_phone = phone(&test).await;
    let (refused, _) = pair_both(&mut machine, &mut other_phone, false).await;
    assert!(matches!(refused, Err(PairError::Declined)));
    assert!(other_phone.device.is_some());

    machine.post_board(board(MARKER)).await.unwrap();
    assert_eq!(admitted_phone.boards().await.unwrap().len(), 1);
    assert!(other_phone.boards().await.unwrap().is_empty());
    let raw: Value = other_phone.client().get("/v1/d/boards", &[]).await.unwrap();
    assert_eq!(raw, serde_json::json!({ "boards": [] }));
}

#[tokio::test]
async fn a_board_sealed_before_a_phone_paired_is_listed_but_not_readable_by_it() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    machine.post_board(board(MARKER)).await.unwrap();
    let mut phone = phone(&test).await;
    pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    let boards = phone.boards().await.unwrap();
    assert!(matches!(
        boards[0].board,
        Err(kelpie_hub_fakes::phone::BoardProblem::NotSealedToThisDevice)
    ));
}

#[tokio::test]
async fn requests_without_a_valid_token_get_401_and_no_cors_grant() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let mut phone = phone(&test).await;
    pair_both(&mut machine, &mut phone, true).await.0.unwrap();

    let anonymous = HubClient::new(&test.url, None);
    let forged = HubClient::new(
        &test.url,
        Some("khd_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    );
    for client in [&anonymous, &forged] {
        assert_eq!(
            status_of(client.get::<Value>("/v1/d/boards", &[]).await),
            401
        );
        assert_eq!(
            status_of(client.get::<Value>("/v1/m/inbox", &[]).await),
            401
        );
        assert_eq!(
            status_of(client.post::<_, Value>("/v1/m/pairings", &()).await),
            401
        );
    }
    assert_eq!(
        status_of(machine.client().get::<Value>("/v1/d/boards", &[]).await),
        401
    );
    assert_eq!(
        status_of(phone.client().get::<Value>("/v1/m/inbox", &[]).await),
        401
    );
    assert_eq!(
        status_of(HubClient::new(&test.url, None).post::<_, Value>(
            "/v1/m/join",
            &serde_json::json!({ "code": "0000-0000", "name": "x", "keys": machine.public() })
        ).await),
        401
    );

    let response = reqwest::Client::new()
        .get(format!("{}/v1/d/boards", test.url))
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
        Err(kelpie_hub_fakes::Error::Hub(error)) => assert_eq!(error.status(), Some(429)),
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
        kelpie_hub::now(),
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
            &kelpie_hub_api::BoardUpload { rev: 1, envelope },
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
    let mut machine = joined_machine(&test).await;
    let person = signed_up(&test).await;
    let registered = person.register_device().await;
    let mut phone = PhoneState::registered(&test.url, registered.device.clone(), &registered.token);
    pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    machine.post_board(board(MARKER)).await.unwrap();
    let boards = phone.boards().await.unwrap();
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
    let session = person.cookie.as_deref().unwrap().split_once('=').unwrap().1;
    for leak in [
        MARKER,
        "refunds@greenhouse",
        "Keep the old endpoint",
        "Nightly deps",
        &registered.token,
        session,
    ] {
        assert!(
            !holds(&stored, leak.as_bytes()),
            "the database holds {leak:?}"
        );
    }
}

#[tokio::test]
async fn a_second_commitment_ends_the_pairing_for_everyone() {
    let test = start_hub(|_| {}).await;
    let machine = joined_machine(&test).await;
    let opened: PairingOpened = machine.client().post("/v1/m/pairings", &()).await.unwrap();
    let commit = |seed: u8| CommitRequest {
        code: opened.code.clone(),
        commitment: Bytes32([seed; 32]),
    };
    let first_device = device_client(&test, &signed_up(&test).await.register_device().await);
    let second_device = device_client(&test, &signed_up(&test).await.register_device().await);
    let first: Committed = first_device
        .post("/v1/d/pairings", &commit(1))
        .await
        .unwrap();
    let second = second_device
        .post::<_, Committed>("/v1/d/pairings", &commit(2))
        .await;
    assert_eq!(
        second.unwrap_err(),
        HubError::Status {
            status: 409,
            code: "second-commitment".to_string()
        }
    );
    let seen_by_machine: PairingView = machine
        .client()
        .get(&format!("/v1/m/pairings/{}", opened.pairing), &[])
        .await
        .unwrap();
    assert_eq!(seen_by_machine.state, PairingState::Failed);
    assert_eq!(
        seen_by_machine.reason,
        Some(PairingFailure::SecondCommitment)
    );
    let seen_by_device: PairingView = first_device
        .get(&format!("/v1/d/pairings/{}", first.pairing), &[])
        .await
        .unwrap();
    assert_eq!(seen_by_device.state, PairingState::Failed);
}

#[tokio::test]
async fn a_pairing_nobody_answers_ends_in_a_visible_timeout() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let outcome = machine
        .pair(Duration::from_secs(1), |_| {}, async |_| true)
        .await;
    assert!(matches!(outcome, Err(PairError::Timeout)), "{outcome:?}");
    let events = test.hub.store().events().unwrap();
    assert!(events.contains(&("pairing.refused".to_string(), Some("timeout".to_string()))));
}

#[tokio::test]
async fn unknown_expired_and_finished_codes_all_get_the_same_401() {
    let test = start_hub(|config| config.pairing_ttl = 1).await;
    let machine = joined_machine(&test).await;
    let opened: PairingOpened = machine.client().post("/v1/m/pairings", &()).await.unwrap();
    let device = device_client(&test, &signed_up(&test).await.register_device().await);
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let late = device
        .post::<_, Committed>(
            "/v1/d/pairings",
            &CommitRequest {
                code: opened.code,
                commitment: Bytes32([0; 32]),
            },
        )
        .await;
    assert_eq!(status_of(late), 401);
    let unknown = device
        .post::<_, Committed>(
            "/v1/d/pairings",
            &CommitRequest {
                code: "0000-0000".to_string(),
                commitment: Bytes32([0; 32]),
            },
        )
        .await;
    assert_eq!(
        unknown.unwrap_err(),
        HubError::Status {
            status: 401,
            code: "unauthorized".to_string()
        }
    );

    let mut machine = machine;
    let mut phone = phone(&test).await;
    let (code_out, code_in) = oneshot::channel::<String>();
    let used = std::sync::Mutex::new(None);
    let (admitted, _) = tokio::join!(
        machine.pair(
            PAIRING_TIMEOUT,
            |code| {
                *used.lock().unwrap() = Some(code.to_string());
                let _ = code_out.send(code.to_string());
            },
            async |_| true
        ),
        async {
            let code = code_in.await.unwrap();
            phone.pair(&code, PAIRING_TIMEOUT, async |_| true).await
        }
    );
    admitted.unwrap();
    let used_code = used.lock().unwrap().clone().unwrap();
    let finished = device
        .post::<_, Committed>(
            "/v1/d/pairings",
            &CommitRequest {
                code: used_code,
                commitment: Bytes32([0; 32]),
            },
        )
        .await;
    assert_eq!(
        finished.unwrap_err(),
        HubError::Status {
            status: 401,
            code: "unauthorized".to_string()
        }
    );
}

#[tokio::test]
async fn a_join_code_works_once() {
    let test = start_hub(|_| {}).await;
    let code = test.hub.mint_join_code().unwrap();
    join(&test.url, &code, "mac").await.unwrap();
    match join(&test.url, &code, "mac").await {
        Err(kelpie_hub_fakes::Error::Hub(error)) => assert_eq!(error.status(), Some(401)),
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
        .post(format!("{}/v1/d/pairings", test.url))
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
    let device = Id::new(Prefix::Device).unwrap();
    let device_keys = SecretKeys::generate();
    let (machine_id, machine_keys) = (machine.machine.clone(), machine.public());
    let command = |at| {
        seal(
            Header::new(
                Id::new(Prefix::Command).unwrap(),
                Kind::Routine,
                device.clone(),
                machine_id.clone(),
                at,
            ),
            &Payload::Routine(RunRoutine {
                id: "nightly-deps".into(),
            }),
            &device_keys,
            &[(machine_id.clone(), machine_keys)],
        )
        .unwrap()
    };
    let first = command(kelpie_hub::now());
    test.hub.enqueue(&machine.machine, &first).unwrap();
    let inbox = machine.poll_once(0).await.unwrap();
    assert_eq!(inbox.messages.len(), 1);
    assert_eq!(inbox.messages[0].envelope, first);
    assert_eq!(inbox.seq, 1);

    let hub = Arc::clone(&test.hub);
    let target = machine.machine.clone();
    let second = command(kelpie_hub::now());
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
