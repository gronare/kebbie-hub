#[allow(dead_code)]
mod common;

use axum::Router;
use common::*;
use kebbie_hub::store::Store;
use kebbie_hub::{Gated, Refusal};
use kebbie_hub_api::{Inbox, NO_ACCOUNT, NO_JOB, NOT_A_WORKER};
use kebbie_hub_fakes::Error;
use kebbie_hub_fakes::browser::Browser;
use kebbie_hub_fakes::machine::{Handling, MachineState};
use kebbie_hub_protocol::{
    Header, Id, Job, JobEnd, JobReport, Kind, OpenError, Payload, Prefix, RunRoutine, SecretKeys, open_blob, seal, seal_blob,
};

const COMMIT: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const BASE: &str = "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a";

fn job(bundle: Vec<kebbie_hub_protocol::BlobRef>) -> Job {
    Job {
        repo: "kebbie".into(),
        name: "ios-tests".into(),
        command: "xcodebuild test -scheme Kebbie".into(),
        task: "email-column".into(),
        commit: COMMIT.into(),
        base: Some(BASE.into()),
        bundle,
        on: Some("macos".into()),
    }
}

fn report(job: &Id, seq: u32, log: &str, end: Option<JobEnd>) -> JobReport {
    JobReport { job: job.clone(), seq, log: log.into(), end }
}

fn refused<T: std::fmt::Debug>(result: Result<T, Error>) -> (u16, String) {
    match result {
        Err(Error::Hub(error)) => hub_code(error),
        other => panic!("the hub accepted it: {other:?}"),
    }
}

async fn two_machines(test: &TestHub) -> (Browser, MachineState, MachineState) {
    let mut person = member(test).await;
    let master = connected_with_board(test, &mut person).await;
    let worker = connected_with_board(test, &mut person).await;
    (person, master, worker)
}

fn header(kind: Kind, from: &MachineState, to: &Id) -> Header {
    Header::new(Id::new(Prefix::Command).unwrap(), kind, from.machine.clone(), to.clone(), kebbie_hub::now())
}

#[tokio::test]
async fn two_machines_of_one_account_exchange_a_job_and_its_reports_with_the_bundle_fetched_across() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let (_person, mut master, mut worker) = two_machines(&test).await;
    assert_eq!(refused(master.send_job(&worker.machine, job(Vec::new())).await), (409, NOT_A_WORKER.to_string()));
    assert_eq!(worker.set_worker("macos").await.unwrap(), "macos");
    assert_eq!(refused(worker.set_worker("macOS").await), (400, "bad-os".to_string()));

    let listed = master.machines().await.unwrap();
    let mut seen: Vec<_> = listed.machines.iter().map(|row| (row.machine.clone(), row.worker_os.clone(), row.last_seen.is_some())).collect();
    seen.sort();
    let mut expected = vec![(master.machine.clone(), None, true), (worker.machine.clone(), Some("macos".to_string()), true)];
    expected.sort();
    assert_eq!(seen, expected);
    assert_eq!(master.peer_keys(&listed, &worker.machine).unwrap(), worker.public(), "the worker's keys come from the account-signed list");

    let bundle: Vec<u8> = (0..300 * 1024).map(|index| (index % 251) as u8).collect();
    let parts = master.put_bundle(&bundle).await.unwrap();
    assert_eq!(parts.len(), 2);
    let sent = job(parts);
    let (envelope, queued) = master.send_job(&worker.machine, sent.clone()).await.unwrap();
    assert_eq!(queued.message, envelope.header.id);

    let received = worker.receive(0).await.unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].envelope.keys.keys().collect::<Vec<_>>(), [&worker.machine], "the worker's inbox holds only its own key row");
    assert_eq!(received[0].opened, Ok(Payload::Job(sent.clone())));
    assert_eq!(worker.fetch_bundle(&sent.bundle).await.unwrap(), bundle);

    let (artifact, ciphertext) = seal_blob(b"Executed 214 tests, with 0 failures", "text/plain");
    worker.put_blob(&artifact.id, ciphertext).await.unwrap();
    let job_id = envelope.header.id.clone();
    worker.send_report(&master.machine, report(&job_id, 0, "Building Kebbie\n", None)).await.unwrap();
    let done = report(&job_id, 1, "Test Suite passed\n", Some(JobEnd::Exited { code: 0, artifacts: vec![artifact.clone()] }));
    worker.send_report(&master.machine, done.clone()).await.unwrap();

    let reports: Vec<_> = master.receive(0).await.unwrap().into_iter().map(|received| received.opened.unwrap()).collect();
    assert_eq!(reports, [Payload::JobReport(report(&job_id, 0, "Building Kebbie\n", None)), Payload::JobReport(done)]);
    let fetched = master.fetch_blob(&artifact.id).await.unwrap();
    assert_eq!(open_blob(&artifact, &fetched).unwrap(), b"Executed 214 tests, with 0 failures");

    worker.clear_worker().await.unwrap();
    assert_eq!(master.machines().await.unwrap().machines.iter().find(|row| row.machine == worker.machine).unwrap().worker_os, None);
    assert_eq!(refused(master.send_job(&worker.machine, job(Vec::new())).await), (409, NOT_A_WORKER.to_string()));
    let events = test.hub.store().events().unwrap();
    assert_eq!(events.iter().filter(|(kind, _)| kind == "message.queued").count(), 3);
}

