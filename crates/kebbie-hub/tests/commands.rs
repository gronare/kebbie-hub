#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub::RateLimit;
use kebbie_hub_api::{
    CommandQueued, CommandUpload, DeviceUpload, EVENT_RESULT, ResultReady, ResultStored, ResultUpload,
};
use kebbie_hub_fakes::HubError;
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::{Handling, MachineState, Posted};
use kebbie_hub_protocol::{
    AccountKey, Answer, CommandResult, Dropped, Envelope, Header, Id, Kind, Outcome, Payload,
    DeviceCert, DeviceKey, DeviceKind, Idea, ItemAnswer, ItemKey, Prefix, PromptAnswer, PushApproval, Reason, SecretKeys, StartItem, StartTask, seal, seal_command,
};
use serde_json::Value;

const SOON: Duration = Duration::from_secs(5);
const ADDRESS: &str = "refunds@shop";
const QUESTION: &str = "1761069540-4411-0-question";
const HEAD: &str = "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a";
const MOVED_HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const PAST_THE_WINDOW: u64 = 8 * 86_400;
const PAST_THE_HISTORY: u64 = 91 * 86_400;

async fn commanding(test: &TestHub) -> (MachineState, Browser) {
    let mut person = member(test).await;
    let mut machine = connected_with_board(test, &mut person).await;
    person.knows(&machine.machine, machine.public());
    machine.post_board(board(MARKER)).await.unwrap();
    (machine, person)
}

fn answer(choice: Option<u8>, comment: &str) -> Payload {
    Payload::Answer(Answer {
        address: ADDRESS.to_string(),
        question: QUESTION.to_string(),
        choice,
        comment: comment.to_string(),
    })
}

fn push(head: &str) -> Payload {
    Payload::Push(PushApproval {
        address: ADDRESS.to_string(),
        head: head.to_string(),
        card_sha: None,
        through_step: None,
    })
}

fn start(name: &str, prompt: &str) -> Payload {
    Payload::Start(StartTask {
        repo: "shop".to_string(),
        name: name.to_string(),
        prompt: prompt.to_string(),
    })
}

fn idea(text: &str) -> Payload {
    Payload::Idea(Idea { text: text.to_string(), repo: Some("shop".to_string()) })
}

fn item_answer(answer: &str) -> Payload {
    Payload::ItemAnswer(ItemAnswer { item: ItemKey::vault("Projects/x.md").unwrap(), index: 0, question: "Which?".to_string(), answer: answer.to_string() })
}

fn start_item(item: &str) -> Payload {
    Payload::StartItem(StartItem { item: ItemKey::parse(item).unwrap(), anyway: false })
}

fn refusal_of<T: std::fmt::Debug>(result: Result<T, HubError>) -> (u16, String) {
    match result {
        Err(error) => hub_code(error),
        other => panic!("the hub accepted it: {other:?}"),
    }
}

fn machine_refusal<T: std::fmt::Debug>(result: Result<T, kebbie_hub_fakes::Error>) -> (u16, String) {
    match result {
        Err(kebbie_hub_fakes::Error::Hub(error)) => hub_code(error),
        other => panic!("the hub accepted it: {other:?}"),
    }
}

async fn result_of(person: &mut Browser, envelope: &Envelope) -> CommandResult {
    person
        .await_result(&envelope.header.id, SOON)
        .await
        .unwrap()
        .expect("a result arrives")
}

fn refused(reason: Reason) -> Outcome {
    Outcome::Refused { reason }
}

fn account_of(person: &Browser) -> AccountKey {
    person.account_key().expect("the account is unlocked").clone()
}

fn done(command: &Id) -> CommandResult {
    CommandResult {
        command: command.clone(),
        device: Id::ulid(Prefix::Device, 0, [0; 10]).unwrap(),
        outcome: Outcome::Done,
        text: "done".to_string(),
        blob: None,
    }
}

#[tokio::test]
async fn an_answer_from_a_device_is_applied_and_its_result_comes_back() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let sent = person
        .command(&machine.machine, &answer(Some(1), ""))
        .await
        .unwrap();
    assert_eq!(Some(&sent.header.from), person.device_cert().map(|cert| &cert.device));

    let handled = machine.apply_once(0, None).await.unwrap();
    assert_eq!(handled.len(), 1);
    assert_eq!(handled[0].command, sent.header.id);
    assert_eq!(handled[0].kind, Kind::Answer);
    match &handled[0].handling {
        Handling::Applied { did, result } => {
            assert!(did.contains(QUESTION) && did.contains("choice 1"), "{did}");
            assert_eq!(result.outcome, Outcome::Done);
        }
        other => panic!("not applied: {other:?}"),
    }
    let result = result_of(&mut person, &sent).await;
    assert_eq!(result.command, sent.header.id);
    assert_eq!(result.outcome, Outcome::Done);
    assert_eq!(machine.applied.len(), 1);
}

