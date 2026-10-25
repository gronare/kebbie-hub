#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub::{DEV_SESSION_COOKIE, SESSION_COOKIE};
use kebbie_hub_api::{
    AccountView, BoardChanged, Bytes32, CommitRequest, Committed, CreationCeremony, DeviceRequest,
    Devices, EVENT_BOARD, EVENT_MACHINES, EVENT_MACHINE_REMOVED, EVENT_PAIRING, MachineRemoved, PairingChanged,
    PairingOpened, PairingState, Passkeys,
};
use kebbie_hub_api::{LoginFinish, RegisterFinish};
use kebbie_hub_fakes::phone::PhoneState;
use kebbie_hub_fakes::{HubClient, PairError};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use webauthn_authenticator_rs::WebauthnAuthenticator;
use kebbie_hub_fakes::passkey::DiscoverablePasskey;
use webauthn_rs::prelude::Url;
use webauthn_rs_proto::UserVerificationPolicy;

const SOON: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(600);

const HTTPS_ORIGIN: &str = "https://localhost:8470";

fn attributes(set_cookie: &str) -> Vec<&str> {
    set_cookie.split(';').map(str::trim).collect()
}

#[tokio::test]
async fn a_passkey_signs_up_and_logs_in_with_a_strict_http_only_cookie() {
    let test = start_hub(|_| {}).await;
    let mut person = Person::new(&test.url);
    let invite = test.hub.mint_invite().unwrap();
    let ceremony: CreationCeremony = body(person.begin_sign_up(&invite).await).await;
    let credential = person
        .passkey
        .do_registration(origin(), ceremony.options)
        .unwrap();
    let response = person
        .post(
            "/v1/a/register/finish",
            &kebbie_hub_api::RegisterFinish {
                ceremony: ceremony.ceremony,
                credential,
                name: None,
            },
        )
        .await;
    let set_cookie = person.keep_cookie(&response);
    let created: AccountView = body(response).await;
    assert!(created.handle.starts_with('k'), "{}", created.handle);
    let parts = attributes(&set_cookie);
    assert!(parts[0].starts_with(&format!("{DEV_SESSION_COOKIE}=khs_")), "{set_cookie}");
    for flag in ["HttpOnly", "SameSite=Strict", "Path=/"] {
        assert!(parts.contains(&flag), "{set_cookie}");
    }
    assert!(!parts.contains(&"Secure"), "{set_cookie}");
    assert!(!parts.iter().any(|part| part.starts_with("Domain")));

    let me: AccountView = body(person.get("/v1/a/account").await).await;
    assert_eq!(me, created);
    let token = person.cookie.clone().unwrap();
    let token = token.split_once('=').unwrap().1.to_string();
    let mut wrong_name = Person::new(&test.url);
    wrong_name.cookie = Some(format!("{SESSION_COOKIE}={token}"));
    assert_eq!(wrong_name.get("/v1/a/account").await.status(), 401);

    let logout = person.post("/v1/a/logout", &()).await;
    assert_eq!(logout.status(), 204);
    let cleared = logout.headers()["set-cookie"].to_str().unwrap().to_string();
    assert!(cleared.starts_with(&format!("{DEV_SESSION_COOKIE}=;")), "{cleared}");
    assert!(cleared.contains("Max-Age=0"), "{cleared}");
    assert!(!attributes(&cleared).contains(&"Secure"), "{cleared}");
    assert_eq!(person.get("/v1/a/account").await.status(), 401);

    person.cookie = None;
    let logged_in: AccountView = body(person.log_in().await).await;
    assert_eq!(logged_in, created);
    assert_eq!(person.get("/v1/a/account").await.status(), 200);
    let passkeys: Passkeys = body(person.get("/v1/a/passkeys").await).await;
    assert!(passkeys.passkeys[0].last_used_at.is_some());
}

