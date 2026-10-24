#[allow(dead_code)]
mod common;

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::*;
use kebbie_hub_api::{
    CONNECT_UNKNOWN, ConnectApproval, ConnectApproved, ConnectPoll, ConnectState, Connected,
};
use kebbie_hub_fakes::machine::{ConnectError, ConnectPolled, PendingConnect, connect};
use kebbie_hub_fakes::{HubClient, PairError};
use kebbie_hub_protocol::{SecretKeys, machine_fingerprint};

async fn poll(pending: &mut PendingConnect) -> Result<&'static str, ConnectError> {
    Ok(match pending.poll().await? {
        ConnectPolled::Pending => "pending",
        ConnectPolled::SlowDown => "slow-down",
        ConnectPolled::Connected { .. } => "connected",
    })
}

#[tokio::test]
async fn a_machine_connects_through_the_browser_and_belongs_to_the_approving_account() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut pending = connect(&test.url, "Carl's Mac").await.unwrap();
    let started = pending.started.clone();
    assert_eq!(started.user_code.len(), 9);
    assert_eq!(started.verification_uri, "http://localhost:8470/connect");
    assert_eq!(
        started.verification_uri_complete,
        format!("http://localhost:8470/connect?code={}", started.user_code)
    );
    assert_eq!((started.interval, started.expires_in), (0, 600));
    assert!(
        pending
            .link()
            .contains(&format!("?code={}#k=", started.user_code))
    );
    assert_eq!(poll(&mut pending).await.unwrap(), "pending");

    let mut person = member(&test).await;
    let shown = person
        .connect_request(&started.user_code.to_lowercase())
        .await
        .unwrap();
    assert_eq!(
        (shown.code.as_str(), shown.name.as_str(), shown.state),
        (
            started.user_code.as_str(),
            "Carl's Mac",
            ConnectState::Pending
        )
    );
    let approval = person
        .approve_connect(&pending.link(), false)
        .await
        .unwrap();
    assert_eq!(approval.approved.name, "Carl's Mac");
    assert_eq!(approval.approved.pairing, None);

    let (mut machine, pairing, _) = pending.wait(WAIT).await.unwrap();
    assert_eq!(machine.machine, approval.approved.machine);
    assert_eq!(pairing, None);
    machine.post_board(board(MARKER)).await.unwrap();
    let account = person.account().await.unwrap();
    let owner: Option<String> = rusqlite::Connection::open(test.hub.config().database())
        .unwrap()
        .query_row(
            "SELECT account_id FROM machines WHERE id = ?1",
            [machine.machine.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(owner, Some(account.account));
    assert!(
        test.hub
            .store()
            .events()
            .unwrap()
            .iter()
            .any(|(kind, detail)| kind == "machine.joined"
                && detail.as_deref() == Some("by=connect"))
    );
}

#[tokio::test]
async fn polling_faster_than_the_interval_slows_the_machine_down() {
    let test = start_hub(|config| config.connect_interval = 5).await;
    let mut pending = connect(&test.url, "mac").await.unwrap();
    assert_eq!(poll(&mut pending).await.unwrap(), "pending");
    assert_eq!(poll(&mut pending).await.unwrap(), "slow-down");
    assert_eq!(pending.interval, 10);
    assert_eq!(poll(&mut pending).await.unwrap(), "slow-down");
    let stored: u64 = rusqlite::Connection::open(test.hub.config().database())
        .unwrap()
        .query_row("SELECT poll_interval FROM connects", [], |row| row.get(0))
        .unwrap();
    assert_eq!(stored, 15);
}

#[tokio::test]
async fn cancelling_in_the_browser_denies_the_machine_once() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut pending = connect(&test.url, "mac").await.unwrap();
    let mut person = member(&test).await;
    person
        .deny_connect(&pending.started.user_code)
        .await
        .unwrap();
    assert!(matches!(
        poll(&mut pending).await,
        Err(ConnectError::Denied)
    ));
    match poll(&mut pending).await {
        Err(ConnectError::Hub(error)) => {
            assert_eq!(hub_code(error), (400, CONNECT_UNKNOWN.to_string()))
        }
        other => panic!("a denied code polled again: {:?}", other.map(drop)),
    }
    let again = person.approve_connect(&pending.link(), false).await;
    assert_eq!(
        hub_code(again.err().unwrap()),
        (404, "unknown-code".to_string())
    );
}