#[tokio::test]
async fn a_prompt_answer_is_relayed_like_any_command_and_lives_ten_minutes_at_most() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let chosen = Payload::PromptAnswer(PromptAnswer {
        address: ADDRESS.to_string(),
        prompt: "3f2a9c1d".to_string(),
        key: "y".to_string(),
    });
    let sent = person.command(&machine.machine, &chosen).await.unwrap();
    assert_eq!(sent.header.exp - sent.header.at, 600);
    let handled = machine.apply_once(0, None).await.unwrap();
    assert_eq!(handled[0].kind, Kind::PromptAnswer);
    assert!(
        matches!(&handled[0].handling, Handling::Applied { did, .. } if did.contains("prompt 3f2a9c1d")),
        "{:?}",
        handled[0].handling
    );
    assert_eq!(result_of(&mut person, &sent).await.outcome, Outcome::Done);
    assert!(
        person
            .seal_command(&machine.machine, &chosen, kebbie_hub::now(), Some(601))
            .is_err(),
        "a screen prompt is momentary"
    );
}

#[tokio::test]
async fn a_command_another_account_signed_is_dropped_by_the_machine_and_nothing_is_applied() {
    let test = start_hub(|_| {}).await;
    let (mut machine, _person) = commanding(&test).await;
    let mut stranger = member(&test).await;
    stranger.knows(&machine.machine, machine.public());
    let theirs = stranger
        .seal_command(&machine.machine, &answer(Some(0), ""), kebbie_hub::now(), None)
        .unwrap();
    assert_eq!(
        refusal_of(stranger.send_command(&theirs).await),
        (404, "unknown-machine".to_string()),
        "the hub queues nothing for a machine on another account"
    );
    test.hub.enqueue(&machine.machine, &theirs).unwrap();
    let handled = machine.apply_once(0, None).await.unwrap();
    assert_eq!(handled.len(), 1);
    assert_eq!(handled[0].handling, Handling::Dropped(Dropped::WrongAccount));
    assert_eq!(handled[0].posted, None);
    assert!(machine.applied.is_empty());
    assert_eq!(rows(&test, "results"), 0);
}

#[tokio::test]
async fn a_kind_the_machine_does_not_take_is_refused_and_the_account_hears_why() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    machine.accepts = vec![Kind::Answer, Kind::PromptAnswer];
    let pushed = person.command(&machine.machine, &push(HEAD)).await.unwrap();
    let answered = person
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    let handled = machine.apply_once(0, None).await.unwrap();
    assert!(
        matches!(&handled[0].handling, Handling::Refused(result) if result.outcome == refused(Reason::NotAllowed)),
        "{:?}",
        handled[0].handling
    );
    assert!(matches!(&handled[1].handling, Handling::Applied { .. }));
    assert_eq!(result_of(&mut person, &pushed).await.outcome, refused(Reason::NotAllowed));
    assert_eq!(result_of(&mut person, &answered).await.outcome, Outcome::Done);
}

#[tokio::test]
async fn a_command_delivered_twice_gets_the_stored_result_and_a_resend_queues_nothing() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let envelope = person
        .seal_command(&machine.machine, &answer(Some(1), ""), kebbie_hub::now(), None)
        .unwrap();
    let first = person.send_command(&envelope).await.unwrap();
    let again = person.send_command(&envelope).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(rows(&test, "queue"), 1);
    assert_eq!(rows(&test, "commands"), 1);

    let applied = machine.apply_once(0, None).await.unwrap();
    let Handling::Applied { result, .. } = &applied[0].handling else {
        panic!("not applied: {:?}", applied[0].handling);
    };
    machine.inbox_seq = 0;
    let replayed = machine.apply_once(0, None).await.unwrap();
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].handling, Handling::Replayed(result.clone()));

    assert_eq!(rows(&test, "results"), 1);
    assert_eq!(&result_of(&mut person, &envelope).await, result);
    assert_eq!(person.results(0, 0).await.unwrap().results.len(), 1);
}

#[tokio::test]
async fn a_command_for_a_machine_off_the_account_is_refused_and_not_queued() {
    let test = start_hub(|_| {}).await;
    let (_machine, mut person) = commanding(&test).await;
    let stranger = joined_machine(&test).await;
    person.knows(&stranger.machine, stranger.public());
    let elsewhere = person.command(&stranger.machine, &answer(Some(0), "")).await;
    assert_eq!(refusal_of(elsewhere), (404, "unknown-machine".to_string()));
    assert_eq!(rows(&test, "queue"), 0);
    assert_eq!(rows(&test, "commands"), 0);
}