#[tokio::test]
async fn an_https_hub_sets_and_reads_only_the_secure_host_cookie() {
    let test = start_hub(|config| config.origin = HTTPS_ORIGIN.to_string()).await;
    let mut person = Person::new(&test.url);
    person.origin = Some(HTTPS_ORIGIN.to_string());
    let invite = test.hub.mint_invite().unwrap();
    let ceremony: CreationCeremony = body(person.begin_sign_up(&invite).await).await;
    let credential = person
        .passkey
        .do_registration(Url::parse(HTTPS_ORIGIN).unwrap(), ceremony.options)
        .unwrap();
    let response = person
        .post(
            "/v1/a/register/finish",
            &kebbie_hub_api::RegisterFinish {
                ceremony: ceremony.ceremony,
                credential,
                name: None,
            },
        )
        .await;
    let set_cookie = person.keep_cookie(&response);
    assert_eq!(response.status(), 200);
    let parts = attributes(&set_cookie);
    assert!(parts[0].starts_with(&format!("{SESSION_COOKIE}=khs_")), "{set_cookie}");
    for flag in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"] {
        assert!(parts.contains(&flag), "{set_cookie}");
    }
    assert!(SESSION_COOKIE.starts_with("__Host-"));
    assert!(!parts.iter().any(|part| part.starts_with("Domain")));
    assert_eq!(person.get("/v1/a/account").await.status(), 200);

    let token = person.cookie.clone().unwrap();
    let token = token.split_once('=').unwrap().1.to_string();
    let mut plain = Person::new(&test.url);
    plain.origin = Some(HTTPS_ORIGIN.to_string());
    plain.cookie = Some(format!("{DEV_SESSION_COOKIE}={token}"));
    assert_eq!(plain.get("/v1/a/account").await.status(), 401);
    assert_eq!(plain.post("/v1/a/logout", &()).await.status(), 204);
    assert_eq!(person.get("/v1/a/account").await.status(), 200);

    let logout = person.post("/v1/a/logout", &()).await;
    assert_eq!(logout.status(), 204);
    let cleared = logout.headers()["set-cookie"].to_str().unwrap().to_string();
    assert!(cleared.starts_with(&format!("{SESSION_COOKIE}=;")), "{cleared}");
    assert!(attributes(&cleared).contains(&"Secure"), "{cleared}");
    assert_eq!(person.get("/v1/a/account").await.status(), 401);
}

#[test]
fn a_plain_http_origin_is_refused_off_loopback() {
    for (rp_id, origin) in [("hub.example.com", "http://hub.example.com"), ("10.0.0.5", "http://10.0.0.5:8470")] {
        let refused = kebbie_hub::relying_party(rp_id, origin).err().map(|error| error.to_string());
        assert!(refused.as_deref().is_some_and(|text| text.contains("must be https")), "{origin}: {refused:?}");
    }
    for (rp_id, origin) in [("localhost", "http://localhost:8470"), ("127.0.0.1", "http://127.0.0.1:8470"), ("::1", "http://[::1]:8470")] {
        let accepted = kebbie_hub::relying_party(rp_id, origin).err().map(|error| error.to_string());
        assert!(!accepted.as_deref().is_some_and(|text| text.contains("must be https")), "{origin}: {accepted:?}");
    }
}

#[tokio::test]
async fn an_invite_signs_up_one_account_even_when_two_ceremonies_race() {
    let test = start_hub(|_| {}).await;
    let invite = test.hub.mint_invite().unwrap();
    let mut first = Person::new(&test.url);
    let mut second = Person::new(&test.url);
    let first_ceremony: CreationCeremony = body(first.begin_sign_up(&invite).await).await;
    let second_ceremony: CreationCeremony = body(second.begin_sign_up(&invite).await).await;
    assert!(
        first
            .finish_sign_up(first_ceremony)
            .await
            .status()
            .is_success()
    );
    assert_eq!(
        refusal(second.finish_sign_up(second_ceremony).await).await,
        (401, "unauthorized".to_string())
    );
    assert_eq!(second.cookie, None);
    let third = Person::new(&test.url);
    assert_eq!(refusal(third.begin_sign_up(&invite).await).await.0, 401);
    assert_eq!(rows(&test, "accounts"), 1);
}