#[tokio::test]
async fn the_hub_queues_only_a_message_its_sender_signed_and_sealed_to_the_target_the_sender_and_the_account() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let (person, master, worker) = two_machines(&test).await;
    worker.set_worker("macos").await.unwrap();
    let account = person.account_key().unwrap().clone();
    let payload = Payload::Job(job(Vec::new()));
    let to_worker = |keys: &[(Id, kebbie_hub_protocol::PublicKeys)], signer: &SecretKeys| seal(header(Kind::Job, &master, &worker.machine), &payload, signer, keys).unwrap();
    let all = [(worker.machine.clone(), worker.public()), (master.machine.clone(), master.public()), account.recipient()];
    assert!(master.post_message(&to_worker(&all, &master.secret())).await.is_ok());

    assert_eq!(refused(master.post_message(&to_worker(&all[..2], &master.secret())).await), (400, "bad-recipients".to_string()), "the account reads every job");
    assert_eq!(refused(master.post_message(&to_worker(&[all[0].clone(), all[2].clone()], &master.secret())).await), (400, "bad-recipients".to_string()), "the sender reads its own job");
    let stranger = (Id::new(Prefix::Machine).unwrap(), SecretKeys::generate().public());
    assert_eq!(refused(master.post_message(&to_worker(&[all[0].clone(), all[1].clone(), all[2].clone(), stranger.clone()], &master.secret())).await), (400, "bad-recipients".to_string()));
    assert_eq!(refused(master.post_message(&to_worker(&[all[0].clone(), all[1].clone(), stranger], &master.secret())).await), (400, "bad-recipients".to_string()));
    assert_eq!(refused(master.post_message(&to_worker(&all, &SecretKeys::generate())).await), (400, OpenError::BadSignature.key().to_string()));
    assert_eq!(refused(worker.post_message(&to_worker(&all, &master.secret())).await), (400, OpenError::WrongSender.key().to_string()), "a machine sends only as itself");

    let huge = Payload::Job(Job { command: "x".repeat(40_000), ..job(Vec::new()) });
    assert!(huge.validate().is_err());
    let huge = seal(header(Kind::Job, &master, &worker.machine), &huge, &master.secret(), &all).unwrap();
    assert_eq!(refused(master.post_message(&huge).await), (413, "too-large".to_string()));

    let board = master.seal_board(board(MARKER), 1, kebbie_hub::now()).unwrap();
    assert_eq!(refused(master.post_message(&board).await), (400, "not-a-machine-message".to_string()));
    let mut to_itself = header(Kind::JobReport, &master, &master.machine);
    to_itself.exp = to_itself.at + 60;
    let report_to_itself = Payload::JobReport(report(&Id::new(Prefix::Command).unwrap(), 0, "", None));
    let malformed = kebbie_hub_protocol::Envelope { header: to_itself, ..master.seal_message(&worker.machine, &worker.public(), &report_to_itself, kebbie_hub::now()).unwrap() };
    assert_eq!(refused(master.post_message(&malformed).await), (400, OpenError::Malformed.key().to_string()));

    let worker_inbox: Inbox = worker.client().get("/v1/m/inbox", &[("after", "0".to_string())]).await.unwrap();
    assert_eq!(worker_inbox.messages.len(), 1, "only the one good job was queued");
}