#[tokio::test]
async fn a_forged_or_misattributed_command_is_refused_and_not_queued() {
    let test = start_hub(|_| {}).await;
    let (machine, mut person) = commanding(&test).await;
    let account = account_of(&person);
    let impostor = SecretKeys::generate();
    let forged = seal(
        Header::command(
            Id::new(Prefix::Command).unwrap(),
            Kind::Answer,
            person.device_cert().unwrap().clone(),
            machine.machine.clone(),
            kebbie_hub::now(),
        ),
        &answer(Some(0), ""),
        &impostor,
        &[(machine.machine.clone(), machine.public()), account.recipient()],
    )
    .unwrap();
    assert_eq!(
        refusal_of(person.send_command(&forged).await),
        (400, "bad-signature".to_string())
    );

    let genuine = person
        .seal_command(&machine.machine, &answer(Some(0), ""), kebbie_hub::now(), None)
        .unwrap();
    let mut other = member(&test).await;
    assert_eq!(
        refusal_of(other.send_command(&genuine).await),
        (400, "wrong-account".to_string())
    );

    let mut tampered = genuine.clone();
    tampered.header.exp -= 1;
    assert_eq!(
        refusal_of(person.send_command(&tampered).await),
        (400, "bad-signature".to_string())
    );

    let board = machine
        .seal_board(board(MARKER), 9, kebbie_hub::now())
        .unwrap();
    assert_eq!(
        refusal_of(person.send_command(&board).await),
        (400, "not-a-command".to_string())
    );

    let mut sprayed = genuine.clone();
    sprayed
        .keys
        .insert(Id::new(Prefix::Device).unwrap(), "AAAA".to_string());
    assert_eq!(
        refusal_of(person.send_command(&sprayed).await),
        (400, "bad-recipients".to_string())
    );

    let cross_site = reqwest::Client::new()
        .post(format!("{}/v1/a/commands", test.url))
        .header("cookie", person.cookie.clone().unwrap())
        .header("origin", "https://elsewhere.example")
        .json(&CommandUpload { envelope: genuine })
        .send()
        .await
        .unwrap();
    assert_eq!(refusal(cross_site).await, (403, "cross-origin".to_string()));
    assert_eq!(rows(&test, "queue"), 0);
    assert_eq!(rows(&test, "commands"), 0);
}

#[tokio::test]
async fn an_expired_command_is_refused_by_the_hub_or_by_the_machine() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let stale = person
        .seal_command(&machine.machine, &answer(Some(0), ""), kebbie_hub::now() - 700, Some(600))
        .unwrap();
    assert_eq!(
        refusal_of(person.send_command(&stale).await),
        (400, "expired".to_string())
    );
    assert_eq!(rows(&test, "queue"), 0);

    let short = person
        .seal_command(&machine.machine, &answer(Some(0), ""), kebbie_hub::now(), Some(1))
        .unwrap();
    person.send_command(&short).await.unwrap();
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let handled = machine.apply_once(0, None).await.unwrap();
    assert_eq!(
        handled[0].handling.result().unwrap().outcome,
        refused(Reason::Expired)
    );
    assert_eq!(
        result_of(&mut person, &short).await.outcome,
        refused(Reason::Expired)
    );
}

#[tokio::test]
async fn a_push_of_a_head_that_moved_since_the_account_saw_it_is_still_pushed() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let mut moved = board(MARKER);
    moved.items[0].head = Some(MOVED_HEAD.to_string());
    machine.post_board(moved).await.unwrap();

    let seen = person.command(&machine.machine, &push(HEAD)).await.unwrap();
    let handled = machine.apply_once(0, None).await.unwrap();
    match &handled[0].handling {
        Handling::Applied { did, .. } => assert!(did.contains(MOVED_HEAD), "{did}"),
        other => panic!("not applied: {other:?}"),
    }
    assert_eq!(result_of(&mut person, &seen).await.outcome, Outcome::Done);
}

#[tokio::test]
async fn starts_past_their_rate_limit_get_429_while_answers_still_go_through() {
    let test = start_hub(|config| {
        config.start_push_rate = RateLimit {
            burst: 2,
            per_minute: 1,
        }
    })
    .await;
    let (machine, mut person) = commanding(&test).await;
    for name in ["first-task", "second-task"] {
        person
            .command(&machine.machine, &start(name, "do the thing"))
            .await
            .unwrap();
    }
    let third = person
        .command(&machine.machine, &start("third-task", "do the thing"))
        .await;
    assert_eq!(refusal_of(third), (429, "rate-limited".to_string()));
    person
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    assert_eq!(rows(&test, "queue"), 3);
}