#[tokio::test]
async fn a_ceremony_works_once() {
    let test = start_hub(|_| {}).await;
    let mut person = signed_up(&test).await;
    person.cookie = None;
    let ceremony = person.begin_log_in().await;
    let credential = person
        .passkey
        .do_authentication(origin(), ceremony.options)
        .unwrap();
    let finish = kebbie_hub_api::LoginFinish {
        ceremony: ceremony.ceremony,
        credential,
    };
    assert!(
        person
            .post("/v1/a/login/finish", &finish)
            .await
            .status()
            .is_success()
    );
    assert_eq!(
        refusal(person.post("/v1/a/login/finish", &finish).await).await,
        (401, "unauthorized".to_string())
    );
}

#[tokio::test]
async fn a_session_expires_and_the_sweep_deletes_it() {
    let test = start_hub(|config| config.session_ttl = 1).await;
    let person = signed_up(&test).await;
    assert_eq!(person.get("/v1/a/account").await.status(), 200);
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert_eq!(person.get("/v1/a/account").await.status(), 401);
    assert_eq!(register_device_status(&person).await, 401);
    assert_eq!(rows(&test, "sessions"), 1);
    assert_eq!(test.hub.sweep().unwrap().sessions, 1);
    assert_eq!(rows(&test, "sessions"), 0);
}

async fn register_device_status(person: &Person) -> u16 {
    person
        .post("/v1/a/devices", &DeviceRequest::default())
        .await
        .status()
        .as_u16()
}

#[tokio::test]
async fn state_changing_account_calls_need_the_hub_origin() {
    let test = start_hub(|_| {}).await;
    let mut person = signed_up(&test).await;
    for origin in [Some("https://evil.example"), Some("null"), None] {
        person.origin = origin.map(str::to_string);
        for path in [
            "/v1/a/devices",
            "/v1/a/logout",
            "/v1/a/passkeys/begin",
            "/v1/a/login/begin",
            "/v1/a/login/finish",
            "/v1/a/register/begin",
            "/v1/a/register/finish",
            "/v1/a/passkeys/finish",
        ] {
            let refused = refusal(person.post(path, &serde_json::json!({})).await).await;
            assert_eq!(
                refused,
                (403, "cross-origin".to_string()),
                "{path} {origin:?}"
            );
        }
        for path in ["/v1/a/devices/dev_x", "/v1/a/passkeys/AAAA"] {
            assert_eq!(person.delete(path).await.status(), 403, "{path} {origin:?}");
        }
    }
    person.origin = None;
    assert_eq!(person.get("/v1/a/devices").await.status(), 200);
    person.origin = Some(kebbie_hub::DEV_ORIGIN.to_string());
    assert_eq!(person.get("/v1/a/account").await.status(), 200);
}

#[tokio::test]
async fn a_device_token_comes_only_from_a_logged_in_account() {
    let test = start_hub(|_| {}).await;
    let anonymous = Person::new(&test.url);
    assert_eq!(register_device_status(&anonymous).await, 401);

    let machine = joined_machine(&test).await;
    let opened: PairingOpened = machine.client().post("/v1/m/pairings", &()).await.unwrap();
    let commit = CommitRequest {
        code: opened.code.clone(),
        commitment: Bytes32([1; 32]),
    };
    let without_token = HubClient::new(&test.url, None)
        .post::<_, Committed>("/v1/d/pairings", &commit)
        .await;
    assert_eq!(status_of(without_token), 401);

    let person = signed_up(&test).await;
    let registered = person.register_device().await;
    let committed: Committed = device_client(&test, &registered)
        .post("/v1/d/pairings", &commit)
        .await
        .unwrap();
    assert_eq!(committed.device, registered.device);
    let devices: Devices = body(person.get("/v1/a/devices").await).await;
    assert_eq!(devices.devices.len(), 1);
    assert_eq!(devices.devices[0].name.as_deref(), Some("phone"));
}