#[tokio::test]
async fn a_message_reaches_only_a_machine_of_the_senders_own_account() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let (_person, master, worker) = two_machines(&test).await;
    let mut stranger = member(&test).await;
    let theirs = connected(&test, &mut stranger).await;
    theirs.set_worker("linux").await.unwrap();
    let joined = joined_machine(&test).await;
    let report = Payload::JobReport(report(&Id::new(Prefix::Command).unwrap(), 0, "", None));
    for target in [&theirs, &joined] {
        let envelope = master.seal_message(&target.machine, &target.public(), &Payload::Job(job(Vec::new())), kebbie_hub::now()).unwrap();
        assert_eq!(refused(master.post_message(&envelope).await), (404, "unknown-machine".to_string()));
        let envelope = master.seal_message(&target.machine, &target.public(), &report, kebbie_hub::now()).unwrap();
        assert_eq!(refused(master.post_message(&envelope).await), (404, "unknown-machine".to_string()));
    }
    assert!(!master.machines().await.unwrap().machines.iter().any(|row| row.machine == theirs.machine || row.machine == joined.machine));

    let (blob, ciphertext) = seal_blob(b"their bundle", "application/octet-stream");
    theirs.put_blob(&blob.id, ciphertext.clone()).await.unwrap();
    assert_eq!(refused(master.fetch_blob(&blob.id).await), (404, "not-found".to_string()));
    assert_eq!(theirs.fetch_blob(&blob.id).await.unwrap(), ciphertext, "a machine reads its own blob back");
    let (ours, ciphertext) = seal_blob(b"our bundle", "application/octet-stream");
    worker.put_blob(&ours.id, ciphertext.clone()).await.unwrap();
    assert_eq!(master.fetch_blob(&ours.id).await.unwrap(), ciphertext);
    assert_eq!(refused(theirs.fetch_blob(&ours.id).await), (404, "not-found".to_string()));
    assert_eq!(refused(master.fetch_blob(&Id::new(Prefix::Command).unwrap()).await), (404, "not-found".to_string()));
}

#[tokio::test]
async fn a_machine_joined_with_a_code_has_no_account_and_is_refused_every_machine_route() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let (person, _master, worker) = two_machines(&test).await;
    let mut joined = joined_machine(&test).await;
    let no_account = (409, NO_ACCOUNT.to_string());
    assert_eq!(refused(joined.set_worker("macos").await), no_account);
    assert_eq!(refused(joined.clear_worker().await), no_account);
    assert_eq!(refused(joined.machines().await), no_account);
    joined.account = person.account_key().cloned();
    worker.set_worker("macos").await.unwrap();
    let envelope = joined.seal_message(&worker.machine, &worker.public(), &Payload::Job(job(Vec::new())), kebbie_hub::now()).unwrap();
    assert_eq!(refused(joined.post_message(&envelope).await), no_account);
    let (blob, ciphertext) = seal_blob(b"bundle", "application/octet-stream");
    worker.put_blob(&blob.id, ciphertext).await.unwrap();
    assert_eq!(refused(joined.fetch_blob(&blob.id).await), (404, "not-found".to_string()));
    assert_eq!(test.hub.store().worker_os(&joined.machine).unwrap(), None);
}

