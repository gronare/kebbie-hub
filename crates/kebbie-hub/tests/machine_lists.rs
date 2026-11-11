#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub_api::{Boards, Bytes64, EVENT_MACHINES, SignedMachineList};
use kebbie_hub_fakes::browser::{BoardProblem, Browser};
use kebbie_hub_protocol::{Id, MachineList, SecretKeys};

fn database(test: &TestHub) -> rusqlite::Connection {
    rusqlite::Connection::open(test.hub.config().database()).unwrap()
}

fn problem(boards: &[kebbie_hub_fakes::browser::MachineBoard], machine: &Id) -> Option<String> {
    let entry = boards.iter().find(|entry| &entry.machine == machine)?;
    Some(match &entry.board {
        Ok(_) => "readable".to_string(),
        Err(BoardProblem::UnconfirmedMachine) => "unconfirmed".to_string(),
        Err(BoardProblem::RolledBackList) => "rolled-back".to_string(),
        Err(other) => other.to_string(),
    })
}

fn save_machine(test: &TestHub, machine: &Id) {
    database(test)
        .execute_batch(&format!(
            "CREATE TABLE saved_machine AS SELECT * FROM machines WHERE id = '{machine}';
             CREATE TABLE saved_board AS SELECT * FROM boards WHERE machine_id = '{machine}';"
        ))
        .unwrap();
}

fn restore_machine(test: &TestHub) {
    database(test)
        .execute_batch(
            "INSERT INTO machines SELECT * FROM saved_machine;
             INSERT INTO boards SELECT * FROM saved_board;",
        )
        .unwrap();
}

async fn put_list(person: &mut Browser, list: &MachineList, secret: &SecretKeys) -> (u16, String) {
    let account = person.account_key().unwrap().account.clone();
    let signature = list.sign(secret, &account).unwrap();
    match person
        .put::<_, Option<()>>(
            "/v1/a/machines",
            &SignedMachineList {
                list: list.clone(),
                signature: Bytes64(signature),
            },
        )
        .await
    {
        Ok(_) => (204, String::new()),
        Err(error) => hub_code(error),
    }
}

fn secret_of(person: &Browser) -> SecretKeys {
    let (signing, encryption) = person.account_secret().unwrap();
    SecretKeys::from_bytes(signing, encryption)
}

#[tokio::test]
async fn only_machines_on_the_signed_list_with_their_listed_keys_are_trusted() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let mut listed = connected_with_board(&test, &mut person).await;
    let mut unlisted = connected(&test, &mut person).await;
    listed.post_board(board(MARKER)).await.unwrap();
    unlisted.account = person.account_key().cloned();
    unlisted.post_board(board(MARKER)).await.unwrap();
    let boards = person.boards().await.unwrap();
    assert_eq!(
        problem(&boards, &listed.machine).as_deref(),
        Some("readable")
    );
    assert_eq!(
        problem(&boards, &unlisted.machine).as_deref(),
        Some("unconfirmed")
    );

    let substitute = SecretKeys::generate().public();
    database(&test)
        .execute(
            "UPDATE machines SET signing_key = ?1 WHERE id = ?2",
            rusqlite::params![substitute.signing.as_slice(), listed.machine.as_str()],
        )
        .unwrap();
    let boards = person.boards().await.unwrap();
    assert_eq!(
        problem(&boards, &listed.machine).as_deref(),
        Some("unconfirmed"),
        "a key the hub swapped is not the listed one"
    );
}

#[tokio::test]
async fn a_machine_removed_from_the_web_stays_untrusted_when_the_hub_serves_it_again() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let mut kept = connected_with_board(&test, &mut person).await;
    kept.post_board(board(MARKER)).await.unwrap();
    let mut removed = connected_with_board(&test, &mut person).await;
    removed.post_board(board(MARKER)).await.unwrap();
    assert_eq!(
        problem(&person.boards().await.unwrap(), &removed.machine).as_deref(),
        Some("readable")
    );
    let old_list: String = database(&test)
        .query_row("SELECT list FROM machine_lists", [], |row| row.get(0))
        .unwrap();
    let old_signature: Vec<u8> = database(&test)
        .query_row("SELECT signature FROM machine_lists", [], |row| row.get(0))
        .unwrap();
    save_machine(&test, &removed.machine);

    person.remove_machine(&removed.machine).await.unwrap();
    assert!(
        removed.poll_once(0).await.is_err(),
        "the removed machine's token is gone"
    );
    restore_machine(&test);
    let boards = person.boards().await.unwrap();
    assert_eq!(
        problem(&boards, &removed.machine).as_deref(),
        Some("unconfirmed")
    );
    assert_eq!(problem(&boards, &kept.machine).as_deref(), Some("readable"));

    database(&test)
        .execute(
            "UPDATE machine_lists SET list = ?1, signature = ?2, counter = 2",
            rusqlite::params![old_list, old_signature],
        )
        .unwrap();
    let boards = person.boards().await.unwrap();
    assert_eq!(
        problem(&boards, &removed.machine).as_deref(),
        Some("rolled-back"),
        "an older signed list is refused"
    );
    assert_eq!(
        problem(&boards, &kept.machine).as_deref(),
        Some("rolled-back")
    );
}

