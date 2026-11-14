#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::{Backlog, MachineState};
use kebbie_hub_protocol::{
    BacklogRow, BacklogState, Board, CARD_MEDIA_TYPE, Card, CommandResult, Found, FoundAdd, FoundKeep, Home, HomeKind, Item, ItemKey, NoticedChoice, Outcome, Payload, Priority, Reason, ReviewSend, open_blob,
};

const SOON: Duration = Duration::from_secs(5);
const CARDS: &str = include_str!("../../../vectors/cards.json");
const CARDED: &str = "payments@api";

fn vectors() -> serde_json::Value {
    serde_json::from_str(CARDS).unwrap()
}

fn carded_board() -> (Board, Item, Card) {
    let Payload::Board(board) = serde_json::from_value(vectors()["board"].clone()).unwrap() else {
        unreachable!("the vector is a board")
    };
    let item = board.items[0].clone();
    let card = serde_json::from_value(vectors()["card"].clone()).unwrap();
    (board, item, card)
}

fn found(number: u32, title: &str) -> BacklogRow {
    BacklogRow {
        item: ItemKey::parse(&format!("found:f{number}")).unwrap(),
        title: title.to_string(),
        priority: Priority::None,
        repo: Some("api".to_string()),
        state: BacklogState::Ready,
        filed: 1,
        home: Home { kind: HomeKind::Found, label: "found by tasks".to_string() },
        questions: 0,
        blocking: 0,
        goal: None,
        body: None,
        found: Some(Found { sources: vec![CARDED.to_string(), "refunds@api".to_string()], count: 2 }),
    }
}

async fn carded(test: &TestHub) -> (MachineState, Browser) {
    let mut person = member(test).await;
    let mut machine = connected_with_board(test, &mut person).await;
    person.knows(&machine.machine, machine.public());
    let (board, item, card) = carded_board();
    machine.post_board(Board { items: Vec::new(), ..board }).await.unwrap();
    machine.backlog = Backlog { rows: vec![found(3, "The refund mail names the old gateway"), found(4, "settle_batch writes outside a transaction")], bodies: Default::default() };
    machine.publish_backlog().await.unwrap();
    machine.post_card(item, card).await.unwrap();
    (machine, person)
}

async fn sent(person: &mut Browser, machine: &mut MachineState, payload: Payload) -> CommandResult {
    let envelope = person.command(&machine.machine, &payload).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    person.await_result(&envelope.header.id, SOON).await.unwrap().expect("a result arrives")
}

async fn shown(person: &mut Browser, machine: &MachineState) -> Item {
    let boards = person.boards().await.unwrap();
    let board = boards.into_iter().find(|board| board.machine == machine.machine).unwrap().board.unwrap();
    board.items.into_iter().find(|item| item.address == CARDED).unwrap()
}

async fn card_of(person: &mut Browser, item: &Item) -> Card {
    let blob = &item.card.as_ref().expect("the task has a card").blob;
    assert_eq!(blob.media_type, CARD_MEDIA_TYPE);
    Card::read(&open_blob(blob, &person.blob(&blob.id).await.unwrap()).unwrap()).unwrap()
}

#[tokio::test]
async fn a_sign_off_card_travels_as_a_sealed_blob_and_opens_on_the_phone_as_data() {
    let test = start_hub(|_| {}).await;
    let (machine, mut person) = carded(&test).await;
    let item = shown(&mut person, &machine).await;
    let card = card_of(&mut person, &item).await;
    assert_eq!(card, carded_board().2);
    assert_eq!(card.previous.as_ref().map(|previous| previous.version), Some(1), "a changed card carries the version before it");
}

#[tokio::test]
async fn a_noticed_line_is_kept_in_the_backlog_or_added_as_a_subtask_and_the_card_says_which() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = carded(&test).await;
    let key = |number: u32| ItemKey::parse(&format!("found:f{number}")).unwrap();

    assert_eq!(sent(&mut person, &mut machine, Payload::FoundKeep(FoundKeep { item: key(3) })).await.outcome, Outcome::Done);
    let item = shown(&mut person, &machine).await;
    let card = card_of(&mut person, &item).await;
    assert_eq!(card.noticed[0].chosen, Some(NoticedChoice::Kept));
    assert_eq!(card.version, 2, "a choice on a line is not a change to the card");
    let backlog = person.boards().await.unwrap().into_iter().find(|board| board.machine == machine.machine).unwrap().board.unwrap().backlog.unwrap();
    assert!(backlog.iter().any(|row| row.item == key(3)), "a kept item stays in the backlog");

    assert_eq!(sent(&mut person, &mut machine, Payload::FoundAdd(FoundAdd { item: key(3), task: CARDED.into() })).await.outcome, Outcome::Done);
    let item = shown(&mut person, &machine).await;
    let card = card_of(&mut person, &item).await;
    assert_eq!(card.noticed[0].chosen, Some(NoticedChoice::Subtask));
    assert_eq!(sent(&mut person, &mut machine, Payload::FoundKeep(FoundKeep { item: key(3) })).await.outcome, Outcome::Refused { reason: Reason::UnknownItem }, "an item already added is no longer in the backlog to keep");
}

#[tokio::test]
async fn send_to_review_starts_the_full_review_once() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = carded(&test).await;
    let send = || Payload::ReviewSend(ReviewSend { address: CARDED.into(), subtask: None });
    assert_eq!(sent(&mut person, &mut machine, send()).await.outcome, Outcome::Done);
    assert!(shown(&mut person, &machine).await.verdict.is_some_and(|verdict| verdict.running), "the board shows the review running");
    assert_eq!(sent(&mut person, &mut machine, send()).await.outcome, Outcome::Refused { reason: Reason::AlreadySettled });
}
