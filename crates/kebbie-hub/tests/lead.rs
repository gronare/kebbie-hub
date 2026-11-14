#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::{Handling, LeadHold, MachineState};
use kebbie_hub_protocol::{
    Away, Board, CommandResult, Delivery, Envelope, KEPT_ANSWER_TTL, LEAD_THREAD_MEDIA_TYPE, LeadMessage, LeadThread, MAX_LEAD_PAGE_TURNS, Outcome, Payload, Turn, open_blob,
};

const SOON: Duration = Duration::from_secs(5);

async fn leading(test: &TestHub) -> (MachineState, Browser) {
    let mut person = member(test).await;
    let mut machine = connected_with_board(test, &mut person).await;
    person.knows(&machine.machine, machine.public());
    machine.post_board(board(MARKER)).await.unwrap();
    (machine, person)
}

fn message(text: &str) -> Payload {
    Payload::LeadMessage(LeadMessage { text: text.into() })
}

async fn shown(person: &mut Browser, machine: &MachineState) -> Board {
    person.boards().await.unwrap().into_iter().find(|board| board.machine == machine.machine).unwrap().board.unwrap()
}

async fn page(person: &mut Browser, blob: &kebbie_hub_protocol::BlobRef) -> LeadThread {
    assert_eq!(blob.media_type, LEAD_THREAD_MEDIA_TYPE);
    LeadThread::read(&open_blob(blob, &person.blob(&blob.id).await.unwrap()).unwrap()).unwrap()
}

async fn thread(person: &mut Browser, machine: &MachineState) -> (Board, LeadThread) {
    let board = shown(person, machine).await;
    let newest = page(person, &board.lead.as_ref().expect("the board points to the lead's thread").thread).await;
    (board, newest)
}

async fn result_of(person: &mut Browser, envelope: &Envelope) -> CommandResult {
    person.await_result(&envelope.header.id, SOON).await.unwrap().expect("a result arrives")
}

#[tokio::test]
async fn a_message_reaches_the_lead_sealed_and_comes_back_in_the_thread_from_this_device() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = leading(&test).await;
    machine.say_at_the_computer("Take refunds through the obstacle flow.");
    machine.lead_replies("Starting with the tests.", false);
    machine.publish_lead().await.unwrap();

    let said = format!("{MARKER} hold the push until I read step 5");
    let envelope = person.command(&machine.machine, &message(&said)).await.unwrap();
    assert_eq!(envelope.header.exp - envelope.header.at, KEPT_ANSWER_TTL, "a message waits a week for a machine that sleeps");
    machine.apply_once(0, None).await.unwrap();
    let result = result_of(&mut person, &envelope).await;
    assert_eq!(result.outcome, Outcome::Done);

    let (board, newest) = thread(&mut person, &machine).await;
    let lead = board.lead.unwrap();
    assert!(lead.working_since.is_some(), "the lead works on the message");
    assert!(lead.replied_at.is_some());
    let mine = newest.turns.last().unwrap();
    let Turn::You { id, device, text, delivery: Some(Delivery::Delivered { .. }), .. } = mine else {
        panic!("the message is delivered: {mine:?}");
    };
    assert_eq!((id.as_str(), device.as_ref(), text.as_deref()), (envelope.header.id.as_str(), Some(person.device_id()), Some(said.as_str())));
    assert!(matches!(&newest.turns[0], Turn::You { device: None, .. }), "the first turn was at the computer");
    assert!(matches!(&newest.turns[1], Turn::Lead { .. }));
    assert!(!holds(&database_bytes(&test), MARKER.as_bytes()), "the hub keeps the thread sealed");
}