#[tokio::test]
async fn starts_from_the_backlog_share_the_start_limit_while_ideas_and_item_answers_still_go_through() {
    let test = start_hub(|config| {
        config.start_push_rate = RateLimit {
            burst: 2,
            per_minute: 1,
        }
    })
    .await;
    let (machine, mut person) = commanding(&test).await;
    for item in ["kept:one", "kept:two"] {
        person.command(&machine.machine, &start_item(item)).await.unwrap();
    }
    assert_eq!(refusal_of(person.command(&machine.machine, &start_item("kept:three")).await), (429, "rate-limited".to_string()));
    for _ in 0..3 {
        person.command(&machine.machine, &idea("Mail a weekly summary")).await.unwrap();
    }
    person.command(&machine.machine, &item_answer("Half up")).await.unwrap();
    assert_eq!(rows(&test, "queue"), 6);
}

#[tokio::test]
async fn a_result_reaches_only_the_account_that_sent_the_command() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let account = account_of(&person);
    let mut bystander = member(&test).await;
    let elsewhere = account_of(&bystander);

    let waiting = person
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    let command = waiting.header.id.clone();
    let early = machine.post_result(&account, &done(&command)).await;
    assert_eq!(machine_refusal(early), (409, "not-delivered".to_string()));

    machine.poll_once(0).await.unwrap();
    let misdirected = machine.post_result(&elsewhere, &done(&command)).await;
    assert_eq!(machine_refusal(misdirected), (404, "unknown-command".to_string()));
    let unknown = machine
        .post_result(&account, &done(&Id::new(Prefix::Command).unwrap()))
        .await;
    assert_eq!(machine_refusal(unknown), (404, "unknown-command".to_string()));

    let to_both = seal(
        Header::new(
            command.clone(),
            Kind::Result,
            machine.machine.clone(),
            machine.machine.clone(),
            kebbie_hub::now(),
        ),
        &Payload::Result(done(&command)),
        &machine.secret(),
        &[account.recipient(), elsewhere.recipient()],
    )
    .unwrap();
    let doubled = machine
        .client()
        .post::<_, Value>("/v1/m/results", &ResultUpload { envelope: to_both })
        .await;
    assert_eq!(status_of(doubled), 400);

    let impostor_result = {
        let mut envelope = machine.seal_result(&account, &done(&command)).unwrap();
        envelope.sig = person
            .seal_command(&machine.machine, &answer(Some(0), ""), kebbie_hub::now(), None)
            .unwrap()
            .sig;
        envelope
    };
    let forged = machine
        .client()
        .post::<_, Value>("/v1/m/results", &ResultUpload { envelope: impostor_result })
        .await;
    assert_eq!(status_of(forged), 400);

    machine.post_result(&account, &done(&command)).await.unwrap();
    assert_eq!(result_of(&mut person, &waiting).await, done(&command));
    assert!(bystander.results(0, 0).await.unwrap().results.is_empty());
}

#[tokio::test]
async fn the_database_never_holds_command_text() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let answered = person
        .command(&machine.machine, &answer(None, &format!("{MARKER} comment")))
        .await
        .unwrap();
    let started = person
        .command(&machine.machine, &start("zebra-task", &format!("{MARKER} prompt")))
        .await
        .unwrap();
    let pushed = person.command(&machine.machine, &push(HEAD)).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    for sent in [&answered, &started, &pushed] {
        assert_eq!(result_of(&mut person, sent).await.outcome, Outcome::Done);
    }

    let stored = database_bytes(&test);
    assert!(
        holds(&stored, answered.header.id.as_str().as_bytes()),
        "the scan did not read the commands"
    );
    for leak in [
        format!("{MARKER} comment"),
        format!("{MARKER} prompt"),
        "zebra-task".to_string(),
        QUESTION.to_string(),
        ADDRESS.to_string(),
        HEAD.to_string(),
        "Answered".to_string(),
        "Started".to_string(),
    ] {
        assert!(!holds(&stored, leak.as_bytes()), "the database holds {leak:?}");
    }
}

#[tokio::test]
async fn the_account_feed_announces_a_result_by_command_id_only() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let mut events = EventStream::of_account(&person).await;

    let sent = person
        .command(&machine.machine, &answer(None, &format!("{MARKER} comment")))
        .await
        .unwrap();
    machine.apply_once(0, None).await.unwrap();
    let data = events.until(EVENT_RESULT, SOON).await;
    assert_eq!(data.as_object().unwrap().len(), 2, "{data}");
    assert_eq!(
        serde_json::from_value::<ResultReady>(data.clone()).unwrap(),
        ResultReady {
            machine: machine.machine.clone(),
            command: sent.header.id.clone(),
        }
    );
    assert!(!data.to_string().contains(MARKER));

    async fn quiet_about_results(mut stream: EventStream) {
        loop {
            match stream.next(Duration::from_millis(300)).await {
                Next::Event(name, _) if name == EVENT_RESULT => panic!("announced again"),
                Next::Event(..) => continue,
                Next::Quiet => break,
                Next::Ended => panic!("the feed ended"),
            }
        }
    }
    quiet_about_results(EventStream::of_account(&person).await).await;
    let mut replayed = EventStream::of_account_after(&person, 0).await;
    assert_eq!(
        replayed.until(EVENT_RESULT, SOON).await["command"],
        sent.header.id.as_str()
    );
    let seq = person.results(0, 0).await.unwrap().results[0].seq;
    quiet_about_results(EventStream::of_account_after(&person, seq).await).await;
}