#[tokio::test]
async fn an_expired_code_is_neither_approved_nor_redeemed() {
    let test = start_hub(|config| {
        config.connect_ttl = 1;
        config.connect_interval = 0;
    })
    .await;
    let mut pending = connect(&test.url, "mac").await.unwrap();
    let mut person = member(&test).await;
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let refused = person.approve_connect(&pending.link(), false).await;
    assert_eq!(
        hub_code(refused.err().unwrap()),
        (404, "unknown-code".to_string())
    );
    assert!(matches!(
        poll(&mut pending).await,
        Err(ConnectError::Expired)
    ));
    test.hub.sweep().unwrap();
    assert_eq!(rows(&test, "connects"), 0);
}

#[tokio::test]
async fn an_approval_left_unredeemed_leaves_no_machine_behind() {
    let test = start_hub(|config| config.connect_ttl = 2).await;
    let mut person = member(&test).await;
    let pending = connect(&test.url, "mac").await.unwrap();
    person
        .approve_connect(&pending.link(), false)
        .await
        .unwrap();
    assert_eq!(rows(&test, "machines"), 1);
    assert!(test.hub.store().machines().unwrap().is_empty());
    tokio::time::sleep(Duration::from_millis(2200)).await;
    test.hub.sweep().unwrap();
    assert_eq!((rows(&test, "machines"), rows(&test, "connects")), (0, 0));
}

#[tokio::test]
async fn a_code_works_once() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let pending = connect(&test.url, "mac").await.unwrap();
    let link = pending.link();
    let device_code = pending.started.device_code.clone();
    let mut person = member(&test).await;
    person.approve_connect(&link, false).await.unwrap();
    let twice = person.approve_connect(&link, false).await;
    assert_eq!(
        hub_code(twice.err().unwrap()),
        (404, "unknown-code".to_string())
    );
    pending.wait(WAIT).await.unwrap();
    let redeemed_again = HubClient::new(&test.url, None)
        .post::<_, Connected>(
            "/v1/m/connect/token",
            &ConnectPoll {
                device_code: device_code.clone(),
            },
        )
        .await;
    assert_eq!(
        hub_code(redeemed_again.unwrap_err()),
        (400, CONNECT_UNKNOWN.to_string())
    );
    let stranger = HubClient::new(&test.url, None)
        .post::<_, Connected>(
            "/v1/m/connect/token",
            &ConnectPoll {
                device_code: "khc_made-up".to_string(),
            },
        )
        .await;
    assert_eq!(
        hub_code(stranger.unwrap_err()),
        (400, CONNECT_UNKNOWN.to_string())
    );
}

#[tokio::test]
async fn approving_needs_a_fresh_passkey_session_from_the_hub_origin() {
    let test = start_hub(|config| {
        config.fresh_session_ttl = 1;
        config.connect_interval = 0;
    })
    .await;
    let pending = connect(&test.url, "mac").await.unwrap();
    let code = pending.started.user_code.clone();
    let mut person = member(&test).await;
    let approval = ConnectApproval {
        code: code.clone(),
        device: None,
        commitment: None,
    };
    let mut anonymous = browser(&test);
    let refused = anonymous
        .post::<_, ConnectApproved>("/v1/a/connect/approve", &approval)
        .await;
    assert_eq!(
        hub_code(refused.unwrap_err()),
        (401, "unauthorized".to_string())
    );
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let stale = person
        .post::<_, ConnectApproved>("/v1/a/connect/approve", &approval)
        .await;
    assert_eq!(
        hub_code(stale.unwrap_err()),
        (401, "reauth-required".to_string())
    );
    person.log_in().await.unwrap();
    let origin = person.origin.clone();
    for forged in ["https://evil.example", "null"] {
        person.origin = forged.to_string();
        let cross = person
            .post::<_, ConnectApproved>("/v1/a/connect/approve", &approval)
            .await;
        assert_eq!(
            hub_code(cross.unwrap_err()),
            (403, "cross-origin".to_string())
        );
        let denied = person.deny_connect(&code).await;
        assert_eq!(
            hub_code(denied.unwrap_err()),
            (403, "cross-origin".to_string())
        );
    }
    person.origin = origin;
    person
        .approve_connect(&pending.link(), false)
        .await
        .unwrap();
    pending.wait(WAIT).await.unwrap();
}