#[tokio::test]
async fn a_message_waits_while_the_lead_is_mid_turn_and_its_result_says_delivered_when_it_goes_in() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = leading(&test).await;
    machine.lead.hold = LeadHold::Turn;
    let envelope = person.command(&machine.machine, &message("And ask before step 4.")).await.unwrap();
    let handled = machine.apply_once(0, None).await.unwrap();
    assert!(matches!(handled[0].handling, Handling::Held { .. }), "{handled:?}");
    let (_, newest) = thread(&mut person, &machine).await;
    assert!(matches!(newest.turns.last(), Some(Turn::You { delivery: Some(Delivery::WaitingTurn), .. })));
    assert!(person.await_result(&envelope.header.id, Duration::from_millis(300)).await.unwrap().is_none(), "no result while the message waits");

    machine.lead.hold = LeadHold::Draft;
    let second = person.command(&machine.machine, &message("Or behind the draft.")).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    let (_, newest) = thread(&mut person, &machine).await;
    assert!(matches!(newest.turns.last(), Some(Turn::You { delivery: Some(Delivery::WaitingDraft), .. })));

    machine.release_lead().await.unwrap();
    for sent in [&envelope, &second] {
        assert_eq!(result_of(&mut person, sent).await.outcome, Outcome::Done);
    }
    let (_, newest) = thread(&mut person, &machine).await;
    assert!(newest.turns.iter().all(|turn| matches!(turn, Turn::You { delivery: Some(Delivery::Delivered { .. }), .. })), "{newest:?}");
}

#[tokio::test]
async fn a_message_to_a_sleeping_machine_waits_in_the_hub_and_is_delivered_when_it_wakes() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = leading(&test).await;
    let envelope = person.command(&machine.machine, &message("Run the suite when you wake.")).await.unwrap();
    assert!(person.await_result(&envelope.header.id, Duration::from_millis(300)).await.unwrap().is_none(), "the hub holds it while the machine sleeps");
    machine.apply_once(0, None).await.unwrap();
    assert_eq!(result_of(&mut person, &envelope).await.outcome, Outcome::Done);
}

#[tokio::test]
async fn away_mode_goes_to_every_machine_and_each_lead_says_so_in_its_thread() {
    let test = start_hub(|_| {}).await;
    let (mut first, mut person) = leading(&test).await;
    let mut second = connected_with_board(&test, &mut person).await;
    person.knows(&second.machine, second.public());
    second.post_board(board(MARKER)).await.unwrap();

    let on = Payload::Away(Away { on: true });
    let sent = [person.command(&first.machine, &on).await.unwrap(), person.command(&second.machine, &on).await.unwrap()];
    assert!(sent.iter().all(|envelope| envelope.header.exp - envelope.header.at == KEPT_ANSWER_TTL), "away mode waits for a machine that sleeps");
    for (machine, envelope) in [&mut first, &mut second].into_iter().zip(&sent) {
        machine.apply_once(0, None).await.unwrap();
        assert_eq!(result_of(&mut person, envelope).await.outcome, Outcome::Done);
        let (board, newest) = thread(&mut person, machine).await;
        assert!(board.away_since.is_some());
        assert!(matches!(&newest.turns[..], [Turn::You { away: Some(true), text: None, .. }, Turn::Lead { .. }]), "{newest:?}");
    }

    let off = person.command(&first.machine, &Payload::Away(Away { on: false })).await.unwrap();
    first.apply_once(0, None).await.unwrap();
    assert_eq!(result_of(&mut person, &off).await.outcome, Outcome::Done);
    assert!(shown(&mut person, &first).await.away_since.is_none());
}

#[tokio::test]
async fn a_long_thread_is_paged_and_older_pages_open_on_demand() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = leading(&test).await;
    for turn in 0..MAX_LEAD_PAGE_TURNS + 10 {
        machine.lead_replies(&format!("Reply {turn}"), false);
    }
    machine.publish_lead().await.unwrap();
    let (_, newest) = thread(&mut person, &machine).await;
    assert_eq!(newest.turns.len(), 10);
    let older = page(&mut person, newest.older.as_ref().expect("the newest page points to the one before")).await;
    assert_eq!(older.turns.len(), MAX_LEAD_PAGE_TURNS);
    assert!(older.older.is_none());
    assert!(older.turns.last().unwrap().at() <= newest.turns[0].at());
}