#[tokio::test]
async fn a_last_event_id_past_what_sqlite_holds_is_refused_with_400() {
    let test = start_hub(|_| {}).await;
    let person = member(&test).await;
    for id in ["9223372036854775808", "soon"] {
        let response = EventStream::account_request(&person)
            .header("last-event-id", id)
            .send()
            .await
            .unwrap();
        assert_eq!(refusal(response).await, (400, "bad-last-event-id".to_string()));
    }
}

#[tokio::test]
async fn a_machine_back_after_the_command_window_drains_its_queue() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    person
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    test.hub
        .store()
        .sweep(kebbie_hub::now() + PAST_THE_WINDOW)
        .unwrap();
    assert_eq!(rows(&test, "queue"), 0);
    assert_eq!(rows(&test, "commands"), 1, "the command stays in the device's history");
    assert!(machine.apply_once(0, None).await.unwrap().is_empty());

    let fresh = person
        .command(&machine.machine, &answer(Some(1), ""))
        .await
        .unwrap();
    machine.apply_once(0, None).await.unwrap();
    assert_eq!(result_of(&mut person, &fresh).await.outcome, Outcome::Done);
}

#[tokio::test]
async fn a_result_for_a_command_the_hub_dropped_is_dropped_and_the_inbox_moves_on() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    person
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    machine.poll_once(0).await.unwrap();
    machine.inbox_seq = 0;
    test.hub
        .store()
        .connection()
        .execute("DELETE FROM commands", [])
        .unwrap();

    let handled = machine.apply_once(0, None).await.unwrap();
    assert_eq!(handled.len(), 1);
    assert_eq!(handled[0].posted, Some(Posted::CommandGone));
    assert!(machine.apply_once(0, None).await.unwrap().is_empty());
    assert_eq!(rows(&test, "queue"), 0);
}

async fn send(person: &mut Browser, machine: &Id) -> Result<CommandQueued, HubError> {
    let envelope = person
        .seal_command(machine, &answer(Some(0), ""), kebbie_hub::now(), None)
        .unwrap();
    person.send_command(&envelope).await
}

#[tokio::test]
async fn an_account_holds_only_so_many_undelivered_commands_per_machine() {
    let test = start_hub(|config| config.max_undelivered_per_account = 2).await;
    let (mut machine, mut person) = commanding(&test).await;
    let target = machine.machine.clone();
    send(&mut person, &target).await.unwrap();
    send(&mut person, &target).await.unwrap();
    assert_eq!(
        refusal_of(send(&mut person, &target).await),
        (429, "queue-full".to_string())
    );
    machine.poll_once(0).await.unwrap();
    machine.poll_once(0).await.unwrap();
    send(&mut person, &target).await.unwrap();
}

#[tokio::test]
async fn a_machine_holds_only_so_many_queued_messages() {
    let test = start_hub(|config| config.max_queued = 2).await;
    let (machine, mut person) = commanding(&test).await;
    let target = machine.machine.clone();
    send(&mut person, &target).await.unwrap();
    send(&mut person, &target).await.unwrap();
    assert_eq!(
        refusal_of(send(&mut person, &target).await),
        (429, "queue-full".to_string())
    );
}

#[tokio::test]
async fn result_positions_rise_and_list_only_the_accounts_own() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let (mut theirs, mut other) = commanding(&test).await;
    let first = person.command(&machine.machine, &answer(Some(0), "")).await.unwrap();
    let elsewhere = other.command(&theirs.machine, &answer(Some(0), "")).await.unwrap();
    let second = person.command(&machine.machine, &answer(Some(1), "")).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    theirs.apply_once(0, None).await.unwrap();
    let listing = person.results(0, 0).await.unwrap();
    let commands: Vec<&Id> = listing.results.iter().map(|entry| &entry.command).collect();
    assert_eq!(commands, [&first.header.id, &second.header.id]);
    assert!(listing.results[0].seq < listing.results[1].seq);
    assert_eq!(listing.cursor, listing.results[1].seq);
    assert!(person.results(listing.cursor, 0).await.unwrap().results.is_empty());
    let theirs_listed = other.results(0, 0).await.unwrap();
    assert_eq!(theirs_listed.results.len(), 1);
    assert_eq!(theirs_listed.results[0].command, elsewhere.header.id);
}