#[tokio::test]
async fn a_dev_hub_still_lets_a_pairing_code_create_a_device() {
    let test = start_hub(|config| config.anonymous_devices = true).await;
    let mut machine = joined_machine(&test).await;
    let mut phone = PhoneState::new(&test.url);
    let (admitted, paired) = pair_both(&mut machine, &mut phone, true).await;
    assert_eq!(phone.device, Some(admitted.unwrap()));
    paired.unwrap();
}

#[tokio::test]
async fn an_account_adds_and_removes_passkeys_but_never_its_last() {
    let test = start_hub(|_| {}).await;
    let mut person = signed_up(&test).await;
    let mut second = soft_passkey();
    let added = person.add_passkey(&mut second).await;
    let listed: Passkeys = body(person.get("/v1/a/passkeys").await).await;
    assert_eq!(listed.passkeys.len(), 2);
    let first = listed
        .passkeys
        .iter()
        .find(|passkey| passkey.passkey != added.passkey)
        .unwrap()
        .passkey
        .clone();

    let removed = person.delete(&format!("/v1/a/passkeys/{first}")).await;
    assert_eq!(removed.status(), 204);
    assert_eq!(
        refusal(
            person
                .delete(&format!("/v1/a/passkeys/{}", added.passkey))
                .await
        )
        .await,
        (409, "last-passkey".to_string())
    );

    std::mem::swap(&mut person.passkey, &mut second);
    person.cookie = None;
    assert!(person.log_in().await.status().is_success());
    std::mem::swap(&mut person.passkey, &mut second);
    person.cookie = None;
    assert_eq!(
        person.log_in().await.status(),
        401,
        "the removed passkey no longer signs in"
    );
}

#[tokio::test]
async fn the_device_feed_announces_pairings_without_content_and_no_boards() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let registered = signed_up(&test).await.register_device().await;
    let mut events = EventStream::open(&test.url, &registered.token).await;
    assert_eq!(events.next(QUIET).await, Next::Quiet);

    let mut phone = PhoneState::registered(&test.url, registered.device.clone(), &registered.token);
    pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    let mut states = Vec::new();
    while states.last() != Some(&PairingState::Admitted) {
        let data = events.until(EVENT_PAIRING, SOON).await;
        let changed: PairingChanged = serde_json::from_value(data).unwrap();
        states.push(changed.state);
    }
    assert!(states.iter().all(|state| *state != PairingState::Open));

    machine.post_board(board(MARKER)).await.unwrap();
    assert_eq!(events.next(QUIET).await, Next::Quiet);
}

#[tokio::test]
async fn the_account_feed_announces_its_boards_without_content() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let mut machine = connected_with_board(&test, &mut person).await;
    let mut events = EventStream::of_account(&person).await;
    events.until(EVENT_MACHINES, SOON).await;
    assert_eq!(events.next(QUIET).await, Next::Quiet);
    let rev = machine.post_board(board(MARKER)).await.unwrap();
    let data = events.until(EVENT_BOARD, SOON).await;
    assert_eq!(data.as_object().unwrap().len(), 2, "{data}");
    let changed: BoardChanged = serde_json::from_value(data.clone()).unwrap();
    assert_eq!(
        changed,
        BoardChanged {
            machine: machine.machine.clone(),
            rev
        }
    );
    assert!(!data.to_string().contains(MARKER));
    assert_eq!(events.next(QUIET).await, Next::Quiet);
}

#[tokio::test]
async fn a_board_wakes_only_the_account_of_its_machine() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut first = member(&test).await;
    let mut second = member(&test).await;
    let mut first_machine = connected_with_board(&test, &mut first).await;
    let mut second_machine = connected_with_board(&test, &mut second).await;
    second_machine.post_board(board("second")).await.unwrap();
    let mut first_events = EventStream::of_account(&first).await;
    let mut second_events = EventStream::of_account(&second).await;
    second_events.until(EVENT_BOARD, SOON).await;
    second_events.until(EVENT_MACHINES, SOON).await;
    first_events.until(EVENT_MACHINES, SOON).await;
    assert_eq!(first_events.next(QUIET).await, Next::Quiet);

    first_machine.post_board(board(MARKER)).await.unwrap();
    first_events.until(EVENT_BOARD, SOON).await;
    assert_eq!(second_events.next(QUIET).await, Next::Quiet);
    let boards = second.boards().await.unwrap();
    assert_eq!(boards.len(), 1);
    assert_eq!(boards[0].machine, second_machine.machine);
}

