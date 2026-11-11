#[allow(dead_code)]
mod common;

use std::time::Duration;

use axum::Router;
use common::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use kebbie_hub_api::{Bytes64, DeviceListView, HubInfo, Inbox, SignedDeviceList};
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::{Handling, MachineState};
use kebbie_hub_fakes::phone::PhoneState;
use kebbie_hub_fakes::{Error, HubClient, HubError};
use kebbie_hub_protocol::{
    Answer, DeviceList, ListedDevice, Outcome, Payload, Role, SecretKeys,
};

const SOON: Duration = Duration::from_secs(5);

fn on(config: &mut kebbie_hub::Config) {
    config.connect_interval = 0;
}

fn answer() -> Payload {
    Payload::Answer(Answer {
        address: "refunds@shop".to_string(),
        question: "1761069540-4411-0-question".to_string(),
        choice: Some(0),
        comment: String::new(),
    })
}

fn database(test: &TestHub) -> rusqlite::Connection {
    rusqlite::Connection::open(test.hub.config().database()).unwrap()
}

fn refusal<T: std::fmt::Debug>(result: Result<T, Error>) -> (u16, String) {
    match result {
        Err(Error::Hub(error)) => hub_code(error),
        other => panic!("the hub accepted it: {other:?}"),
    }
}

fn browser_refusal<T: std::fmt::Debug>(result: Result<T, HubError>) -> (u16, String) {
    match result {
        Err(error) => hub_code(error),
        other => panic!("the hub accepted it: {other:?}"),
    }
}

async fn account_with_machine(test: &TestHub) -> (Browser, MachineState) {
    let mut person = member(test).await;
    let mut machine = connected_with_board(test, &mut person).await;
    machine.post_board(board(MARKER)).await.unwrap();
    (person, machine)
}

async fn listed_phone(person: &mut Browser, machine: &MachineState) -> PhoneState {
    let mut phone = person.enable_device().await.unwrap();
    phone.knows(&machine.machine, machine.secret().public());
    phone
}

#[tokio::test]
async fn a_device_on_the_signed_list_is_admitted_and_its_command_applied() {
    let test = start_hub(on).await;
    let (mut person, mut machine) = account_with_machine(&test).await;
    let phone = listed_phone(&mut person, &machine).await;

    assert_eq!(machine.sync_devices().await.unwrap(), 1);
    let sent = phone.command(&machine.machine, &answer()).await.unwrap();
    let handled = machine.apply_once(0, None).await.unwrap();
    assert!(
        matches!(&handled[0].handling, Handling::Applied { result, .. } if result.outcome == Outcome::Done),
        "{:?}",
        handled[0].handling
    );
    let result = phone.await_result(&sent.header.id, SOON).await.unwrap();
    assert_eq!(result.map(|result| result.outcome), Some(Outcome::Done));
}

#[tokio::test]
async fn a_device_taken_off_the_list_is_refused_and_can_then_be_deleted() {
    let test = start_hub(on).await;
    let (mut person, machine) = account_with_machine(&test).await;
    let phone = listed_phone(&mut person, &machine).await;
    let device = phone.device.clone().unwrap();
    assert_eq!(
        browser_refusal(person.delete(&format!("/v1/a/devices/{device}")).await),
        (409, "still-listed".to_string())
    );

    let list = person.device_list().await.unwrap().without(&device);
    person.publish_devices(&list).await.unwrap();
    assert_eq!(
        refusal(phone.command(&machine.machine, &answer()).await),
        (404, "unknown-machine".to_string())
    );
    person.delete(&format!("/v1/a/devices/{device}")).await.unwrap();
}

#[tokio::test]
async fn a_device_of_another_account_is_refused_and_nothing_is_queued() {
    let test = start_hub(on).await;
    let (_, machine) = account_with_machine(&test).await;
    let mut stranger = member(&test).await;
    let phone = listed_phone(&mut stranger, &machine).await;

    assert_eq!(
        refusal(phone.command(&machine.machine, &answer()).await),
        (404, "unknown-machine".to_string())
    );
    assert_eq!(rows(&test, "queue"), 0);
    assert_eq!(rows(&test, "commands"), 0);
}

