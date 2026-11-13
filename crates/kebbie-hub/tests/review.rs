#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub::RateLimit;
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::MachineState;
use kebbie_hub_protocol::{
    AcceptStep, CommandResult, Item, Outcome, Payload, PushApproval, REVIEW_MEDIA_TYPE, Reason, Review, ReviewComment, ReviewComments, ReviewOpen,
    ReviewRead, Side, StepState, open_blob,
};
use serde::Deserialize;

const SOON: Duration = Duration::from_secs(5);
const EXAMPLE: &str = include_str!("../../../examples/review.json");

#[derive(Deserialize)]
struct Example {
    item: Item,
    review: Review,
}

fn example() -> Example {
    serde_json::from_str(EXAMPLE).unwrap()
}

async fn reviewing(test: &TestHub) -> (MachineState, Browser) {
    let mut person = member(test).await;
    let mut machine = connected_with_board(test, &mut person).await;
    person.knows(&machine.machine, machine.public());
    let Example { item, review } = example();
    machine.post_review(item, review).await.unwrap();
    (machine, person)
}

async fn sent(person: &mut Browser, machine: &mut MachineState, payload: Payload) -> CommandResult {
    let envelope = person.command(&machine.machine, &payload).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    person.await_result(&envelope.header.id, SOON).await.unwrap().expect("a result arrives")
}

async fn opened_review(person: &mut Browser, machine: &mut MachineState) -> Review {
    let address = example().review.address;
    let result = sent(person, machine, Payload::ReviewOpen(ReviewOpen { address, step: None })).await;
    assert_eq!(result.outcome, Outcome::Done);
    let blob = result.blob.expect("the result names the review's blob");
    assert_eq!(blob.media_type, REVIEW_MEDIA_TYPE);
    let ciphertext = person.blob(&blob.id).await.unwrap();
    assert!(!holds(&ciphertext, b"src/tasks/push.rs"), "the hub holds the review sealed");
    let review: Review = serde_json::from_slice(&open_blob(&blob, &ciphertext).unwrap()).unwrap();
    review.check().unwrap();
    review
}