#[tokio::test]
async fn the_browser_that_connects_also_reads_the_board_without_a_digit_compare() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let pending = connect(&test.url, "mac").await.unwrap();
    let link = pending.link();
    let mut person = member(&test).await;
    let approval = person.approve_connect(&link, true).await.unwrap();
    assert!(approval.approved.pairing.is_some());
    let (mut machine, pairing, secret) = pending.wait(WAIT).await.unwrap();
    let pairing = pairing.expect("the machine learns the browser's pairing");
    let (admitted, paired) = tokio::join!(
        machine.finish_connect(&pairing, &secret, WAIT),
        person.show_board(approval, WAIT)
    );
    let device = admitted.unwrap();
    assert_eq!(paired.unwrap(), machine.machine);
    assert_eq!(
        Some(&device),
        person.device.as_ref().unwrap().device.as_ref()
    );
    machine.post_board(board(MARKER)).await.unwrap();
    let boards = person.device.as_ref().unwrap().boards().await.unwrap();
    let opened = boards[0].board.as_ref().unwrap();
    assert_eq!(opened.items[0].summary, format!("{MARKER} summary"));
}

#[tokio::test]
async fn a_link_that_names_another_machine_is_refused_before_the_reveal() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let pending = connect(&test.url, "mac").await.unwrap();
    let other = URL_SAFE_NO_PAD.encode(machine_fingerprint(&SecretKeys::generate().public()));
    let (head, fragment) = pending
        .link()
        .split_once("&m=")
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .unwrap();
    assert!(!fragment.is_empty());
    let forged = format!("{head}&m={other}");
    let mut person = member(&test).await;
    let approval = person.approve_connect(&forged, true).await.unwrap();
    let (mut machine, pairing, secret) = pending.wait(WAIT).await.unwrap();
    let pairing = pairing.unwrap();
    let short = Duration::from_secs(2);
    let (admitted, paired) = tokio::join!(
        machine.finish_connect(&pairing, &secret, short),
        person.show_board(approval, short)
    );
    assert!(matches!(paired, Err(PairError::WrongMachine)), "{paired:?}");
    assert!(admitted.is_err());
    assert!(machine.devices.is_empty());
}

#[tokio::test]
async fn a_browser_without_the_machines_secret_is_refused_by_the_machine() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let pending = connect(&test.url, "mac").await.unwrap();
    let link = pending.link();
    let (head, tail) = link.split_once("#k=").unwrap();
    let (_, fingerprint) = tail.split_once('&').unwrap();
    let guessed = URL_SAFE_NO_PAD.encode([7u8; 32]);
    let mut person = member(&test).await;
    let approval = person
        .approve_connect(&format!("{head}#k={guessed}&{fingerprint}"), true)
        .await
        .unwrap();
    let (mut machine, pairing, secret) = pending.wait(WAIT).await.unwrap();
    let pairing = pairing.unwrap();
    let (admitted, paired) = tokio::join!(
        machine.finish_connect(&pairing, &secret, WAIT),
        person.show_board(approval, WAIT)
    );
    assert!(matches!(admitted, Err(PairError::Mismatch)), "{admitted:?}");
    assert!(paired.is_err());
}

#[tokio::test]
async fn starting_a_connect_is_rate_limited_per_address() {
    let test = start_hub(|config| {
        config.connect_rate = kebbie_hub::RateLimit {
            burst: 2,
            per_minute: 1,
        }
    })
    .await;
    connect(&test.url, "one").await.unwrap();
    connect(&test.url, "two").await.unwrap();
    match connect(&test.url, "three").await {
        Err(kebbie_hub_fakes::Error::Hub(error)) => assert_eq!(error.status(), Some(429)),
        other => panic!("a third connect went through: {:?}", other.map(|_| ())),
    }
}

#[tokio::test]
async fn codes_and_tokens_are_kept_as_hashes() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let pending = connect(&test.url, "mac").await.unwrap();
    let user_code = pending.started.user_code.clone();
    let device_code = pending.started.device_code.clone();
    let mut person = member(&test).await;
    person
        .approve_connect(&pending.link(), false)
        .await
        .unwrap();
    let held = database_bytes(&test);
    assert!(!holds(&held, user_code.as_bytes()));
    assert!(!holds(&held, device_code.as_bytes()));
    let mut machine = pending.wait(WAIT).await.unwrap().0;
    machine.post_board(board(MARKER)).await.unwrap();
    let held = database_bytes(&test);
    let token = serde_json::to_value(&machine).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!holds(&held, token.as_bytes()));
}

#[tokio::test]
async fn a_join_code_still_joins_a_machine_without_an_account() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    machine.post_board(board(MARKER)).await.unwrap();
    assert_eq!(test.hub.store().machines().unwrap()[0].owner, None);
}