#[tokio::test]
async fn a_list_from_another_account_or_device_is_refused() {
    let test = start_hub(on).await;
    let (mut person, _) = account_with_machine(&test).await;
    person.enable_device().await.unwrap();
    let mut stranger = member(&test).await;
    let theirs = stranger.ensure_device().await.unwrap().clone();
    let list = person.device_list().await.unwrap().with(ListedDevice {
        device: theirs.device.clone().unwrap(),
        name: "theirs".into(),
        keys: theirs.public(),
        role: Role::Control,
    });
    assert_eq!(
        browser_refusal(person.publish_devices(&list).await),
        (400, "unknown-device".to_string())
    );

    let mine = person.ensure_device().await.unwrap().clone();
    let swapped = person.device_list().await.unwrap().with(ListedDevice {
        device: mine.device.clone().unwrap(),
        name: "browser".into(),
        keys: SecretKeys::generate().public(),
        role: Role::Control,
    });
    assert_eq!(
        browser_refusal(person.publish_devices(&swapped).await),
        (400, "unknown-device".to_string()),
        "keys other than the registered ones"
    );
}

#[tokio::test]
async fn a_list_with_a_lower_counter_or_another_key_is_refused() {
    let test = start_hub(on).await;
    let (mut person, _) = account_with_machine(&test).await;
    person.enable_device().await.unwrap();
    let held = person.device_list().await.unwrap();
    assert_eq!(held.counter, 1);

    let replayed = DeviceList {
        counter: held.counter,
        ..held.clone()
    };
    assert_eq!(
        browser_refusal(person.publish_devices(&replayed).await),
        (409, "stale-list".to_string())
    );
    let lower = DeviceList {
        counter: 0,
        ..held.clone()
    };
    assert_eq!(
        browser_refusal(person.publish_devices(&lower).await),
        (409, "stale-list".to_string())
    );

    let account = person.account_key().unwrap().account.clone();
    let next = held.without(&held.devices[0].device).with(held.devices[0].clone());
    let forged = next.sign(&SecretKeys::generate(), &account).unwrap();
    let put = person
        .put::<_, Option<()>>(
            "/v1/a/device-list",
            &SignedDeviceList {
                list: next,
                signature: Bytes64(forged),
            },
        )
        .await;
    assert_eq!(browser_refusal(put), (400, "bad-signature".to_string()));
}

