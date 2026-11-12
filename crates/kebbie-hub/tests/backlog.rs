#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::{Backlog, MachineState};
use kebbie_hub_protocol::{
    BacklogBody, BacklogRow, BacklogState, Board, Home, HomeKind, Idea, ItemAnswer, ItemKey, OpenQuestion, Outcome,
    Payload, Priority, Reason, StartItem, open_blob,
};

const SOON: Duration = Duration::from_secs(5);
const ITEM: &str = "local:retry-a-flaky-ci-push";

fn backlog() -> Backlog {
    let item = ItemKey::parse(ITEM).unwrap();
    let question = |text: &str, blocks| OpenQuestion { text: text.to_string(), blocks, pending_answer: None };
    Backlog {
        rows: vec![BacklogRow {
            item: item.clone(),
            title: "Retry a flaky CI push".to_string(),
            priority: Priority::Medium,
            repo: Some("shop".to_string()),
            state: BacklogState::Drafting,
            filed: 1,
            home: Home { kind: HomeKind::Local, label: ".kebbie/backlog/retry.md".to_string() },
            questions: 0,
            blocking: 0,
            goal: None,
            body: None,
        }],
        bodies: [(item, BacklogBody {
            background: "A push stops when the runner drops.".to_string(),
            goal: "Retry a dropped runner.".to_string(),
            what_we_know: "- Three of ten failures were drops.".to_string(),
            questions: vec![question("How many tries?", true), question("Wait for the runner?", true), question("Log each retry?", false)],
            out_of_scope: "Flaky tests.".to_string(),
            source: "From the board.".to_string(),
        })]
        .into(),
    }
}

async fn board_of(person: &mut Browser, machine: &MachineState) -> Board {
    let boards = person.boards().await.unwrap();
    boards.into_iter().find(|board| board.machine == machine.machine).unwrap().board.unwrap()
}

async fn sent(person: &mut Browser, machine: &mut MachineState, payload: Payload) -> Outcome {
    let envelope = person.command(&machine.machine, &payload).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    person.await_result(&envelope.header.id, SOON).await.unwrap().expect("a result arrives").outcome
}

fn start(anyway: bool) -> Payload {
    Payload::StartItem(StartItem { item: ItemKey::parse(ITEM).unwrap(), anyway })
}

#[tokio::test]
async fn an_idea_an_answer_and_a_start_travel_through_the_hub_and_the_board_shows_each() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    let mut machine = connected_with_board(&test, &mut person).await;
    person.knows(&machine.machine, machine.public());
    machine.post_board(board(MARKER)).await.unwrap();
    machine.backlog = backlog();
    machine.publish_backlog().await.unwrap();

    let shown = board_of(&mut person, &machine).await;
    let row = &shown.backlog.as_ref().unwrap()[0];
    assert_eq!((row.questions, row.blocking), (3, 2));
    let body_ref = row.body.clone().unwrap();
    let body: BacklogBody = serde_json::from_slice(&open_blob(&body_ref, &person.blob(&body_ref.id).await.unwrap()).unwrap()).unwrap();
    assert_eq!(body.questions.len(), 3);
    assert!(shown.accepts.as_ref().unwrap().contains(&kebbie_hub_protocol::Kind::StartItem));

    let filed = sent(&mut person, &mut machine, Payload::Idea(Idea { text: "Mail a weekly summary".to_string(), repo: None })).await;
    assert_eq!(filed, Outcome::Done);
    let rows = board_of(&mut person, &machine).await.backlog.unwrap();
    assert_eq!((rows[0].title.as_str(), rows[0].state, rows[0].home.kind), ("Mail a weekly summary", BacklogState::ToWrite, HomeKind::Kept));

    assert_eq!(sent(&mut person, &mut machine, start(false)).await, Outcome::Refused { reason: Reason::QuestionsOpen { blocking: 2 } });

    let answer = |index: u32, question: &str| Payload::ItemAnswer(ItemAnswer { item: ItemKey::parse(ITEM).unwrap(), index, question: question.to_string(), answer: "Two".to_string() });
    assert_eq!(sent(&mut person, &mut machine, answer(0, "How many tries?")).await, Outcome::Done);
    assert_eq!(sent(&mut person, &mut machine, answer(1, "Something else?")).await, Outcome::Refused { reason: Reason::QuestionChanged });
    let rows = board_of(&mut person, &machine).await.backlog.unwrap();
    let row = rows.iter().find(|row| row.item.as_str() == ITEM).unwrap();
    assert_eq!(row.blocking, 1, "an answer with the lead no longer blocks");
    assert_eq!(sent(&mut person, &mut machine, start(false)).await, Outcome::Refused { reason: Reason::QuestionsOpen { blocking: 1 } });

    assert_eq!(sent(&mut person, &mut machine, start(true)).await, Outcome::Done);
    let shown = board_of(&mut person, &machine).await;
    assert!(shown.backlog.unwrap().iter().all(|row| row.item.as_str() != ITEM), "a started item leaves the backlog");
    assert_eq!(shown.items[0].title.as_deref(), Some("Retry a flaky CI push"));
    assert_eq!(sent(&mut person, &mut machine, start(true)).await, Outcome::Refused { reason: Reason::AlreadyStarted });
}