#[tokio::test]
async fn a_machine_the_hub_removed_is_dropped_from_the_list_by_the_next_unlocked_browser() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let kept = connected_with_board(&test, &mut person).await;
    let mut gone = connected_with_board(&test, &mut person).await;
    gone.post_board(board(MARKER)).await.unwrap();
    save_machine(&test, &gone.machine);
    assert!(test.hub.store().remove_machine(&gone.machine).unwrap());

    person.forget_keys();
    person.log_in().await.unwrap();
    let later = &mut person;
    let boards = later.boards().await.unwrap();
    assert!(problem(&boards, &gone.machine).is_none());
    let listing: Boards = later.get("/v1/a/boards").await.unwrap();
    let list = listing.list.unwrap().list;
    assert_eq!(list.counter, 3);
    assert!(list.trusts(&kept.machine, &kept.public()));
    assert!(
        !list
            .machines
            .iter()
            .any(|listed| listed.machine == gone.machine)
    );

    restore_machine(&test);
    let boards = later.boards().await.unwrap();
    assert_eq!(
        problem(&boards, &gone.machine).as_deref(),
        Some("unconfirmed")
    );
}

#[tokio::test]
async fn the_hub_keeps_only_a_newer_list_signed_by_the_account_naming_its_own_machines() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let machine = connected_with_board(&test, &mut person).await;
    let mut stranger = member(&test).await;
    let theirs = connected_with_board(&test, &mut stranger).await;
    let secret = secret_of(&person);
    let current = MachineList::default().with(machine.machine.clone(), machine.public());
    assert_eq!(current.counter, 1);
    assert_eq!(
        put_list(&mut person, &current, &secret).await,
        (409, "stale-list".to_string())
    );
    let foreign = current.with(theirs.machine.clone(), theirs.public());
    assert_eq!(
        put_list(&mut person, &foreign, &secret).await,
        (400, "unknown-machine".to_string())
    );
    let forged = current.without(&theirs.machine);
    assert_eq!(
        put_list(&mut person, &forged, &SecretKeys::generate()).await,
        (400, "bad-signature".to_string())
    );
    let mut events = EventStream::of_account(&person).await;
    events.until(EVENT_MACHINES, Duration::from_secs(5)).await;
    assert_eq!(
        put_list(&mut person, &forged, &secret).await,
        (204, String::new())
    );
    let changed = events.until(EVENT_MACHINES, Duration::from_secs(5)).await;
    assert_eq!(changed["counter"], 2);
}

#[tokio::test]
async fn a_machine_is_removed_from_the_web_only_once_the_list_dropped_it() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let machine = connected_with_board(&test, &mut person).await;
    let mut stranger = member(&test).await;
    let theirs = connected_with_board(&test, &mut stranger).await;
    let still = person
        .delete(&format!("/v1/a/machines/{}", machine.machine))
        .await;
    assert_eq!(
        hub_code(still.unwrap_err()),
        (409, "still-listed".to_string())
    );
    let foreign = person
        .delete(&format!("/v1/a/machines/{}", theirs.machine))
        .await;
    assert_eq!(hub_code(foreign.unwrap_err()).0, 404);
    person.remove_machine(&machine.machine).await.unwrap();
    assert!(person.boards().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_machine_leaves_with_its_own_token_and_the_next_browser_drops_it_from_the_list() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let mut kept = connected_with_board(&test, &mut person).await;
    let mut leaving = connected_with_board(&test, &mut person).await;
    kept.post_board(board(MARKER)).await.unwrap();
    leaving.post_board(board(MARKER)).await.unwrap();

    leaving.leave().await.unwrap();
    let refused = leaving.post_board(board(MARKER)).await.unwrap_err();
    assert!(refused.to_string().contains("401"), "{refused}");
    let again = leaving.client().post_empty("/v1/m/leave", &()).await;
    assert_eq!(hub_code(again.unwrap_err()).0, 401, "the token went with the machine");
    let left: Vec<_> = test
        .hub
        .store()
        .events()
        .unwrap()
        .into_iter()
        .filter(|(kind, _)| kind == "machine.left")
        .collect();
    assert_eq!(left.len(), 1);

    person.forget_keys();
    person.log_in().await.unwrap();
    let boards = person.boards().await.unwrap();
    assert!(problem(&boards, &leaving.machine).is_none());
    assert_eq!(problem(&boards, &kept.machine).as_deref(), Some("readable"));
    let list = person
        .get::<Boards>("/v1/a/boards")
        .await
        .unwrap()
        .list
        .unwrap()
        .list;
    assert!(list.trusts(&kept.machine, &kept.public()));
    assert!(!list.machines.iter().any(|listed| listed.machine == leaving.machine));
}

#[tokio::test]
async fn a_machine_that_joined_with_a_code_leaves_too() {
    let test = start_hub(|_| {}).await;
    let machine = joined_machine(&test).await;
    machine.leave().await.unwrap();
    let anonymous = kebbie_hub_fakes::HubClient::new(&test.url, None)
        .post_empty("/v1/m/leave", &())
        .await;
    assert_eq!(hub_code(anonymous.unwrap_err()).0, 401);
}
