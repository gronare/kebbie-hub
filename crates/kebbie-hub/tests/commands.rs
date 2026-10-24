#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::*;
use kebbie_hub::RateLimit;
use kebbie_hub_api::{
    CommandQueued, CommandUpload, EVENT_RESULT, ResultReady, ResultStored, ResultUpload,
};
use kebbie_hub_fakes::machine::{Handling, MachineState, Posted};
use kebbie_hub_fakes::phone::{PairedMachine, PhoneState};
use kebbie_hub_fakes::{Error, HubError};
use kebbie_hub_protocol::{
    Answer, CommandResult, Envelope, Header, Id, Kind, Outcome, Payload, Prefix, PushApproval,
    Reason, Role, SecretKeys, StartTask, TrustedDevice, seal,
};
use serde_json::Value;

const SOON: Duration = Duration::from_secs(5);
const ADDRESS: &str = "refunds@greenhouse";
const QUESTION: &str = "1761069540-4411-0-question";
const HEAD: &str = "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a";
const MOVED_HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

async fn paired(test: &TestHub) -> (MachineState, PhoneState) {
    let mut machine = joined_machine(test).await;
    let mut phone = phone(test).await;
    pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    machine.post_board(board(MARKER)).await.unwrap();
    (machine, phone)
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
    })
}

fn start(name: &str, prompt: &str) -> Payload {
    Payload::Start(StartTask {
        repo: "greenhouse".to_string(),
        name: name.to_string(),
        prompt: prompt.to_string(),
    })
}

fn hub_refusal<T: std::fmt::Debug>(result: Result<T, Error>) -> (u16, String) {
    match result {
        Err(Error::Hub(HubError::Status { status, code })) => (status, code),
        other => panic!("the hub accepted it: {other:?}"),
    }
}

async fn result_of(phone: &PhoneState, envelope: &Envelope) -> CommandResult {
    phone
        .await_result(&envelope.header.id, SOON)
        .await
        .unwrap()
        .expect("a result arrives")
}

fn refused(reason: Reason) -> Outcome {
    Outcome::Refused { reason }
}

#[tokio::test]
async fn an_answer_from_the_phone_is_applied_and_its_result_comes_back() {
    let test = start_hub(|_| {}).await;
    let (mut machine, phone) = paired(&test).await;
    let sent = phone
        .command(&machine.machine, &answer(Some(1), ""))
        .await
        .unwrap();

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
    let result = result_of(&phone, &sent).await;
    assert_eq!(result.command, sent.header.id);
    assert_eq!(result.outcome, Outcome::Done);
    assert_eq!(machine.applied.len(), 1);
}

#[tokio::test]
async fn a_read_only_device_is_refused_by_the_machine_and_hears_why() {
    let test = start_hub(|_| {}).await;
    let (mut machine, phone) = paired(&test).await;
    assert!(machine.set_role(phone.device.as_ref().unwrap(), Role::Read));
    let sent = phone
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();

    let handled = machine.apply_once(0, None).await.unwrap();
    assert!(
        matches!(&handled[0].handling, Handling::Refused(result) if result.outcome == refused(Reason::NotAllowed)),
        "{:?}",
        handled[0].handling
    );
    assert_eq!(
        result_of(&phone, &sent).await.outcome,
        refused(Reason::NotAllowed)
    );
}

#[tokio::test]
async fn a_command_delivered_twice_gets_the_stored_result_and_a_resend_queues_nothing() {
    let test = start_hub(|_| {}).await;
    let (mut machine, phone) = paired(&test).await;
    let envelope = phone
        .seal_command(
            &machine.machine,
            &answer(Some(1), ""),
            kebbie_hub::now(),
            None,
        )
        .unwrap();
    let first = phone.send_command(&envelope).await.unwrap();
    let again = phone.send_command(&envelope).await.unwrap();
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
    assert_eq!(&result_of(&phone, &envelope).await, result);
    let (_, received) = phone.results(0, 0).await.unwrap();
    assert_eq!(received.len(), 1);
}