#[tokio::test]
async fn a_machine_refuses_a_list_the_hub_rolled_back_or_forged() {
    let test = start_hub(on).await;
    let (mut person, mut machine) = account_with_machine(&test).await;
    person.enable_device().await.unwrap();
    machine.sync_devices().await.unwrap();
    let (old_list, old_signature): (String, Vec<u8>) = database(&test)
        .query_row("SELECT list, signature FROM device_lists", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    let list = person.device_list().await.unwrap();
    person
        .publish_devices(&list.without(&list.devices[0].device))
        .await
        .unwrap();
    assert_eq!(machine.sync_devices().await.unwrap(), 2);

    database(&test)
        .execute(
            "UPDATE device_lists SET list = ?1, signature = ?2, counter = 1",
            rusqlite::params![old_list, old_signature],
        )
        .unwrap();
    assert!(machine.sync_devices().await.is_err(), "a rolled back list");
    database(&test)
        .execute(
            "UPDATE device_lists SET signature = ?1",
            rusqlite::params![vec![7u8; 64]],
        )
        .unwrap();
    assert!(machine.sync_devices().await.is_err(), "a forged list");
}

#[tokio::test]
async fn changing_the_list_needs_a_fresh_passkey_sign_in() {
    let test = start_hub(on).await;
    let (mut person, _) = account_with_machine(&test).await;
    person.ensure_device().await.unwrap();
    database(&test)
        .execute("UPDATE sessions SET created_at = created_at - 3600", [])
        .unwrap();
    assert_eq!(
        browser_refusal(person.enable_device().await.map(drop)),
        (401, "reauth-required".to_string())
    );
    assert_eq!(rows(&test, "device_lists"), 0);
    person.log_in().await.unwrap();
    person.enable_device().await.unwrap();
    assert_eq!(rows(&test, "device_lists"), 1);
}

#[tokio::test]
async fn the_machine_reads_the_list_and_its_inbox_wakes_when_it_changes() {
    let test = start_hub(on).await;
    let (mut person, machine) = account_with_machine(&test).await;
    let client = machine.client();
    let empty: DeviceListView = client.get("/v1/m/device-list", &[]).await.unwrap();
    assert_eq!(empty.list, None);
    let inbox: Inbox = client.get("/v1/m/inbox", &[]).await.unwrap();
    assert_eq!(inbox.device_list_rev, None);

    let waiting = tokio::spawn(async move {
        let started = tokio::time::Instant::now();
        let inbox: Inbox = client
            .get(
                "/v1/m/inbox",
                &[("wait", "10".to_string()), ("devices", "0".to_string())],
            )
            .await
            .unwrap();
        (inbox.device_list_rev, started.elapsed())
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    person.enable_device().await.unwrap();
    let (rev, waited) = waiting.await.unwrap();
    assert_eq!(rev, Some(1));
    assert!(waited < Duration::from_secs(5), "{waited:?}");
}

#[tokio::test]
async fn with_commands_off_the_hub_refuses_them_and_says_so() {
    let test = start_hub(|config| {
        on(config);
        config.commands = false;
    })
    .await;
    let info: kebbie_hub_api::HubInfo = browser(&test).get("/v1/hub").await.unwrap();
    assert!(!info.commands);
    let (mut person, machine) = account_with_machine(&test).await;
    let phone = listed_phone(&mut person, &machine).await;
    assert_eq!(
        refusal(phone.command(&machine.machine, &answer()).await),
        (404, "commands-off".to_string())
    );
    assert_eq!(rows(&test, "commands"), 0);
}

#[tokio::test]
async fn a_command_gate_refuses_a_closed_accounts_commands_with_402_and_its_url() {
    let closed: Arc<Mutex<HashSet<String>>> = Arc::default();
    let shut = Arc::clone(&closed);
    let gate: kebbie_hub::CommandGate = Box::new(move |_, account| {
        if shut.lock().unwrap().contains(account) {
            Err(kebbie_hub::Refusal {
                url: Some("https://hub.example.com/details".to_string()),
            })
        } else {
            Ok(())
        }
    });
    let test = start_hub_with(on, Some(gate), Router::new()).await;
    let info: HubInfo = HubClient::new(&test.url, None).get("/v1/hub", &[]).await.unwrap();
    assert!(info.plans, "the hub says it gates commands");
    let (mut person, machine) = account_with_machine(&test).await;
    let phone = listed_phone(&mut person, &machine).await;
    let account = person.account().await.unwrap().account;
    closed.lock().unwrap().insert(account.clone());
    let refused = phone.command(&machine.machine, &answer()).await.unwrap_err();
    assert!(
        matches!(&refused, Error::Hub(HubError::CommandsRefused { url: Some(url) }) if url == "https://hub.example.com/details"),
        "{refused:?}"
    );
    assert_eq!(
        refusal(Err::<(), _>(refused)),
        (402, kebbie_hub_api::PLAN_INACTIVE.to_string())
    );
    assert_eq!(rows(&test, "commands"), 0);

    closed.lock().unwrap().remove(&account);
    phone.command(&machine.machine, &answer()).await.unwrap();
}

#[tokio::test]
async fn without_a_gate_the_hub_says_so_and_takes_every_accounts_commands() {
    let test = start_hub(on).await;
    let info: HubInfo = HubClient::new(&test.url, None).get("/v1/hub", &[]).await.unwrap();
    assert!(!info.plans);
    let (mut person, machine) = account_with_machine(&test).await;
    let phone = listed_phone(&mut person, &machine).await;
    phone.command(&machine.machine, &answer()).await.unwrap();
}