#[tokio::test]
async fn a_job_passes_the_plans_gate_as_a_command_does_and_its_report_is_not_gated() {
    let refusing = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let gate = refusing.clone();
    let refuse_jobs = move |_: &Store, _: &str, gated: Gated| match gated {
        Gated::Command(Kind::Job | Kind::JobReport) if gate.load(std::sync::atomic::Ordering::SeqCst) => Err(Refusal { url: Some("https://example.com/pro".into()), pro: true }),
        _ => Ok(()),
    };
    let test = start_hub_with(|config| config.connect_interval = 0, Some(Box::new(refuse_jobs)), Router::new()).await;
    let (_person, master, worker) = two_machines(&test).await;
    worker.set_worker("macos").await.unwrap();
    master.send_job(&worker.machine, job(Vec::new())).await.unwrap();
    refusing.store(true, std::sync::atomic::Ordering::SeqCst);
    match master.send_job(&worker.machine, job(Vec::new())).await {
        Err(Error::Hub(kebbie_hub_fakes::HubError::ProRequired { url })) => assert_eq!(url.as_deref(), Some("https://example.com/pro")),
        other => panic!("a job is pro: {other:?}"),
    }
    let answer = report(&Id::new(Prefix::Command).unwrap(), 0, "", Some(JobEnd::Refused { reason: "not a worker for kebbie".into() }));
    assert!(worker.send_report(&master.machine, answer).await.is_ok());
}

#[tokio::test]
async fn a_full_queue_takes_no_more_messages() {
    let test = start_hub(|config| {
        config.connect_interval = 0;
        config.max_queued = 1;
    })
    .await;
    let (_person, master, worker) = two_machines(&test).await;
    worker.set_worker("macos").await.unwrap();
    master.send_job(&worker.machine, job(Vec::new())).await.unwrap();
    assert_eq!(refused(master.send_job(&worker.machine, job(Vec::new())).await), (429, "queue-full".to_string()));
}

#[tokio::test]
async fn a_machine_takes_a_report_only_from_a_worker_it_sent_a_job() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let mut person = member(&test).await;
    let master = connected_with_board(&test, &mut person).await;
    let worker = connected_with_board(&test, &mut person).await;
    let other = connected_with_board(&test, &mut person).await;
    let job_id = Id::new(Prefix::Command).unwrap();
    assert_eq!(refused(worker.send_report(&master.machine, report(&job_id, 0, "unasked", None)).await), (409, NO_JOB.to_string()), "a machine that sent no job, as one on an older kebbie, is never sent a report");
    worker.set_worker("macos").await.unwrap();
    other.set_worker("linux").await.unwrap();
    master.send_job(&worker.machine, job(Vec::new())).await.unwrap();
    assert!(worker.send_report(&master.machine, report(&job_id, 0, "building", None)).await.is_ok());
    assert_eq!(refused(other.send_report(&master.machine, report(&job_id, 0, "not mine", None)).await), (409, NO_JOB.to_string()), "only the worker the job went to");
    assert_eq!(refused(master.send_report(&worker.machine, report(&job_id, 0, "backwards", None)).await), (409, NO_JOB.to_string()), "a report goes from the worker to its master, not back");
}

#[tokio::test]
async fn the_inbox_serves_a_devices_command_as_before_beside_a_job() {
    let test = start_hub(|config| config.connect_interval = 0).await;
    let (mut person, master, mut worker) = two_machines(&test).await;
    person.knows(&worker.machine, worker.public());
    worker.post_board(board(MARKER)).await.unwrap();
    worker.set_worker("macos").await.unwrap();
    let command = person.command(&worker.machine, &Payload::Routine(RunRoutine { id: "nightly-deps".into() })).await.unwrap();
    let (job_envelope, _) = master.send_job(&worker.machine, job(Vec::new())).await.unwrap();

    let inbox: Inbox = worker.client().get("/v1/m/inbox", &[("after", "0".to_string())]).await.unwrap();
    assert_eq!(inbox.messages.iter().map(|message| message.envelope.header.kind).collect::<Vec<_>>(), [Kind::Routine, Kind::Job]);
    assert_eq!(inbox.messages[0].envelope, command, "a command keeps the machine's and the account's rows");
    assert_eq!(inbox.messages[1].envelope, job_envelope.for_recipient(&worker.machine).unwrap());

    let handled = worker.apply_once(0, None).await.unwrap();
    assert_eq!(handled.len(), 1);
    assert!(matches!(handled[0].handling, Handling::Applied { .. }), "{:?}", handled[0].handling);
    let received = worker.receive(0).await.unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].opened, Ok(Payload::Job(job(Vec::new()))));
    assert!(worker.receive(0).await.unwrap().is_empty());
}