#[tokio::test]
async fn a_command_for_a_machine_that_did_not_admit_the_device_is_refused_and_not_queued() {
    let test = start_hub(|_| {}).await;
    let (mut machine, mut phone) = paired(&test).await;
    let stranger = joined_machine(&test).await;
    phone.machines.push(PairedMachine {
        id: stranger.machine.clone(),
        keys: stranger.public(),
    });
    let elsewhere = phone.command(&stranger.machine, &answer(Some(0), "")).await;
    assert_eq!(hub_refusal(elsewhere), (404, "unknown-machine".to_string()));

    let device = phone.device.clone().unwrap();
    machine.remove_device(&device).await.unwrap();
    let removed = phone.command(&machine.machine, &answer(Some(0), "")).await;
    assert_eq!(hub_refusal(removed), (404, "unknown-machine".to_string()));
    assert_eq!(rows(&test, "queue"), 0);
    assert_eq!(rows(&test, "commands"), 0);
}

#[tokio::test]
async fn a_forged_or_misattributed_command_is_refused_with_400_and_not_queued() {
    let test = start_hub(|_| {}).await;
    let (machine, phone) = paired(&test).await;
    let device = phone.device.clone().unwrap();
    let impostor = SecretKeys::generate();
    let forged = seal(
        Header::new(
            Id::new(Prefix::Command).unwrap(),
            Kind::Answer,
            device.clone(),
            machine.machine.clone(),
            kebbie_hub::now(),
        ),
        &answer(Some(0), ""),
        &impostor,
        &[(machine.machine.clone(), machine.public())],
    )
    .unwrap();
    assert_eq!(
        hub_refusal(phone.send_command(&forged).await),
        (400, "bad-signature".to_string())
    );

    let genuine = phone
        .seal_command(
            &machine.machine,
            &answer(Some(0), ""),
            kebbie_hub::now(),
            None,
        )
        .unwrap();
    let other = device_client(&test, &signed_up(&test).await.register_device().await);
    let misattributed = other
        .post::<_, CommandQueued>(
            "/v1/d/commands",
            &CommandUpload {
                envelope: genuine.clone(),
            },
        )
        .await;
    assert_eq!(status_of(misattributed), 400);

    let mut tampered = genuine.clone();
    tampered.header.exp -= 1;
    assert_eq!(
        hub_refusal(phone.send_command(&tampered).await),
        (400, "bad-signature".to_string())
    );

    let board = machine
        .seal_board(board(MARKER), 9, kebbie_hub::now())
        .unwrap();
    assert_eq!(
        hub_refusal(phone.send_command(&board).await),
        (400, "not-a-command".to_string())
    );

    let mut sprayed = genuine;
    sprayed
        .keys
        .insert(Id::new(Prefix::Device).unwrap(), "AAAA".to_string());
    assert_eq!(
        hub_refusal(phone.send_command(&sprayed).await),
        (400, "bad-recipients".to_string())
    );
    assert_eq!(rows(&test, "queue"), 0);
    assert_eq!(rows(&test, "commands"), 0);
}

#[tokio::test]
async fn an_expired_command_is_refused_by_the_hub_or_by_the_machine() {
    let test = start_hub(|_| {}).await;
    let (mut machine, phone) = paired(&test).await;
    let stale = phone
        .seal_command(
            &machine.machine,
            &answer(Some(0), ""),
            kebbie_hub::now() - 700,
            Some(600),
        )
        .unwrap();
    assert_eq!(
        hub_refusal(phone.send_command(&stale).await),
        (400, "expired".to_string())
    );
    assert_eq!(rows(&test, "queue"), 0);

    let short = phone
        .seal_command(
            &machine.machine,
            &answer(Some(0), ""),
            kebbie_hub::now(),
            Some(1),
        )
        .unwrap();
    phone.send_command(&short).await.unwrap();
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let handled = machine.apply_once(0, None).await.unwrap();
    assert_eq!(
        handled[0].handling.result().unwrap().outcome,
        refused(Reason::Expired)
    );
    assert_eq!(
        result_of(&phone, &short).await.outcome,
        refused(Reason::Expired)
    );
}

