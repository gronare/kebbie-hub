#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::{Backlog, MachineState};
use kebbie_hub_protocol::{
    BacklogBody, BacklogRow, BacklogState, Board, BodySection, Found, FoundAdd, FoundDismiss, FoundStart, Home, HomeKind, Idea, ItemAnswer, ItemEdit, ItemKey, OpenQuestion,
    Outcome, Payload, Priority, Reason, StartItem, open_blob, text_hash,
};

const SOON: Duration = Duration::from_secs(5);
const ITEM: &str = "local:retry-a-flaky-ci-push";

fn section(key: &str, heading: &str, text: &str, required: bool) -> BodySection {
    BodySection { key: key.to_string(), heading: heading.to_string(), text: text.to_string(), required }
}

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
            found: None,
        }],
        bodies: [(item, BacklogBody {
            sections: vec![
                section("problem", "Problem", "A push stops when the runner drops.", true),
                section("solution", "Solution", "Retry a dropped runner.", true),
                section("questions", "Open questions", "", false),
                section("out-of-scope", "Out of scope", "Flaky tests.", false),
            ],
            questions: vec![question("How many tries?", true), question("Wait for the runner?", true), question("Log each retry?", false)],
            questions_key: Some("questions".to_string()),
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

fn found(number: u32, title: &str) -> BacklogRow {
    BacklogRow {
        item: ItemKey::parse(&format!("found:f{number}")).unwrap(),
        title: title.to_string(),
        priority: Priority::None,
        repo: Some("shop".to_string()),
        state: BacklogState::Ready,
        filed: 1,
        home: Home { kind: HomeKind::Found, label: "found by tasks".to_string() },
        questions: 0,
        blocking: 0,
        goal: None,
        body: None,
        found: Some(Found { sources: vec!["refunds@shop".to_string()], count: 2 }),
    }
}

#[tokio::test]
async fn what_tasks_found_is_started_added_to_a_task_or_dismissed_from_the_phone() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    let mut machine = connected_with_board(&test, &mut person).await;
    person.knows(&machine.machine, machine.public());
    machine.post_board(board(MARKER)).await.unwrap();
    machine.backlog = Backlog { rows: vec![found(1, "Receipts drop the currency"), found(2, "The client can't tell an outage"), found(3, "Logs repeat the token")], bodies: Default::default() };
    machine.publish_backlog().await.unwrap();

    let shown = board_of(&mut person, &machine).await.backlog.unwrap();
    assert_eq!(shown[0].found.as_ref().map(|found| (found.sources.clone(), found.count)), Some((vec!["refunds@shop".to_string()], 2)));
    let key = |number: u32| ItemKey::parse(&format!("found:f{number}")).unwrap();
    let address = shown_task(&mut person, &machine).await;
    assert_eq!(sent(&mut person, &mut machine, Payload::FoundAdd(FoundAdd { item: key(2), task: address })).await, Outcome::Done);
    assert_eq!(sent(&mut person, &mut machine, Payload::FoundDismiss(FoundDismiss { item: key(3) })).await, Outcome::Done);
    assert_eq!(sent(&mut person, &mut machine, Payload::FoundStart(FoundStart { item: key(1), name: Some("receipt-currency".to_string()) })).await, Outcome::Done);
    let shown = board_of(&mut person, &machine).await;
    assert!(shown.backlog.unwrap().is_empty(), "each settled found item leaves the backlog");
    assert_eq!(shown.items[0].address, "receipt-currency@shop");
    assert_eq!(sent(&mut person, &mut machine, Payload::FoundDismiss(FoundDismiss { item: key(3) })).await, Outcome::Refused { reason: Reason::UnknownItem });
}

async fn shown_task(person: &mut Browser, machine: &MachineState) -> String {
    board_of(person, machine).await.items[0].address.clone()
}

async fn body_of(person: &mut Browser, machine: &MachineState) -> BacklogBody {
    let rows = board_of(person, machine).await.backlog.unwrap();
    let body_ref = rows.iter().find(|row| row.item.as_str() == ITEM).unwrap().body.clone().unwrap();
    serde_json::from_slice(&open_blob(&body_ref, &person.blob(&body_ref.id).await.unwrap()).unwrap()).unwrap()
}

fn edit(section: Option<&str>, text: &str, base: &str) -> Payload {
    Payload::ItemEdit(ItemEdit { item: ItemKey::parse(ITEM).unwrap(), section: section.map(Into::into), text: text.to_string(), base: text_hash(base) })
}

#[tokio::test]
async fn an_edit_replaces_a_section_or_the_title_only_from_the_text_the_machine_still_has() {
    let test = start_hub(|_| {}).await;
    let mut person = member(&test).await;
    let mut machine = connected_with_board(&test, &mut person).await;
    person.knows(&machine.machine, machine.public());
    machine.post_board(board(MARKER)).await.unwrap();
    machine.backlog = backlog();
    machine.publish_backlog().await.unwrap();

    let solution = "Retry a dropped runner up to twice.";
    assert_eq!(sent(&mut person, &mut machine, edit(Some("solution"), solution, "Retry a dropped runner.")).await, Outcome::Done);
    let body = body_of(&mut person, &machine).await;
    assert_eq!(body.section("solution").map(|one| one.text.as_str()), Some(solution));
    assert_eq!(body.sections.iter().map(|one| one.key.as_str()).collect::<Vec<_>>(), ["problem", "solution", "questions", "out-of-scope"], "an edit keeps the template's order");

    assert_eq!(sent(&mut person, &mut machine, edit(Some("solution"), "Retry forever.", "Retry a dropped runner.")).await, Outcome::Refused { reason: Reason::SectionChanged }, "an edit made on an older text is refused");
    assert_eq!(sent(&mut person, &mut machine, edit(Some("risks"), "None.", "")).await, Outcome::Refused { reason: Reason::SectionChanged });
    assert_eq!(sent(&mut person, &mut machine, edit(Some("problem"), " ", "A push stops when the runner drops.")).await, Outcome::Refused { reason: Reason::BadRequest }, "a required section is never saved empty");
    assert_eq!(sent(&mut person, &mut machine, edit(Some("out-of-scope"), "", "Flaky tests.")).await, Outcome::Done);
    assert_eq!(body_of(&mut person, &machine).await.section("out-of-scope").map(|one| one.text.as_str()), Some(""));

    assert_eq!(sent(&mut person, &mut machine, edit(None, "Retry a dropped CI runner", "Retry a flaky CI push")).await, Outcome::Done);
    let rows = board_of(&mut person, &machine).await.backlog.unwrap();
    assert_eq!(rows.iter().find(|row| row.item.as_str() == ITEM).unwrap().title, "Retry a dropped CI runner");
}