#[tokio::test]
async fn a_review_is_fetched_on_demand_sealed_and_comments_reads_and_an_accept_reach_the_machine() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = reviewing(&test).await;
    let boards = person.boards().await.unwrap();
    let board = boards.into_iter().find(|board| board.machine == machine.machine).unwrap().board.unwrap();
    let offered = board.items.iter().find(|item| item.review.is_some()).unwrap();
    assert_eq!(offered.review.as_ref().map(|offer| (offer.step, offer.files)), Some((Some(1), 3)));
    assert!(!holds(&database_bytes(&test), b"wait_for_ci"), "no board or row carries the diff");

    let review = opened_review(&mut person, &mut machine).await;
    assert_eq!(review, example().review);
    let step = review.step.clone().unwrap();
    let head = step.head().unwrap().to_string();
    assert_eq!((step.commits.len(), head.as_str()), (2, review.head.as_str()), "the step's review ends at its last commit");
    let file = &step.files[1];
    let mark = |sha: Option<&str>, content: &str| Payload::ReviewRead(ReviewRead { address: review.address.clone(), path: file.path.clone(), read: true, content: content.into(), sha: sha.map(Into::into) });

    assert_eq!(sent(&mut person, &mut machine, mark(Some(&head), &file.content)).await.outcome, Outcome::Done);
    let marked = opened_review(&mut person, &mut machine).await.step.unwrap();
    assert!(marked.commits[1].files.iter().find(|one| one.path == file.path).unwrap().read, "the next review carries the mark in the commit");
    assert!(marked.files[1].read, "and on the whole step, where the file holds the same content");
    let changed = sent(&mut person, &mut machine, mark(None, &"0".repeat(40))).await;
    assert_eq!(changed.outcome, Outcome::Refused { reason: Reason::HeadMoved }, "a mark holds only for the content it names");
    let elsewhere = sent(&mut person, &mut machine, mark(Some(&step.commits[0].sha), &file.content)).await;
    assert_eq!(elsewhere.outcome, Outcome::Refused { reason: Reason::HeadMoved }, "a mark in a commit holds only where that commit changed the file");

    let comment = |line, side, text: &str| ReviewComment { path: "src/tasks/push.rs".into(), line, side, sha: Some(head.clone()), text: text.into() };
    let comments = Payload::ReviewComments(ReviewComments { address: review.address.clone(), step: Some(1), comments: vec![comment(112, Side::Old, "Share the deadline?"), comment(127, Side::New, "Keep this line.")] });
    assert_eq!(sent(&mut person, &mut machine, comments).await.text, "Sent 2 comments to retry-push@kebbie's Claude.");

    let accept = |head: &str| Payload::AcceptStep(AcceptStep { address: review.address.clone(), step: step.number, head: head.into() });
    assert_eq!(sent(&mut person, &mut machine, accept(&step.commits[0].sha)).await.outcome, Outcome::Refused { reason: Reason::HeadMoved }, "an accept names the step's last commit");
    assert_eq!(sent(&mut person, &mut machine, accept(&head)).await.outcome, Outcome::Done);
    let boards = person.boards().await.unwrap();
    let board = boards.into_iter().find(|board| board.machine == machine.machine).unwrap().board.unwrap();
    let item = board.items.iter().find(|item| item.address == review.address).unwrap();
    assert_eq!(item.steps.as_ref().unwrap().items[0].state, StepState::Accepted);
    assert_eq!(sent(&mut person, &mut machine, accept(&head)).await.outcome, Outcome::Refused { reason: Reason::AlreadySettled });

    let push = Payload::Push(PushApproval { address: review.address.clone(), head: head.clone(), card_sha: None, through_step: Some(step.number) });
    assert_eq!(sent(&mut person, &mut machine, push).await.text, format!("Pushed retry-push@kebbie through step 1 at {head}."), "a push through a step reaches the machine with its step's last commit");
}

#[tokio::test]
async fn a_review_opens_the_step_it_names_or_the_first_one_waiting() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = reviewing(&test).await;
    let address = example().review.address;
    let open = |step| Payload::ReviewOpen(ReviewOpen { address: address.clone(), step });
    let named = sent(&mut person, &mut machine, open(Some(1))).await;
    assert_eq!((named.outcome, named.text.as_str()), (Outcome::Done, "The review of step 1 of retry-push@kebbie is ready."));
    let waiting = sent(&mut person, &mut machine, open(None)).await;
    assert_eq!((waiting.text.as_str(), waiting.blob.map(|blob| blob.id)), ("The review of step 1 of retry-push@kebbie is ready.", named.blob.map(|blob| blob.id)));
    assert_eq!(sent(&mut person, &mut machine, open(Some(2))).await.outcome, Outcome::Refused { reason: Reason::AlreadySettled }, "step 2 is still being built");
}

#[tokio::test]
async fn comments_travel_in_the_text_size_class_and_the_other_review_kinds_in_the_short_one() {
    let test = start_hub(|_| {}).await;
    let (machine, mut person) = reviewing(&test).await;
    let address = example().review.address;
    let padded = |person: &Browser, payload: &Payload, chars: usize| {
        let mut envelope = person.seal_command(&machine.machine, payload, kebbie_hub::now(), None).unwrap();
        envelope.ct = "A".repeat(chars);
        envelope
    };
    let comments = Payload::ReviewComments(ReviewComments { address: address.clone(), step: None, comments: vec![ReviewComment { path: "src/ci.rs".into(), line: 1, side: Side::New, sha: None, text: "x".into() }] });
    assert_eq!(hub_code(person.send_command(&padded(&person, &comments, 5_000)).await.unwrap_err()), (400, "bad-signature".to_string()));
    assert_eq!(hub_code(person.send_command(&padded(&person, &comments, 37 * 1024)).await.unwrap_err()), (413, "too-large".to_string()));
    let read = Payload::ReviewRead(ReviewRead { address: address.clone(), path: "src/ci.rs".into(), read: true, content: "a".repeat(40), sha: Some("b".repeat(40)) });
    let open = Payload::ReviewOpen(ReviewOpen { address: address.clone(), step: Some(1) });
    let accept = Payload::AcceptStep(AcceptStep { address, step: 1, head: "a".repeat(40) });
    for payload in [read, open, accept] {
        assert_eq!(hub_code(person.send_command(&padded(&person, &payload, 5_000)).await.unwrap_err()), (413, "too-large".to_string()), "{payload:?}");
    }
    assert_eq!(rows(&test, "queue"), 0);
}