#[tokio::test]
async fn a_push_of_a_head_that_moved_is_refused_head_moved() {
    let test = start_hub(|_| {}).await;
    let (mut machine, phone) = paired(&test).await;
    let boards = phone.boards().await.unwrap();
    let seen = boards[0].board.as_ref().unwrap().items[0]
        .head
        .clone()
        .unwrap();
    assert_eq!(seen, HEAD);

    let mut moved = board(MARKER);
    moved.items[0].head = Some(MOVED_HEAD.to_string());
    machine.post_board(moved).await.unwrap();
    let late = phone.command(&machine.machine, &push(&seen)).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    let result = result_of(&phone, &late).await;
    assert_eq!(result.outcome, refused(Reason::HeadMoved));

    let current = phone
        .command(&machine.machine, &push(MOVED_HEAD))
        .await
        .unwrap();
    machine.apply_once(0, None).await.unwrap();
    assert_eq!(result_of(&phone, &current).await.outcome, Outcome::Done);
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
    let (machine, phone) = paired(&test).await;
    for name in ["first-task", "second-task"] {
        phone
            .command(&machine.machine, &start(name, "do the thing"))
            .await
            .unwrap();
    }
    let third = phone
        .command(&machine.machine, &start("third-task", "do the thing"))
        .await;
    assert_eq!(hub_refusal(third), (429, "rate-limited".to_string()));
    phone
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    assert_eq!(rows(&test, "queue"), 3);
}

fn trusted(machine: &MachineState, device: &Id) -> TrustedDevice {
    let admitted = machine
        .devices
        .iter()
        .find(|admitted| &admitted.id == device)
        .unwrap();
    TrustedDevice {
        id: admitted.id.clone(),
        name: "phone".to_string(),
        keys: admitted.keys,
        role: Role::Control,
    }
}

fn done(command: &Id) -> CommandResult {
    CommandResult {
        command: command.clone(),
        outcome: Outcome::Done,
        text: "done".to_string(),
    }
}

#[tokio::test]
async fn a_result_reaches_only_the_device_that_sent_the_command() {
    let test = start_hub(|_| {}).await;
    let (mut machine, sender) = paired(&test).await;
    let mut bystander = phone(&test).await;
    pair_both(&mut machine, &mut bystander, true)
        .await
        .0
        .unwrap();
    let sender_id = sender.device.clone().unwrap();
    let bystander_id = bystander.device.clone().unwrap();

    let waiting = sender
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    let command = waiting.header.id.clone();
    let early = machine
        .post_result(&trusted(&machine, &sender_id), &done(&command))
        .await;
    assert_eq!(hub_refusal(early), (409, "not-delivered".to_string()));

    machine.poll_once(0).await.unwrap();
    let misdirected = machine
        .post_result(&trusted(&machine, &bystander_id), &done(&command))
        .await;
    assert_eq!(
        hub_refusal(misdirected),
        (404, "unknown-command".to_string())
    );
    let unknown = machine
        .post_result(
            &trusted(&machine, &sender_id),
            &done(&Id::new(Prefix::Command).unwrap()),
        )
        .await;
    assert_eq!(hub_refusal(unknown), (404, "unknown-command".to_string()));

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
        &[
            (sender_id.clone(), sender.public()),
            (bystander_id.clone(), bystander.public()),
        ],
    )
    .unwrap();
    let doubled = machine
        .client()
        .post::<_, Value>("/v1/m/results", &ResultUpload { envelope: to_both })
        .await;
    assert_eq!(status_of(doubled), 400);

    let impostor_result = {
        let mut envelope = machine
            .seal_result(&trusted(&machine, &sender_id), &done(&command))
            .unwrap();
        envelope.sig = sender
            .seal_command(
                &machine.machine,
                &answer(Some(0), ""),
                kebbie_hub::now(),
                None,
            )
            .unwrap()
            .sig;
        envelope
    };
    let forged = machine
        .client()
        .post::<_, Value>(
            "/v1/m/results",
            &ResultUpload {
                envelope: impostor_result,
            },
        )
        .await;
    assert_eq!(status_of(forged), 400);

    machine
        .post_result(&trusted(&machine, &sender_id), &done(&command))
        .await
        .unwrap();
    assert_eq!(result_of(&sender, &waiting).await, done(&command));
    let (_, seen_by_bystander) = bystander.results(0, 0).await.unwrap();
    assert!(seen_by_bystander.is_empty());
}