#[tokio::test]
async fn a_second_result_for_a_command_must_be_the_same_envelope() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let account = account_of(&person);
    let sent = person
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    machine.poll_once(0).await.unwrap();
    let command = sent.header.id.clone();
    let envelope = machine.seal_result(&account, &done(&command)).unwrap();
    let post = |envelope: Envelope| {
        let client = machine.client();
        async move {
            client
                .post::<_, ResultStored>("/v1/m/results", &ResultUpload { envelope })
                .await
        }
    };
    let first = post(envelope.clone()).await.unwrap();
    assert_eq!(post(envelope).await.unwrap(), first);
    let other = machine.seal_result(&account, &done(&command)).unwrap();
    assert_eq!(
        post(other).await.unwrap_err(),
        HubError::Status {
            status: 409,
            code: "result-differs".to_string()
        }
    );
    assert_eq!(rows(&test, "results"), 1);
}

#[tokio::test]
async fn a_resend_with_a_changed_body_is_checked_like_a_new_command() {
    let test = start_hub(|_| {}).await;
    let (machine, mut person) = commanding(&test).await;
    let genuine = person
        .seal_command(&machine.machine, &answer(Some(0), ""), kebbie_hub::now(), None)
        .unwrap();
    person.send_command(&genuine).await.unwrap();
    let mut changed = genuine;
    changed.header.exp -= 1;
    assert_eq!(
        refusal_of(person.send_command(&changed).await),
        (400, "bad-signature".to_string())
    );
    assert_eq!(rows(&test, "queue"), 1);
}

#[tokio::test]
async fn each_kind_has_its_own_ciphertext_cap() {
    let test = start_hub(|_| {}).await;
    let (machine, mut person) = commanding(&test).await;
    let padded = |person: &Browser, payload: &Payload, chars: usize| {
        let mut envelope = person
            .seal_command(&machine.machine, payload, kebbie_hub::now(), None)
            .unwrap();
        envelope.ct = "A".repeat(chars);
        envelope
    };
    let pushed = padded(&person, &push(HEAD), 5_000);
    assert_eq!(refusal_of(person.send_command(&pushed).await), (413, "too-large".to_string()));
    let answered = padded(&person, &answer(Some(0), ""), 5_000);
    assert_eq!(refusal_of(person.send_command(&answered).await), (400, "bad-signature".to_string()));
    let started = padded(&person, &start("big-task", "x"), 37 * 1024);
    assert_eq!(refusal_of(person.send_command(&started).await), (413, "too-large".to_string()));
    for payload in [idea("x"), item_answer("x")] {
        let written = padded(&person, &payload, 5_000);
        assert_eq!(refusal_of(person.send_command(&written).await), (400, "bad-signature".to_string()), "{payload:?}");
        let long = padded(&person, &payload, 37 * 1024);
        assert_eq!(refusal_of(person.send_command(&long).await), (413, "too-large".to_string()), "{payload:?}");
    }
    let begun = padded(&person, &start_item("kept:one"), 5_000);
    assert_eq!(refusal_of(person.send_command(&begun).await), (413, "too-large".to_string()));
    assert_eq!(rows(&test, "queue"), 0);
}

#[tokio::test]
async fn a_command_id_once_used_cannot_carry_another_command() {
    let test = start_hub(|_| {}).await;
    let (machine, mut person) = commanding(&test).await;
    let account = account_of(&person);
    let taken = person
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    let reused = seal_command(
        Header::command(
            taken.header.id.clone(),
            Kind::Answer,
            person.device_cert().unwrap().clone(),
            machine.machine.clone(),
            kebbie_hub::now(),
        ),
        &answer(Some(1), ""),
        person.device_key(),
        &machine.public(),
        &account,
    )
    .unwrap();
    assert_eq!(
        refusal_of(person.send_command(&reused).await),
        (409, "command-id-taken".to_string())
    );
    assert_eq!(rows(&test, "queue"), 1);
}

#[tokio::test]
async fn the_results_endpoint_takes_only_results_about_the_posting_machine() {
    let test = start_hub(|_| {}).await;
    let (machine, person) = commanding(&test).await;
    let account = account_of(&person);
    let stranger = joined_machine(&test).await;
    let post = |envelope: Envelope| {
        let client = machine.client();
        async move {
            client
                .post::<_, Value>("/v1/m/results", &ResultUpload { envelope })
                .await
        }
    };
    let board = machine
        .seal_board(board(MARKER), 9, kebbie_hub::now())
        .unwrap();
    assert_eq!(
        post(board).await.unwrap_err(),
        HubError::Status {
            status: 400,
            code: "not-a-result".to_string()
        }
    );
    let command = Id::new(Prefix::Command).unwrap();
    let elsewhere = seal(
        Header::new(
            command.clone(),
            Kind::Result,
            stranger.machine.clone(),
            stranger.machine.clone(),
            kebbie_hub::now(),
        ),
        &Payload::Result(done(&command)),
        &machine.secret(),
        &[account.recipient()],
    )
    .unwrap();
    assert_eq!(
        post(elsewhere).await.unwrap_err(),
        HubError::Status {
            status: 400,
            code: "wrong-machine".to_string()
        }
    );
}