#[tokio::test]
async fn accepts_share_the_start_and_push_limit_while_reads_and_comments_still_go_through() {
    let test = start_hub(|config| config.start_push_rate = RateLimit { burst: 2, per_minute: 1 }).await;
    let (machine, mut person) = reviewing(&test).await;
    let review = example().review;
    let step = review.step.clone().unwrap();
    let accept = Payload::AcceptStep(AcceptStep { address: review.address.clone(), step: step.number, head: step.head().unwrap().into() });
    for _ in 0..2 {
        person.command(&machine.machine, &accept).await.unwrap();
    }
    assert_eq!(hub_code(person.command(&machine.machine, &accept).await.unwrap_err()), (429, "rate-limited".to_string()));
    let file = &step.files[0];
    for read in [true, false, true] {
        person.command(&machine.machine, &Payload::ReviewRead(ReviewRead { address: review.address.clone(), path: file.path.clone(), read, content: file.content.clone(), sha: None })).await.unwrap();
    }
    assert_eq!(rows(&test, "queue"), 5);
}

#[tokio::test]
async fn opening_an_unchanged_review_again_reuses_its_blob_and_a_changed_one_replaces_it() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = reviewing(&test).await;
    let address = example().review.address;
    let open = || Payload::ReviewOpen(ReviewOpen { address: address.clone(), step: None });
    let first = sent(&mut person, &mut machine, open()).await.blob.unwrap();
    let again = sent(&mut person, &mut machine, open()).await.blob.unwrap();
    assert_eq!(again.id, first.id, "the same review is the same blob");
    assert_eq!(rows(&test, "blobs"), 1);

    let file = example().review.step.unwrap().files[0].clone();
    sent(&mut person, &mut machine, Payload::ReviewRead(ReviewRead { address: address.clone(), path: file.path, read: true, content: file.content, sha: None })).await;
    let changed = sent(&mut person, &mut machine, open()).await.blob.unwrap();
    assert_ne!(changed.id, first.id);
    assert_eq!(rows(&test, "blobs"), 1, "the machine deletes the review it replaced, so reviews never fill its quota");
    assert_eq!(status_of(person.blob(&first.id).await), 404);
    assert!(person.blob(&changed.id).await.is_ok());
}

#[tokio::test]
async fn a_machine_deletes_only_its_own_blobs() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = reviewing(&test).await;
    let address = example().review.address;
    let blob = sent(&mut person, &mut machine, Payload::ReviewOpen(ReviewOpen { address, step: None })).await.blob.unwrap();
    let mut stranger = member(&test).await;
    let theirs = connected_with_board(&test, &mut stranger).await;
    theirs.delete_blob(&blob.id).await.unwrap();
    assert!(person.blob(&blob.id).await.is_ok(), "another machine's delete leaves the blob");
    let anonymous = kebbie_hub_fakes::HubClient::new(&test.url, None);
    assert_eq!(status_of(anonymous.delete(&format!("/v1/m/blobs/{}", blob.id)).await), 401);
    machine.delete_blob(&blob.id).await.unwrap();
    assert_eq!(rows(&test, "blobs"), 0);
}