#[tokio::test]
async fn the_database_never_holds_command_text() {
    let test = start_hub(|_| {}).await;
    let (mut machine, phone) = paired(&test).await;
    let answered = phone
        .command(
            &machine.machine,
            &answer(None, &format!("{MARKER} comment")),
        )
        .await
        .unwrap();
    let started = phone
        .command(
            &machine.machine,
            &start("zebra-task", &format!("{MARKER} prompt")),
        )
        .await
        .unwrap();
    let pushed = phone.command(&machine.machine, &push(HEAD)).await.unwrap();
    machine.apply_once(0, None).await.unwrap();
    for sent in [&answered, &started, &pushed] {
        assert_eq!(result_of(&phone, sent).await.outcome, Outcome::Done);
    }

    let stored = database_bytes(&test);
    assert!(
        holds(&stored, answered.header.id.as_str().as_bytes()),
        "the scan did not read the commands"
    );
    for leak in [
        MARKER,
        "zebra-task",
        QUESTION,
        ADDRESS,
        HEAD,
        "Answered",
        "Started",
    ] {
        assert!(
            !holds(&stored, leak.as_bytes()),
            "the database holds {leak:?}"
        );
    }
}

#[tokio::test]
async fn the_feed_announces_a_result_by_command_id_only() {
    let test = start_hub(|_| {}).await;
    let mut machine = joined_machine(&test).await;
    let registered = signed_up(&test).await.register_device().await;
    let mut phone = PhoneState::registered(&test.url, registered.device.clone(), &registered.token);
    pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    machine.post_board(board(MARKER)).await.unwrap();
    let mut events = EventStream::open(&test.url, &registered.token).await;

    let sent = phone
        .command(
            &machine.machine,
            &answer(None, &format!("{MARKER} comment")),
        )
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
    assert_eq!(events.next(Duration::from_millis(300)).await, Next::Quiet);

    let mut fresh = EventStream::open(&test.url, &registered.token).await;
    assert_eq!(
        fresh.until(EVENT_RESULT, SOON).await["command"],
        sent.header.id.as_str()
    );
    let (_, received) = phone.results(0, 0).await.unwrap();
    let mut resumed = EventStream::open_after(&test.url, &registered.token, received[0].seq).await;
    loop {
        match resumed.next(Duration::from_millis(300)).await {
            Next::Event(name, _) if name == EVENT_RESULT => panic!("announced again"),
            Next::Event(..) => continue,
            Next::Quiet => break,
            Next::Ended => panic!("the feed ended"),
        }
    }
}

const PAST_THE_WINDOW: u64 = 8 * 86_400;

async fn paired_with_account(test: &TestHub) -> (MachineState, PhoneState, Person) {
    let mut machine = joined_machine(test).await;
    let person = signed_up(test).await;
    let registered = person.register_device().await;
    let mut phone = PhoneState::registered(&test.url, registered.device, &registered.token);
    pair_both(&mut machine, &mut phone, true).await.0.unwrap();
    machine.post_board(board(MARKER)).await.unwrap();
    (machine, phone, person)
}

#[tokio::test]
async fn a_machine_back_after_the_command_window_drains_its_queue() {
    let test = start_hub(|_| {}).await;
    let (mut machine, phone) = paired(&test).await;
    phone
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    test.hub
        .store()
        .sweep(kebbie_hub::now() + PAST_THE_WINDOW)
        .unwrap();
    assert_eq!(rows(&test, "queue"), 0);
    assert_eq!(rows(&test, "commands"), 0);
    assert!(machine.apply_once(0, None).await.unwrap().is_empty());

    let fresh = phone
        .command(&machine.machine, &answer(Some(1), ""))
        .await
        .unwrap();
    machine.apply_once(0, None).await.unwrap();
    assert_eq!(result_of(&phone, &fresh).await.outcome, Outcome::Done);
}