#[tokio::test]
async fn the_sweep_drops_the_queue_after_the_window_and_the_history_after_ninety_days() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let sent = person
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    machine.apply_once(0, None).await.unwrap();
    result_of(&mut person, &sent).await;
    assert_eq!((rows(&test, "commands"), rows(&test, "results")), (1, 1));
    test.hub
        .store()
        .sweep(kebbie_hub::now() + PAST_THE_WINDOW)
        .unwrap();
    assert_eq!((rows(&test, "queue"), rows(&test, "commands"), rows(&test, "results")), (0, 1, 1));
    test.hub
        .store()
        .sweep(kebbie_hub::now() + PAST_THE_HISTORY)
        .unwrap();
    for table in ["queue", "commands", "results"] {
        assert_eq!(rows(&test, table), 0, "{table}");
    }
}

#[tokio::test]
async fn the_device_routes_answer_404() {
    let test = start_hub(|_| {}).await;
    let (machine, person) = commanding(&test).await;
    let client = reqwest::Client::new();
    let cookie = person.cookie.clone().unwrap();
    let removed = [
        ("POST", "/v1/d/commands"),
        ("GET", "/v1/d/results"),
        ("PUT", "/v1/d/name"),
        ("POST", "/v1/d/pairings"),
        ("GET", "/v1/d/pairings/p_x"),
        ("POST", "/v1/d/pairings/p_x/reveal"),
        ("GET", "/v1/d/events"),
        ("PUT", "/v1/d/push"),
        ("DELETE", "/v1/d/push"),
        ("GET", "/v1/push/vapid"),
        ("DELETE", "/v1/a/devices/dev_x"),
        ("GET", "/v1/a/device-list"),
        ("PUT", "/v1/a/device-list"),
    ];
    for (method, path) in removed {
        let response = client
            .request(method.parse().unwrap(), format!("{}{path}", test.url))
            .header("cookie", &cookie)
            .header("origin", kebbie_hub::DEV_ORIGIN)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 404, "{method} {path}");
    }
    let on_the_machine = machine.client();
    assert_eq!(status_of(on_the_machine.get::<Value>("/v1/m/device-list", &[]).await), 404);
    assert_eq!(status_of(on_the_machine.delete("/v1/m/devices/dev_x").await), 404);
    assert_eq!(status_of(on_the_machine.post::<_, Value>("/v1/m/pairings", &()).await), 404);
}

fn certify(person: &Browser, device: &DeviceKey, name: &str) -> DeviceCert {
    let (signing, encryption) = person.account_secret().unwrap();
    DeviceCert::issue(
        &SecretKeys::from_bytes(signing, encryption),
        &account_of(person).account,
        Id::new(Prefix::Device).unwrap(),
        name,
        DeviceKind::Web,
        device.public(),
        kebbie_hub::now(),
    )
    .unwrap()
}

#[tokio::test]
async fn a_command_from_a_device_the_hub_never_registered_is_refused() {
    let test = start_hub(|_| {}).await;
    let (machine, mut person) = commanding(&test).await;
    let unknown = DeviceKey::generate();
    let cert = certify(&person, &unknown, "Unregistered");
    person.use_device(unknown, cert);
    let envelope = person
        .seal_command(&machine.machine, &answer(Some(0), ""), kebbie_hub::now(), None)
        .unwrap();
    assert_eq!(
        refusal_of(person.post_command(&envelope).await),
        (403, "unknown-device".to_string())
    );
    assert_eq!(rows(&test, "commands"), 0);
    person.send_command(&envelope).await.unwrap();
    assert_eq!(rows(&test, "commands"), 1, "the same command goes once the device is registered");
}

