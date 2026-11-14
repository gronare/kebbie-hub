#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::MachineState;
use kebbie_hub_fakes::now;
use kebbie_hub_protocol::{Answer, Board, CommandResult, Item, KEPT_ANSWER_TTL, Outcome, Payload, StageState};

const SOON: Duration = Duration::from_secs(5);
const STAGES: &str = include_str!("../../../vectors/stages.json");
const STAGED: &str = "email-column@api";

fn staged_board() -> Board {
    let vectors: serde_json::Value = serde_json::from_str(STAGES).unwrap();
    match serde_json::from_value(vectors["board"].clone()).unwrap() {
        Payload::Board(board) => board,
        _ => unreachable!("the vector is a board"),
    }
}

async fn staged(test: &TestHub) -> (MachineState, Browser) {
    let mut person = member(test).await;
    let mut machine = connected_with_board(test, &mut person).await;
    person.knows(&machine.machine, machine.public());
    machine.post_board(staged_board()).await.unwrap();
    (machine, person)
}

fn verify(choice: Option<u8>, comment: &str) -> Payload {
    Payload::Answer(Answer { address: STAGED.into(), question: "verify-backfill".into(), choice, comment: comment.into() })
}

async fn sent(person: &mut Browser, machine: &mut MachineState, payload: Payload) -> CommandResult {
    let envelope = person.command(&machine.machine, &payload).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    person.await_result(&envelope.header.id, SOON).await.unwrap().expect("a result arrives")
}

async fn shown(person: &mut Browser, machine: &MachineState) -> Item {
    let boards = person.boards().await.unwrap();
    let board = boards.into_iter().find(|board| board.machine == machine.machine).unwrap().board.unwrap();
    board.items.into_iter().find(|item| item.address == STAGED).unwrap()
}

fn states(item: &Item) -> Vec<StageState> {
    item.stages.iter().flatten().map(|stage| stage.state).collect()
}

#[tokio::test]
async fn verifying_a_stage_from_the_phone_opens_the_next_and_its_commits_become_pushable() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = staged(&test).await;
    let item = shown(&mut person, &machine).await;
    assert_eq!(states(&item), [StageState::Met, StageState::Waiting, StageState::Locked]);
    assert_eq!(item.pushable, None, "stage 3 waits on stage 2");

    let waiting = sent(&mut person, &mut machine, verify(Some(1), "")).await;
    assert_eq!((waiting.outcome, waiting.text.as_str()), (Outcome::Done, "Fill the old rows waits on your verification."));
    let item = shown(&mut person, &machine).await;
    assert_eq!((states(&item), item.kebbie_questions.as_ref().map(Vec::len)), (vec![StageState::Met, StageState::Waiting, StageState::Locked], Some(1)), "Not yet leaves the question open and stage 3 locked");

    let verified = sent(&mut person, &mut machine, verify(Some(0), "")).await;
    assert_eq!(verified.outcome, Outcome::Done);
    let item = shown(&mut person, &machine).await;
    assert_eq!(states(&item), [StageState::Met, StageState::Met, StageState::Open]);
    assert_eq!(item.kebbie_questions, None, "the verification is settled");
    assert_eq!(item.pushable.map(|run| (run.count, run.subtasks)), Some((2, vec!["read-new".to_string(), "drop-old".to_string()])));
}

#[tokio::test]
async fn something_is_wrong_fails_the_stage_with_what_you_saw_and_keeps_the_next_locked() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = staged(&test).await;
    let wrong = sent(&mut person, &mut machine, verify(Some(2), "Half the rows still have no email\nSeen in the admin")).await;
    assert_eq!(wrong.outcome, Outcome::Done);
    let item = shown(&mut person, &machine).await;
    assert_eq!(states(&item), [StageState::Met, StageState::Failed, StageState::Locked]);
    assert_eq!(item.stages.as_ref().unwrap()[1].note.as_deref(), Some("Half the rows still have no email"));
    assert_eq!(item.pushable, None);
}

#[tokio::test]
async fn an_answer_to_kebbies_question_waits_on_the_hub_past_a_day_for_a_machine_that_sleeps() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = staged(&test).await;
    let at = now();
    let envelope = person.seal_command(&machine.machine, &verify(Some(0), ""), at, Some(KEPT_ANSWER_TTL)).unwrap();
    person.send_command(&envelope).await.unwrap();
    test.hub.store().sweep(at + 3 * 86_400).unwrap();
    assert_eq!(rows(&test, "queue"), 1, "the hub keeps the answer while the machine sleeps for three days");
    machine.apply_once(0, None).await.unwrap();
    let result = person.await_result(&envelope.header.id, SOON).await.unwrap().expect("a result arrives");
    assert_eq!(result.outcome, Outcome::Done);
    assert_eq!(states(&shown(&mut person, &machine).await), [StageState::Met, StageState::Met, StageState::Open]);
    let hour = person.seal_command(&machine.machine, &verify(Some(1), ""), at, None).unwrap();
    person.send_command(&hour).await.unwrap();
    let queued = rows(&test, "queue");
    test.hub.store().sweep(at + 2 * 3_600).unwrap();
    assert_eq!(rows(&test, "queue"), queued - 1, "an answer sent with the hour an agent's question gets is gone once the hour is up");
}

#[tokio::test]
async fn a_close_question_answered_close_takes_the_task_off_the_board() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = staged(&test).await;
    let vectors: serde_json::Value = serde_json::from_str(STAGES).unwrap();
    let asked = vectors["cases"].as_array().unwrap().iter().find(|case| case["name"] == "a manual check and a close, without stages").unwrap();
    let mut item: Item = serde_json::from_value(asked["item"].clone()).unwrap();
    item.address = "docs@api".into();
    let mut asking = staged_board();
    asking.items.push(item);
    machine.post_board(asking.clone()).await.unwrap();
    let close = |choice| Payload::Answer(Answer { address: "docs@api".into(), question: "close-docs".into(), choice: Some(choice), comment: String::new() });
    assert_eq!(sent(&mut person, &mut machine, close(1)).await.text, "docs@api stays open.");
    let boards = person.boards().await.unwrap();
    let board = boards.into_iter().find(|board| board.machine == machine.machine).unwrap().board.unwrap();
    let docs = board.items.iter().find(|item| item.address == "docs@api").unwrap();
    assert_eq!(docs.kebbie_questions.as_ref().map(|questions| questions.iter().map(|question| question.id.as_str()).collect::<Vec<_>>()), Some(vec!["check-mail"]), "Not yet settles the close, and the manual check stays");
    machine.post_board(asking).await.unwrap();
    assert_eq!(sent(&mut person, &mut machine, close(0)).await.text, "Closed docs@api.");
    let boards = person.boards().await.unwrap();
    let board = boards.into_iter().find(|board| board.machine == machine.machine).unwrap().board.unwrap();
    assert!(board.items.iter().all(|item| item.address != "docs@api"));
}