#[tokio::test]
async fn a_result_for_a_command_the_hub_dropped_is_dropped_and_the_inbox_moves_on() {
    let test = start_hub(|_| {}).await;
    let (mut machine, phone, person) = paired_with_account(&test).await;
    phone
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    machine.poll_once(0).await.unwrap();
    machine.inbox_seq = 0;
    let device = phone.device.clone().unwrap();
    let removed = person.delete(&format!("/v1/a/devices/{device}")).await;
    assert!(removed.status().is_success());

    let handled = machine.apply_once(0, None).await.unwrap();
    assert_eq!(handled.len(), 1);
    assert_eq!(handled[0].posted, Some(Posted::CommandGone));
    assert!(machine.apply_once(0, None).await.unwrap().is_empty());
    assert_eq!(rows(&test, "queue"), 0);
}

#[tokio::test]
async fn a_revoked_device_s_undelivered_commands_never_reach_the_machine() {
    let test = start_hub(|_| {}).await;
    let (mut machine, phone, person) = paired_with_account(&test).await;
    phone
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    let device = phone.device.clone().unwrap();
    let removed = person.delete(&format!("/v1/a/devices/{device}")).await;
    assert!(removed.status().is_success());
    assert!(machine.poll_once(0).await.unwrap().messages.is_empty());
    assert_eq!(rows(&test, "queue"), 0);

    let (mut machine, phone) = paired(&test).await;
    phone
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    machine
        .remove_device(&phone.device.clone().unwrap())
        .await
        .unwrap();
    assert!(machine.poll_once(0).await.unwrap().messages.is_empty());
    assert_eq!(rows(&test, "queue"), 0);
    assert_eq!(rows(&test, "commands"), 0);
}

async fn send(phone: &PhoneState, machine: &Id) -> Result<CommandQueued, Error> {
    let envelope = phone
        .seal_command(machine, &answer(Some(0), ""), kebbie_hub::now(), None)
        .unwrap();
    phone.send_command(&envelope).await
}

#[tokio::test]
async fn a_device_and_a_machine_each_hold_only_so_many_undelivered_commands() {
    let test = start_hub(|config| {
        config.max_queued = 3;
        config.max_undelivered_per_device = 2;
    })
    .await;
    let (mut machine, first) = paired(&test).await;
    let mut second = phone(&test).await;
    pair_both(&mut machine, &mut second, true).await.0.unwrap();
    let target = machine.machine.clone();
    send(&first, &target).await.unwrap();
    send(&first, &target).await.unwrap();
    assert_eq!(
        hub_refusal(send(&first, &target).await),
        (429, "queue-full".to_string())
    );
    send(&second, &target).await.unwrap();
    assert_eq!(
        hub_refusal(send(&second, &target).await),
        (429, "queue-full".to_string())
    );

    machine.poll_once(0).await.unwrap();
    machine.poll_once(0).await.unwrap();
    send(&first, &target).await.unwrap();
}

#[tokio::test]
async fn result_positions_count_per_device() {
    let test = start_hub(|_| {}).await;
    let (mut machine, first) = paired(&test).await;
    let mut second = phone(&test).await;
    pair_both(&mut machine, &mut second, true).await.0.unwrap();
    first
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    second
        .command(&machine.machine, &answer(Some(1), ""))
        .await
        .unwrap();
    machine.apply_once(0, None).await.unwrap();
    for phone in [&first, &second] {
        let (cursor, received) = phone.results(0, 0).await.unwrap();
        assert_eq!((cursor, received.len(), received[0].seq), (1, 1, 1));
    }
}

#[tokio::test]
async fn a_last_event_id_past_what_sqlite_holds_is_refused_with_400() {
    let test = start_hub(|_| {}).await;
    let registered = signed_up(&test).await.register_device().await;
    for id in ["9223372036854775808", "soon"] {
        let response = reqwest::Client::new()
            .get(format!("{}/v1/d/events", test.url))
            .bearer_auth(&registered.token)
            .header("last-event-id", id)
            .send()
            .await
            .unwrap();
        assert_eq!(
            refusal(response).await,
            (400, "bad-last-event-id".to_string())
        );
    }
}