#[tokio::test]
async fn the_hub_registers_a_device_only_under_a_certificate_its_account_signed() {
    let test = start_hub(|_| {}).await;
    let (_machine, mut person) = commanding(&test).await;
    let added = person.register_device().await.unwrap();
    let cert = person.device_cert().unwrap().clone();
    assert_eq!((added.device.clone(), added.name.as_str(), added.fingerprint.clone()), (cert.device.clone(), "Test browser", cert.fingerprint()));
    assert_eq!(added.last_seen_at, None);

    let other = member(&test).await;
    let (signing, encryption) = other.account_secret().unwrap();
    let theirs = SecretKeys::from_bytes(signing, encryption);
    let forged = DeviceCert::issue(&theirs, &cert.account, cert.device.clone(), "Mine now", DeviceKind::Web, cert.recipient().1, 1).unwrap();
    let foreign = DeviceCert::issue(&theirs, &account_of(&other).account, Id::new(Prefix::Device).unwrap(), "Theirs", DeviceKind::Web, cert.recipient().1, 1).unwrap();
    let rekeyed = DeviceCert { signing: DeviceKey::generate().public().signing, ..certify(&person, &DeviceKey::generate(), "Rekeyed") };
    let mut long = certify(&person, &DeviceKey::generate(), "x");
    long.name = "x".repeat(65);
    let (other_signing, other_encryption) = person.account_secret().unwrap();
    let taken_id = DeviceCert::issue(&SecretKeys::from_bytes(other_signing, other_encryption), &cert.account, cert.device.clone(), "Same id", DeviceKind::Web, DeviceKey::generate().public(), 1).unwrap();
    for (cert, expected) in [
        (forged, (400, "bad-certificate")),
        (foreign, (400, "wrong-account")),
        (rekeyed, (400, "bad-certificate")),
        (long, (400, "bad-name")),
        (taken_id, (409, "device-taken")),
    ] {
        let refused = person.post::<_, Value>("/v1/a/devices", &DeviceUpload { cert }).await;
        assert_eq!(refusal_of(refused), (expected.0, expected.1.to_string()));
    }

    let renamed = person.rename_device("Work phone").await.unwrap();
    assert_eq!((renamed.device, renamed.name.as_str(), renamed.created_at), (added.device, "Work phone", added.created_at));
    let listed = person.devices().await.unwrap().devices;
    assert_eq!(listed.iter().map(|device| device.name.as_str()).collect::<Vec<_>>(), ["Work phone"]);
    assert!(other.account_key().is_some());
    let mut other = other;
    assert!(other.devices().await.unwrap().devices.is_empty(), "devices are listed per account");
}

#[tokio::test]
async fn a_devices_history_lists_its_commands_newest_first_with_their_results() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut person) = commanding(&test).await;
    let first = person.command(&machine.machine, &answer(Some(0), "")).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    result_of(&mut person, &first).await;
    let second = person.command(&machine.machine, &push(HEAD)).await.unwrap();
    let device = person.device_cert().unwrap().device.clone();
    let seen = person.devices().await.unwrap().devices[0].last_seen_at;
    assert!(seen.is_some_and(|at| at + 5 >= kebbie_hub::now()), "sending touches last seen: {seen:?}");

    let history = person.device_commands(&device, None).await.unwrap();
    assert_eq!(history.commands.iter().map(|entry| &entry.command).collect::<Vec<_>>(), [&second.header.id, &first.header.id]);
    assert_eq!(history.cursor, None);
    let [newest, oldest] = &history.commands[..] else { panic!("two commands") };
    assert_eq!((newest.kind.as_str(), newest.result.is_none()), ("push", true));
    assert!(oldest.delivered_at.is_some());
    let account = account_of(&person).account;
    assert_eq!(oldest.envelope.keys.keys().collect::<Vec<_>>(), [&account], "the hub hands out only the account's copy");
    let (sender, payload) = person.open_command(&oldest.envelope).unwrap();
    assert_eq!((sender.id(), payload), (&device, answer(Some(0), "")));
    let result = person.open_result(&kebbie_hub_api::ResultEntry {
        seq: 0,
        machine: oldest.machine.clone(),
        command: oldest.command.clone(),
        stored_at: oldest.result_at.unwrap(),
        envelope: oldest.result.clone().unwrap(),
    });
    assert_eq!(result.map(|result| (result.outcome, result.device)), Ok((Outcome::Done, device.clone())));

    let other = member(&test).await;
    let mut other = other;
    assert_eq!(refusal_of(other.device_commands(&device, None).await), (404, "unknown-device".to_string()));

    test.hub.store().sweep(kebbie_hub::now() + PAST_THE_WINDOW).unwrap();
    assert_eq!(person.device_commands(&device, None).await.unwrap().commands.len(), 2);
    test.hub.store().sweep(kebbie_hub::now() + PAST_THE_HISTORY).unwrap();
    assert_eq!((rows(&test, "commands"), rows(&test, "results")), (0, 0));
}

#[tokio::test]
async fn a_long_history_pages_by_cursor() {
    let test = start_hub(|config| {
        config.command_rate = RateLimit {
            burst: 100,
            per_minute: 100,
        };
        config.rate = config.command_rate;
    })
    .await;
    let (_machine, mut person) = commanding(&test).await;
    let machine_id = _machine.machine.clone();
    let mut sent = Vec::new();
    for _ in 0..52 {
        sent.push(person.command(&machine_id, &answer(Some(0), "")).await.unwrap().header.id);
    }
    let device = person.device_cert().unwrap().device.clone();
    let first = person.device_commands(&device, None).await.unwrap();
    assert_eq!(first.commands.len(), 50);
    let rest = person.device_commands(&device, first.cursor).await.unwrap();
    assert_eq!(rest.cursor, None);
    let all: Vec<_> = first.commands.iter().chain(&rest.commands).map(|entry| entry.command.clone()).collect();
    sent.reverse();
    assert_eq!(all, sent);
}