#[tokio::test]
async fn a_machine_removing_a_device_announces_it_on_the_device_feed() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let registered = signed_up(&test).await.register_device().await;
    let mut phone = PhoneState::registered(&test.url, registered.device.clone(), &registered.token);
    let device = pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    let mut events = EventStream::open(&test.url, &registered.token).await;
    events.until(EVENT_PAIRING, SOON).await;

    machine.remove_device(&device).await.unwrap();
    let removed: MachineRemoved =
        serde_json::from_value(events.until(EVENT_MACHINE_REMOVED, SOON).await).unwrap();
    assert_eq!(removed.machine, machine.machine);
    assert!(machine.listed_devices().await.unwrap().is_empty());

    machine.post_board(board("after")).await.unwrap();
    assert_eq!(events.next(QUIET).await, Next::Quiet);
    let again = machine
        .client()
        .delete(&format!("/v1/m/devices/{device}"))
        .await;
    assert_eq!(status_of(again), 404);
}

#[tokio::test]
async fn a_machine_cannot_remove_a_device_another_machine_admitted() {
    let test = start_hub(|_| {}).await;
    let mut owner = joined_machine(&test).await;
    let other = joined_machine(&test).await;
    let mut phone = phone(&test).await;
    let device = pair_both(&mut owner, &mut phone, true).await.0.unwrap();
    let refused = other
        .client()
        .delete(&format!("/v1/m/devices/{device}"))
        .await;
    assert_eq!(status_of(refused), 404);
    assert_eq!(owner.listed_devices().await.unwrap()[0].device, device);
}

#[tokio::test]
async fn an_account_removing_its_device_ends_the_token_and_the_feed() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let person = signed_up(&test).await;
    let registered = person.register_device().await;
    let mut phone = PhoneState::registered(&test.url, registered.device.clone(), &registered.token);
    pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    let mut events = EventStream::open(&test.url, &registered.token).await;
    events.until(EVENT_PAIRING, SOON).await;

    let stranger = signed_up(&test).await;
    let path = format!("/v1/a/devices/{}", registered.device);
    assert_eq!(stranger.delete(&path).await.status(), 404);
    assert_eq!(person.delete(&path).await.status(), 204);
    assert_eq!(events.next(SOON).await, Next::Ended);
    assert_eq!(
        status_of(
            device_client(&test, &registered)
                .get::<Value>("/v1/d/results", &[])
                .await
        ),
        401
    );
    let devices: Devices = body(person.get("/v1/a/devices").await).await;
    assert!(devices.devices.is_empty());
    machine.post_board(board(MARKER)).await.unwrap();
}