#[tokio::test]
async fn a_second_result_for_a_command_must_be_the_same_envelope() {
    let test = start_hub(|_| {}).await;
    let (mut machine, phone) = paired(&test).await;
    let sent = phone
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    machine.poll_once(0).await.unwrap();
    let device = trusted(&machine, phone.device.as_ref().unwrap());
    let command = sent.header.id.clone();
    let envelope = machine.seal_result(&device, &done(&command)).unwrap();
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
    let other = machine.seal_result(&device, &done(&command)).unwrap();
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
    let (machine, phone) = paired(&test).await;
    let genuine = phone
        .seal_command(
            &machine.machine,
            &answer(Some(0), ""),
            kebbie_hub::now(),
            None,
        )
        .unwrap();
    phone.send_command(&genuine).await.unwrap();
    let mut changed = genuine;
    changed.header.exp -= 1;
    assert_eq!(
        hub_refusal(phone.send_command(&changed).await),
        (400, "bad-signature".to_string())
    );
    assert_eq!(rows(&test, "queue"), 1);
}

#[tokio::test]
async fn each_kind_has_its_own_ciphertext_cap() {
    let test = start_hub(|_| {}).await;
    let (machine, phone) = paired(&test).await;
    let padded = |payload: &Payload, chars: usize| {
        let mut envelope = phone
            .seal_command(&machine.machine, payload, kebbie_hub::now(), None)
            .unwrap();
        envelope.ct = "A".repeat(chars);
        envelope
    };
    let refused_with = async |envelope: Envelope| hub_refusal(phone.send_command(&envelope).await);
    assert_eq!(
        refused_with(padded(&push(HEAD), 5_000)).await,
        (413, "too-large".to_string())
    );
    assert_eq!(
        refused_with(padded(&answer(Some(0), ""), 5_000)).await,
        (400, "bad-signature".to_string())
    );
    assert_eq!(
        refused_with(padded(&start("big-task", "x"), 37 * 1024)).await,
        (413, "too-large".to_string())
    );
    assert_eq!(rows(&test, "queue"), 0);
}

#[tokio::test]
async fn a_device_cannot_reuse_another_devices_command_id() {
    let test = start_hub(|_| {}).await;
    let (mut machine, first) = paired(&test).await;
    let mut second = phone(&test).await;
    pair_both(&mut machine, &mut second, true).await.0.unwrap();
    let taken = first
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    let reused = seal(
        Header::new(
            taken.header.id.clone(),
            Kind::Answer,
            second.device.clone().unwrap(),
            machine.machine.clone(),
            kebbie_hub::now(),
        ),
        &answer(Some(1), ""),
        &second.secret(),
        &[(machine.machine.clone(), machine.public())],
    )
    .unwrap();
    assert_eq!(
        hub_refusal(second.send_command(&reused).await),
        (409, "command-id-taken".to_string())
    );
    assert_eq!(rows(&test, "queue"), 1);
}

#[tokio::test]
async fn the_results_endpoint_takes_only_results_about_the_posting_machine() {
    let test = start_hub(|_| {}).await;
    let (machine, phone) = paired(&test).await;
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
        &[(phone.device.clone().unwrap(), phone.public())],
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
async fn the_sweep_forgets_commands_and_results_after_their_week() {
    let test = start_hub(|_| {}).await;
    let (mut machine, phone) = paired(&test).await;
    let sent = phone
        .command(&machine.machine, &answer(Some(0), ""))
        .await
        .unwrap();
    machine.apply_once(0, None).await.unwrap();
    result_of(&phone, &sent).await;
    assert_eq!((rows(&test, "commands"), rows(&test, "results")), (1, 1));
    test.hub
        .store()
        .sweep(kebbie_hub::now() + PAST_THE_WINDOW)
        .unwrap();
    for table in ["queue", "commands", "results"] {
        assert_eq!(rows(&test, table), 0, "{table}");
    }
}