#[tokio::test]
async fn the_admin_cli_mints_invites_and_removes_machines() {
    let test = start_hub(|_| {}).await;
    let admin = |args: &[&str]| {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_kebbie-hub"))
            .arg("--data")
            .arg(test.data.path())
            .arg("admin")
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    let invite = admin(&["invite"]);
    let mut person = Person::new(&test.url);
    assert!(person.sign_up(invite.trim()).await.status().is_success());

    let mut machine = joined_machine(&test).await;
    let registered = person.register_device().await;
    let mut phone = PhoneState::registered(&test.url, registered.device.clone(), &registered.token);
    pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    machine.post_board(board(MARKER)).await.unwrap();
    assert!(admin(&["machines"]).contains(machine.machine.as_str()));

    admin(&["remove-machine", machine.machine.as_str()]);
    match machine.poll_once(0).await {
        Err(kebbie_hub_fakes::Error::Hub(error)) => assert_eq!(error.status(), Some(401)),
        other => panic!("the removed machine still polls: {other:?}"),
    }
    let devices: Devices = body(person.get("/v1/a/devices").await).await;
    assert!(devices.devices[0].machines.is_empty());
}

#[tokio::test]
async fn the_sweep_drops_devices_no_account_holds_and_no_machine_admits() {
    let test = start_hub(|config| config.anonymous_devices = true).await;
    let mut machine = joined_machine(&test).await;
    let mut refused = PhoneState::new(&test.url);
    let outcome = pair_both(&mut machine, &mut refused, false).await.0;
    assert!(matches!(outcome, Err(PairError::Declined)), "{outcome:?}");
    let mut admitted = PhoneState::new(&test.url);
    let device = pair_both(&mut machine, &mut admitted, true)
        .await
        .0
        .unwrap();
    let held = signed_up(&test).await.register_device().await;

    assert_eq!(test.hub.sweep().unwrap().devices, 1);
    assert_eq!(
        status_of(refused.client().get::<Value>("/v1/d/results", &[]).await),
        401
    );
    admitted
        .client()
        .get::<Value>("/v1/d/results", &[])
        .await
        .unwrap();
    device_client(&test, &held)
        .get::<Value>("/v1/d/results", &[])
        .await
        .unwrap();

    machine.remove_device(&device).await.unwrap();
    assert_eq!(test.hub.sweep().unwrap().devices, 1);
    assert_eq!(
        status_of(admitted.client().get::<Value>("/v1/d/results", &[]).await),
        401
    );
    device_client(&test, &held)
        .get::<Value>("/v1/d/results", &[])
        .await
        .unwrap();
}

#[tokio::test]
async fn a_device_mid_pairing_is_not_swept() {
    let test = start_hub(|config| config.anonymous_devices = true).await;
    let machine = joined_machine(&test).await;
    let opened: PairingOpened = machine.client().post("/v1/m/pairings", &()).await.unwrap();
    let committed: Committed = HubClient::new(&test.url, None)
        .post(
            "/v1/d/pairings",
            &CommitRequest {
                code: opened.code,
                commitment: Bytes32([7; 32]),
            },
        )
        .await
        .unwrap();
    assert_eq!(test.hub.sweep().unwrap().devices, 0);
    let device = HubClient::new(&test.url, committed.token.as_deref());
    device.get::<Value>("/v1/d/results", &[]).await.unwrap();
}

#[tokio::test]
async fn a_half_sent_request_is_dropped_after_the_header_timeout() {
    let test = start_hub(|config| config.header_read_timeout = Duration::from_secs(1)).await;
    let address = test.url.trim_start_matches("http://");
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(b"GET /v1/d/results HTTP/1.1\r\nHost: hub\r\n")
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let mut answer = Vec::new();
    let closed = tokio::time::timeout(SOON, stream.read_to_end(&mut answer)).await;
    assert!(closed.is_ok(), "the hub kept a half-sent request open");
    assert!(started.elapsed() >= Duration::from_millis(900));
}

#[tokio::test]
async fn the_session_cookie_opens_no_device_or_machine_call() {
    let test = start_hub(|_| {}).await;
    let person = signed_up(&test).await;
    for path in ["/v1/d/results", "/v1/d/events", "/v1/d/pairings/any", "/v1/m/inbox"] {
        assert_eq!(person.get(path).await.status(), 401, "{path}");
    }
}

#[tokio::test]
async fn a_ceremony_only_finishes_what_it_began_for_whom_it_began() {
    let test = start_hub(|_| {}).await;
    let mut person = signed_up(&test).await;
    let other = signed_up(&test).await;

    let sign_up: CreationCeremony = body(
        Person::new(&test.url)
            .begin_sign_up(&test.hub.mint_invite().unwrap())
            .await,
    )
    .await;
    let login = person.begin_log_in().await;
    let assertion = person
        .passkey
        .do_authentication(origin(), login.options)
        .unwrap();
    let crossed = LoginFinish {
        ceremony: sign_up.ceremony,
        credential: assertion,
    };
    assert_eq!(
        refusal(person.post("/v1/a/login/finish", &crossed).await)
            .await
            .0,
        401
    );

    let login = person.begin_log_in().await;
    let adding: CreationCeremony = body(person.post("/v1/a/passkeys/begin", &()).await).await;
    let mut extra = soft_passkey();
    let credential = extra.do_registration(origin(), adding.options).unwrap();
    let crossed = RegisterFinish {
        ceremony: login.ceremony,
        credential: credential.clone(),
        name: None,
    };
    assert_eq!(
        refusal(person.post("/v1/a/passkeys/finish", &crossed).await)
            .await
            .0,
        401
    );

    let adding: CreationCeremony = body(person.post("/v1/a/passkeys/begin", &()).await).await;
    let credential = extra.do_registration(origin(), adding.options).unwrap();
    let foreign = RegisterFinish {
        ceremony: adding.ceremony,
        credential,
        name: None,
    };
    assert_eq!(
        refusal(other.post("/v1/a/passkeys/finish", &foreign).await)
            .await
            .0,
        401
    );
    let passkeys: Passkeys = body(other.get("/v1/a/passkeys").await).await;
    assert_eq!(passkeys.passkeys.len(), 1);
    person.cookie = None;
    assert!(person.log_in().await.status().is_success());
}

#[tokio::test]
async fn ceremonies_and_invites_expire() {
    let short_ceremonies = start_hub(|config| config.ceremony_ttl = 1).await;
    let short_invites = start_hub(|config| config.invite_ttl = 1).await;
    let mut early = Person::new(&short_ceremonies.url);
    let invite = short_ceremonies.hub.mint_invite().unwrap();
    let ceremony: CreationCeremony = body(early.begin_sign_up(&invite).await).await;
    let late_invite = short_invites.hub.mint_invite().unwrap();
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert_eq!(refusal(early.finish_sign_up(ceremony).await).await.0, 401);
    assert_eq!(rows(&short_ceremonies, "accounts"), 0);
    assert!(early.sign_up(&invite).await.status().is_success());
    let late = Person::new(&short_invites.url);
    assert_eq!(refusal(late.begin_sign_up(&late_invite).await).await.0, 401);
    assert_eq!(rows(&short_invites, "accounts"), 0);
}

#[tokio::test]
async fn a_stranger_cannot_remove_another_accounts_passkey() {
    let test = start_hub(|_| {}).await;
    let mut owner = signed_up(&test).await;
    let mut extra = soft_passkey();
    let added = owner.add_passkey(&mut extra).await;
    let stranger = signed_up(&test).await;
    let path = format!("/v1/a/passkeys/{}", added.passkey);
    assert_eq!(refusal(stranger.delete(&path).await).await.0, 404);
    let passkeys: Passkeys = body(owner.get("/v1/a/passkeys").await).await;
    assert_eq!(passkeys.passkeys.len(), 2);
    owner.cookie = None;
    assert!(owner.log_in().await.status().is_success());
}

#[tokio::test]
async fn a_passkey_without_user_verification_is_refused_at_sign_up() {
    let test = start_hub(|_| {}).await;
    let mut person = Person::new(&test.url);
    person.passkey = WebauthnAuthenticator::new(DiscoverablePasskey::new(false));
    let mut ceremony: CreationCeremony =
        body(person.begin_sign_up(&test.hub.mint_invite().unwrap()).await).await;
    if let Some(selection) = &mut ceremony.options.public_key.authenticator_selection {
        selection.user_verification = UserVerificationPolicy::Discouraged_DO_NOT_USE;
    }
    let refused = person.finish_sign_up(ceremony).await;
    assert_eq!(refusal(refused).await, (400, "passkey-refused".to_string()));
    assert_eq!(person.cookie, None);
    assert_eq!(rows(&test, "accounts"), 0);
}

#[tokio::test]
async fn a_counter_that_goes_backwards_is_refused_as_a_clone() {
    let test = start_hub(|_| {}).await;
    let mut person = signed_up(&test).await;
    person.cookie = None;
    assert!(person.log_in().await.status().is_success());
    let connection = rusqlite::Connection::open(test.hub.config().database()).unwrap();
    connection
        .execute("UPDATE passkeys SET counter = 1000000", [])
        .unwrap();
    person.cookie = None;
    assert_eq!(refusal(person.log_in().await).await.0, 401);
    assert_eq!(person.cookie, None);
    let events = test.hub.store().events().unwrap();
    assert!(
        events
            .iter()
            .any(|(kind, _)| kind == "passkey.clone-suspected")
    );
}

#[tokio::test]
async fn a_device_holds_at_most_eight_feeds_and_a_closed_one_frees_its_slot() {
    let test = start_hub(|config| config.feed_keep_alive = Duration::from_millis(100)).await;
    let registered = signed_up(&test).await.register_device().await;
    let mut feeds = Vec::new();
    for _ in 0..8 {
        feeds.push(EventStream::open(&test.url, &registered.token).await);
    }
    let ninth = device_client(&test, &registered)
        .get::<Value>("/v1/d/events", &[])
        .await;
    assert_eq!(status_of(ninth), 429);
    drop(feeds.pop());
    let deadline = std::time::Instant::now() + SOON;
    while test.hub.open_feeds(&registered.device) == 8 {
        assert!(
            std::time::Instant::now() < deadline,
            "the closed feed kept its slot"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    feeds.push(EventStream::open(&test.url, &registered.token).await);
}

#[tokio::test]
async fn a_stalled_request_body_is_cut() {
    let test = start_hub(|config| config.body_frame_timeout = Duration::from_secs(1)).await;
    let address = test.url.trim_start_matches("http://");
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            b"POST /v1/m/join HTTP/1.1\r\nHost: hub\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{\"code\"",
        )
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let mut answer = Vec::new();
    let closed = tokio::time::timeout(SOON, stream.read_to_end(&mut answer)).await;
    assert!(closed.is_ok(), "the hub waited on a stalled body");
    assert!(started.elapsed() >= Duration::from_millis(900));
    assert!(String::from_utf8_lossy(&answer).starts_with("HTTP/1.1 4"));
}

#[tokio::test]
async fn the_hub_serves_no_more_connections_than_its_cap() {
    let test = start_hub(|config| config.max_connections = 1).await;
    let address = test.url.trim_start_matches("http://").to_string();
    let held = tokio::net::TcpStream::connect(&address).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let waiting = tokio::spawn({
        let url = test.url.clone();
        async move {
            reqwest::get(format!("{url}/v1/d/results"))
                .await
                .unwrap()
                .status()
        }
    });
    tokio::time::sleep(QUIET).await;
    assert!(!waiting.is_finished(), "a second connection was served");
    drop(held);
    let status = tokio::time::timeout(SOON, waiting).await.unwrap().unwrap();
    assert_eq!(status, 401);
}

#[tokio::test]
async fn shutting_down_ends_a_waiting_long_poll() {
    let data = tempfile::tempdir().unwrap();
    let hub = kebbie_hub::Hub::open(kebbie_hub::Config::new(data.path())).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(kebbie_hub::serve(
        listener,
        std::sync::Arc::clone(&hub),
        async {
            let _ = stopped.await;
        },
    ));
    let code = hub.mint_join_code().unwrap();
    let mut machine = kebbie_hub_fakes::machine::join(&url, &code, "mac")
        .await
        .unwrap();
    let polling = tokio::spawn(async move { machine.poll_once(25).await.map(|inbox| inbox.seq) });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let started = std::time::Instant::now();
    stop.send(()).unwrap();
    let polled = tokio::time::timeout(SOON, polling).await;
    assert!(polled.is_ok(), "the long-poll held the shutdown");
    tokio::time::timeout(SOON, serving)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn a_dev_hub_refuses_to_listen_beyond_loopback() {
    let data = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kebbie-hub"))
        .args(["--dev", "--listen", "0.0.0.0:0", "--data"])
        .arg(data.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("loopback"));
}

#[test]
fn a_hub_without_dev_refuses_a_plain_http_origin() {
    let data = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kebbie-hub"))
        .args(["--listen", "127.0.0.1:0", "--rp-id", "localhost", "--origin", "http://localhost:8470"])
        .args(["--vapid-subject", "mailto:hub@example.com", "--data"])
        .arg(data.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("must be https://"));
}
