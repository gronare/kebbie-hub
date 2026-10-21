use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::authorize::{AppliedCommands, Machine, Refusal, Role, TrustedDevice, authorize};
use crate::board::{
    Agent, AgentStatus, Board, Checkout, Git, Group, Item, Question, QuestionOption, Report,
    RoutineSummary,
};
use crate::crypto::{
    OpenError, PublicKeys, Randomness, SecretKeys, decrypt, key_commitment, open, seal_unchecked,
    seal_with, wrap_key,
};
use crate::envelope::{Envelope, Header, Kind, Urgency};
use crate::ids::{Id, Prefix};
use crate::message::{
    Answer, CommandResult, Event, Notice, Outcome, Payload, PushApproval, Reason, RunRoutine,
    StartTask,
};
use crate::pairing::{DevicePairing, MachinePairing};

pub const NOW: u64 = 1_761_070_000;
const AT: u64 = 1_761_069_600;
const HEAD: &str = "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a";
const QUESTION: &str = "1761069540-4411-0-question";

struct Party {
    name: &'static str,
    id: Id,
    keys: SecretKeys,
}

impl Party {
    fn new(name: &'static str, prefix: Prefix) -> Party {
        let id = Id::ulid(
            prefix,
            AT * 1_000,
            seed(&format!("{name}/id"))[..10].try_into().unwrap(),
        )
        .unwrap();
        let (signing, encryption) = SecretKeys::from_bytes(
            seed(&format!("{name}/sign")),
            seed(&format!("{name}/encrypt")),
        )
        .to_bytes();
        Party {
            name,
            id,
            keys: SecretKeys::from_bytes(signing, encryption),
        }
    }

    fn public(&self) -> PublicKeys {
        self.keys.public()
    }

    fn to_json(&self) -> Value {
        let (signing, encryption) = self.keys.to_bytes();
        json!({ "id": self.id, "signing_secret": b64(&signing), "encryption_secret": b64(&encryption), "public": self.public() })
    }
}

struct Parties {
    machine: Party,
    other_machine: Party,
    phone: Party,
    tablet: Party,
    intruder: Party,
}

impl Parties {
    fn new() -> Parties {
        Parties {
            machine: Party::new("machine", Prefix::Machine),
            other_machine: Party::new("other-machine", Prefix::Machine),
            phone: Party::new("phone", Prefix::Device),
            tablet: Party::new("tablet", Prefix::Device),
            intruder: Party::new("intruder", Prefix::Device),
        }
    }

    fn all(&self) -> [&Party; 5] {
        [
            &self.machine,
            &self.other_machine,
            &self.phone,
            &self.tablet,
            &self.intruder,
        ]
    }

    fn trusted(&self) -> Vec<TrustedDevice> {
        vec![
            TrustedDevice {
                id: self.phone.id.clone(),
                name: "iPhone".into(),
                keys: self.phone.public(),
                role: Role::Control,
            },
            TrustedDevice {
                id: self.tablet.id.clone(),
                name: "iPad".into(),
                keys: self.tablet.public(),
                role: Role::Read,
            },
        ]
    }
}

fn seed(label: &str) -> [u8; 32] {
    Sha256::digest(format!("kelpie-hub test vector {label}")).into()
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn randomness(case: &str, recipients: &[(Id, PublicKeys)]) -> Randomness {
    Randomness {
        content_key: seed(&format!("{case}/content-key")),
        nonce: seed(&format!("{case}/nonce"))[..12].try_into().unwrap(),
        ephemeral: recipients
            .iter()
            .map(|(id, _)| (id.clone(), seed(&format!("{case}/ephemeral/{id}"))))
            .collect(),
    }
}

fn command_id(case: &str) -> Id {
    Id::command(
        AT * 1_000,
        seed(&format!("{case}/command"))[..10].try_into().unwrap(),
    )
}

fn time_id(case: &str, prefix: Prefix) -> Id {
    Id::ulid(
        prefix,
        AT * 1_000,
        seed(&format!("{case}/id"))[..10].try_into().unwrap(),
    )
    .unwrap()
}

fn board() -> Payload {
    Payload::Board(Board {
        rev: 1842,
        kelpie: "0.24.0".into(),
        away_since: None,
        checkouts: vec![Checkout {
            repo: "greenhouse".into(),
            free_worktrees: 2,
            busy_worktrees: 4,
        }],
        routines: vec![RoutineSummary {
            id: "nightly-deps".into(),
            title: "Nightly deps".into(),
            repo: "greenhouse".into(),
            mode: "fix".into(),
            next_run: Some(1_761_100_200),
            last_outcome: Some("ok".into()),
        }],
        items: vec![Item {
            address: "refunds@greenhouse".into(),
            group: Group::NeedsYou,
            urgency: 1,
            summary: "2 commits · 1 behind main".into(),
            next_for_you: Some("answer: Keep the old endpoint for a release?".into()),
            agent: Some(Agent {
                status: AgentStatus::Idle,
            }),
            report: Some(Report {
                state: "needs-you".into(),
                next: Some("answer: Keep the old endpoint for a release?".into()),
                summary: "asked the user: Keep the old endpoint for a release?".into(),
                at: 1_761_069_540,
            }),
            question: Some(Question {
                id: QUESTION.into(),
                text: "Keep the old endpoint for a release?".into(),
                options: vec![
                    QuestionOption {
                        label: "Keep it".into(),
                        meaning: "deprecate now, remove in the next release".into(),
                    },
                    QuestionOption {
                        label: "Remove it".into(),
                        meaning: "drop it in this task".into(),
                    },
                ],
                recommend: Some(1),
                kept_for_user: false,
                plan: None,
                at: 1_761_069_540,
            }),
            prompt: None,
            git: Some(Git {
                upstream: "main".into(),
                ahead: 2,
                behind: 1,
                uncommitted: 0,
                approved: false,
            }),
            head: Some(HEAD.into()),
            verdict: None,
            push_permission: None,
            push_waits: None,
            follows_main: false,
            ci: None,
            recover: None,
            routine: None,
            card: None,
        }],
        sessions_outside_tasks: 1,
    })
}

fn answer_to(address: &str) -> Payload {
    Payload::Answer(Answer {
        address: address.into(),
        question: QUESTION.into(),
        choice: Some(0),
        comment: String::new(),
    })
}

fn answer() -> Payload {
    answer_to("refunds@greenhouse")
}

fn push(head: &str) -> Payload {
    Payload::Push(PushApproval {
        address: "refunds@greenhouse".into(),
        head: head.into(),
        card_sha: None,
    })
}

fn notice(retract: Option<String>) -> Payload {
    Payload::Notice(Notice {
        event: Event::Question,
        address: Some("refunds@greenhouse".into()),
        title: "refunds asks".into(),
        body: "Keep the old endpoint for a release?".into(),
        question: Some(QUESTION.into()),
        options: vec!["Keep it".into(), "Remove it".into()],
        recommend: Some(0),
        board_rev: 1842,
        retract,
    })
}

fn result() -> Payload {
    Payload::Result(CommandResult {
        command: command_id("answer"),
        outcome: Outcome::Refused {
            reason: Reason::AlreadySettled,
        },
        text: "That question no longer waits for an answer".into(),
    })
}

struct Sealed {
    envelope: Envelope,
    randomness: Randomness,
}

fn recipients_of(parties: &[&Party]) -> Vec<(Id, PublicKeys)> {
    parties
        .iter()
        .map(|party| (party.id.clone(), party.public()))
        .collect()
}

fn seal_case(
    case: &str,
    header: Header,
    payload: &Payload,
    sender: &Party,
    recipients: &[&Party],
) -> Sealed {
    let recipients = recipients_of(recipients);
    let randomness = randomness(case, &recipients);
    let envelope = seal_with(header, payload, &sender.keys, &recipients, &randomness).unwrap();
    Sealed {
        envelope,
        randomness,
    }
}

fn seal_bypassing_checks(
    case: &str,
    mut header: Header,
    plaintext: &[u8],
    sender: &Party,
    recipients: &[&Party],
) -> Envelope {
    let recipients = recipients_of(recipients);
    let randomness = randomness(case, &recipients);
    header.kc = key_commitment(&randomness.content_key);
    seal_unchecked(header, plaintext, &sender.keys, &recipients, &randomness)
}

fn command_header(case: &str, kind: Kind, from: &Party, machine: &Party) -> Header {
    Header::new(
        command_id(case),
        kind,
        from.id.clone(),
        machine.id.clone(),
        AT,
    )
}

fn machine_header(case: &str, kind: Kind, machine: &Party) -> Header {
    let id = if kind == Kind::Result {
        command_id(case)
    } else {
        time_id(case, kind.id_prefix())
    };
    Header::new(id, kind, machine.id.clone(), machine.id.clone(), AT)
}

fn stale(mut header: Header) -> Header {
    header.at = NOW - 7_200;
    header.exp = NOW - 3_600;
    header
}

struct EnvelopeCase<'a> {
    name: &'static str,
    envelope: Envelope,
    randomness: Option<Randomness>,
    recipient: &'a Party,
    sender: &'a Party,
    expect: Result<Payload, OpenError>,
    decrypt_expect: Option<OpenError>,
}

fn opens<'a>(
    name: &'static str,
    sealed: &Sealed,
    recipient: &'a Party,
    sender: &'a Party,
    payload: Payload,
) -> EnvelopeCase<'a> {
    EnvelopeCase {
        name,
        envelope: sealed.envelope.clone(),
        randomness: Some(sealed.randomness.clone()),
        recipient,
        sender,
        expect: Ok(payload),
        decrypt_expect: None,
    }
}

fn fails<'a>(
    name: &'static str,
    envelope: Envelope,
    recipient: &'a Party,
    sender: &'a Party,
    error: OpenError,
    decrypt_error: Option<OpenError>,
) -> EnvelopeCase<'a> {
    EnvelopeCase {
        name,
        envelope,
        randomness: None,
        recipient,
        sender,
        expect: Err(error),
        decrypt_expect: decrypt_error,
    }
}

fn envelope_cases(parties: &Parties) -> Vec<EnvelopeCase<'_>> {
    let Parties {
        machine,
        phone,
        tablet,
        intruder,
        other_machine,
    } = parties;
    let mut cases = Vec::new();

    let board_sealed = seal_case(
        "board",
        machine_header("board", Kind::Board, machine),
        &board(),
        machine,
        &[machine, phone, tablet],
    );
    cases.push(opens(
        "board-opened-by-a-device",
        &board_sealed,
        phone,
        machine,
        board(),
    ));
    cases.push(opens(
        "board-opened-by-a-read-device",
        &board_sealed,
        tablet,
        machine,
        board(),
    ));
    cases.push(opens(
        "board-opened-by-its-own-machine",
        &board_sealed,
        machine,
        machine,
        board(),
    ));

    let answer_sealed = seal_case(
        "answer",
        command_header("answer", Kind::Answer, phone, machine),
        &answer(),
        phone,
        &[phone, machine],
    );
    cases.push(opens(
        "answer-opened-by-the-machine",
        &answer_sealed,
        machine,
        phone,
        answer(),
    ));
    cases.push(opens(
        "answer-opened-by-its-own-device",
        &answer_sealed,
        phone,
        phone,
        answer(),
    ));

    let mut notice_header = machine_header("notice", Kind::Notice, machine);
    notice_header.urgency = Some(Urgency::High);
    notice_header.collapse = Some(b64(&seed("notice/collapse")));
    let notice_sealed = seal_case(
        "notice",
        notice_header,
        &notice(None),
        machine,
        &[machine, phone, tablet],
    );
    let mut split = Sealed {
        envelope: notice_sealed.envelope.for_recipient(&phone.id).unwrap(),
        randomness: notice_sealed.randomness.clone(),
    };
    split.randomness.ephemeral.retain(|id, _| id == &phone.id);
    cases.push(opens(
        "notice-split-to-one-device",
        &split,
        phone,
        machine,
        notice(None),
    ));

    let retract = notice(Some(b64(&seed("notice/collapse"))));
    let mut retract_header = machine_header("retract", Kind::Notice, machine);
    retract_header.urgency = Some(Urgency::Normal);
    retract_header.collapse = Some(b64(&seed("notice/collapse")));
    let retract_sealed = seal_case(
        "retract",
        retract_header,
        &retract,
        machine,
        &[machine, phone],
    );
    cases.push(opens(
        "notice-with-retract",
        &retract_sealed,
        phone,
        machine,
        retract,
    ));

    let result_sealed = seal_case(
        "result",
        machine_header("answer", Kind::Result, machine),
        &result(),
        machine,
        &[machine, phone],
    );
    cases.push(opens(
        "result-opened-by-the-device",
        &result_sealed,
        phone,
        machine,
        result(),
    ));
    cases.push(opens(
        "result-opened-by-its-own-machine",
        &result_sealed,
        machine,
        machine,
        result(),
    ));

    let tampered = |name: &'static str, change: &dyn Fn(&mut Envelope)| {
        let mut envelope = answer_sealed.envelope.clone();
        change(&mut envelope);
        fails(
            name,
            envelope,
            machine,
            phone,
            OpenError::BadSignature,
            Some(OpenError::Undecryptable),
        )
    };
    cases.push(tampered("tampered-machine", &|envelope| {
        envelope.header.machine = other_machine.id.clone()
    }));
    cases.push(tampered("tampered-expiry", &|envelope| {
        envelope.header.exp += 3_600
    }));
    cases.push(tampered("tampered-kind", &|envelope| {
        envelope.header.kind = Kind::Push
    }));
    cases.push(tampered("tampered-id", &|envelope| {
        envelope.header.id = command_id("another")
    }));
    cases.push(tampered("tampered-ciphertext", &|envelope| {
        let mut ct = URL_SAFE_NO_PAD.decode(&envelope.ct).unwrap();
        *ct.last_mut().unwrap() ^= 1;
        envelope.ct = b64(&ct);
    }));

    let expired = seal_case(
        "expired",
        stale(command_header("expired", Kind::Answer, phone, machine)),
        &answer(),
        phone,
        &[phone, machine],
    );
    cases.push(fails(
        "expired",
        expired.envelope,
        machine,
        phone,
        OpenError::Expired,
        None,
    ));

    let mut early_header = command_header("not-yet-valid", Kind::Answer, phone, machine);
    early_header.at = NOW + 3_600;
    early_header.exp = NOW + 7_200;
    let early = seal_case(
        "not-yet-valid",
        early_header,
        &answer(),
        phone,
        &[phone, machine],
    );
    cases.push(fails(
        "not-yet-valid",
        early.envelope,
        machine,
        phone,
        OpenError::NotYetValid,
        None,
    ));

    cases.push(fails(
        "wrong-recipient",
        board_sealed.envelope.clone(),
        intruder,
        machine,
        OpenError::NotARecipient,
        Some(OpenError::NotARecipient),
    ));

    let mut moved = board_sealed.envelope.clone();
    let row = moved.keys[&phone.id].clone();
    moved.keys.insert(intruder.id.clone(), row);
    cases.push(fails(
        "key-row-moved-to-another-device",
        moved,
        intruder,
        machine,
        OpenError::Undecryptable,
        Some(OpenError::Undecryptable),
    ));

    let mut emptied = board_sealed.envelope.clone();
    emptied.keys.clear();
    cases.push(fails(
        "empty-keys",
        emptied,
        phone,
        machine,
        OpenError::NotARecipient,
        Some(OpenError::NotARecipient),
    ));

    let mut swapped = split.envelope.clone();
    let garbage = wrap_key(
        &seed("hub/garbage-key"),
        &phone.public().encryption,
        &swapped.header.bytes(),
        seed("hub/ephemeral"),
    );
    swapped.keys.insert(phone.id.clone(), b64(&garbage));
    cases.push(fails(
        "row-with-another-content-key",
        swapped,
        phone,
        machine,
        OpenError::KeyMismatch,
        Some(OpenError::KeyMismatch),
    ));

    let forged = seal_case(
        "forged",
        command_header("forged", Kind::Answer, phone, machine),
        &answer(),
        intruder,
        &[machine],
    );
    cases.push(fails(
        "signed-by-another-key",
        forged.envelope,
        machine,
        phone,
        OpenError::BadSignature,
        None,
    ));
    cases.push(fails(
        "sender-is-not-who-the-header-says",
        answer_sealed.envelope.clone(),
        machine,
        tablet,
        OpenError::WrongSender,
        None,
    ));

    let mut future = answer_sealed.envelope.clone();
    future.header.v = 2;
    cases.push(fails(
        "unsupported-version",
        future,
        machine,
        phone,
        OpenError::UnsupportedVersion,
        Some(OpenError::Undecryptable),
    ));

    let answer_bytes = serde_json::to_vec(&answer()).unwrap();
    let board_bytes = serde_json::to_vec(&board()).unwrap();
    let mut long_lived = command_header("over-long-ttl", Kind::Answer, phone, machine);
    long_lived.exp = long_lived.at + Kind::Answer.max_ttl() + 1;
    cases.push(fails(
        "over-long-ttl",
        seal_bypassing_checks(
            "over-long-ttl",
            long_lived,
            &answer_bytes,
            phone,
            &[phone, machine],
        ),
        machine,
        phone,
        OpenError::Malformed,
        None,
    ));
    let device_board = Header::new(
        time_id("device-board", Prefix::Board),
        Kind::Board,
        phone.id.clone(),
        machine.id.clone(),
        AT,
    );
    cases.push(fails(
        "device-as-board-sender",
        seal_bypassing_checks(
            "device-board",
            device_board,
            &board_bytes,
            phone,
            &[phone, machine],
        ),
        machine,
        phone,
        OpenError::Malformed,
        None,
    ));
    let command_board = Header::new(
        command_id("command-board"),
        Kind::Board,
        machine.id.clone(),
        machine.id.clone(),
        AT,
    );
    cases.push(fails(
        "command-id-on-a-board",
        seal_bypassing_checks(
            "command-board",
            command_board,
            &board_bytes,
            machine,
            &[machine, phone],
        ),
        phone,
        machine,
        OpenError::Malformed,
        None,
    ));
    let mut urgent_answer = command_header("urgent-answer", Kind::Answer, phone, machine);
    urgent_answer.urgency = Some(Urgency::High);
    cases.push(fails(
        "urgency-on-an-answer",
        seal_bypassing_checks(
            "urgent-answer",
            urgent_answer,
            &answer_bytes,
            phone,
            &[phone, machine],
        ),
        machine,
        phone,
        OpenError::Malformed,
        None,
    ));
    let mut long_collapse = machine_header("long-collapse", Kind::Notice, machine);
    long_collapse.collapse = Some("a".repeat(65));
    let notice_bytes = serde_json::to_vec(&notice(None)).unwrap();
    cases.push(fails(
        "collapse-of-65-chars",
        seal_bypassing_checks(
            "long-collapse",
            long_collapse,
            &notice_bytes,
            machine,
            &[machine, phone],
        ),
        phone,
        machine,
        OpenError::Malformed,
        None,
    ));
    let push_bytes = serde_json::to_vec(&push(HEAD)).unwrap();
    let mismatched = seal_bypassing_checks(
        "kind-mismatch",
        command_header("kind-mismatch", Kind::Answer, phone, machine),
        &push_bytes,
        phone,
        &[phone, machine],
    );
    cases.push(fails(
        "payload-type-differs-from-kind",
        mismatched,
        machine,
        phone,
        OpenError::KindMismatch,
        Some(OpenError::KindMismatch),
    ));

    let mut leaked_board = serde_json::to_value(board()).unwrap();
    leaked_board["items"][0]["path"] = json!("/Users/someone/projects/greenhouse");
    let mut extra_result = serde_json::to_value(result()).unwrap();
    extra_result["detail"] = json!("seen by the hub");
    let mut reasonless = serde_json::to_value(result()).unwrap();
    reasonless.as_object_mut().unwrap().remove("reason");
    let mut string_rev = serde_json::to_value(notice(None)).unwrap();
    string_rev["board_rev"] = json!("1842");
    let strays: [(&'static str, &str, Kind, Value); 4] = [
        (
            "board-item-with-a-path",
            "stray/board",
            Kind::Board,
            leaked_board,
        ),
        (
            "result-with-an-extra-field",
            "stray/result",
            Kind::Result,
            extra_result,
        ),
        (
            "refused-result-without-a-reason",
            "stray/reasonless",
            Kind::Result,
            reasonless,
        ),
        (
            "notice-with-a-string-board-rev",
            "stray/notice",
            Kind::Notice,
            string_rev,
        ),
    ];
    for (name, label, kind, payload) in strays {
        let envelope = seal_bypassing_checks(
            label,
            machine_header(label, kind, machine),
            &serde_json::to_vec(&payload).unwrap(),
            machine,
            &[machine, phone],
        );
        cases.push(fails(
            name,
            envelope,
            phone,
            machine,
            OpenError::Malformed,
            Some(OpenError::Malformed),
        ));
    }

    for case in &cases {
        let opened = open(
            &case.envelope,
            &case.recipient.id,
            &case.recipient.keys,
            &case.sender.id,
            &case.sender.public(),
            NOW,
        );
        assert_eq!(opened, case.expect, "{}", case.name);
        if let Some(expected) = case.decrypt_expect {
            assert_eq!(
                decrypt(&case.envelope, &case.recipient.id, &case.recipient.keys).err(),
                Some(expected),
                "{}",
                case.name
            );
        }
    }
    cases
}

fn seal_json(randomness: &Randomness, payload: &Payload) -> Value {
    let ephemeral: BTreeMap<String, String> = randomness
        .ephemeral
        .iter()
        .map(|(id, ikm)| (id.to_string(), b64(ikm)))
        .collect();
    json!({
        "content_key": b64(&randomness.content_key),
        "nonce": b64(&randomness.nonce),
        "ephemeral": ephemeral,
        "plaintext": b64(&serde_json::to_vec(payload).unwrap()),
    })
}

fn envelopes_json(parties: &Parties) -> Value {
    let cases: Vec<Value> = envelope_cases(parties)
        .into_iter()
        .map(|case| {
            let (expect, seal) = match &case.expect {
                Ok(payload) => (json!({ "payload": payload }), case.randomness.as_ref().map(|randomness| seal_json(randomness, payload))),
                Err(error) => (json!({ "error": error.key(), "decrypt_error": case.decrypt_expect.map(OpenError::key) }), None),
            };
            json!({
                "name": case.name,
                "recipient": case.recipient.name,
                "sender": case.sender.name,
                "envelope": case.envelope,
                "header_bytes": b64(&case.envelope.header.bytes()),
                "seal": seal,
                "expect": expect,
            })
        })
        .collect();
    let max_ttl: BTreeMap<&str, u64> = [
        Kind::Board,
        Kind::Notice,
        Kind::Result,
        Kind::Answer,
        Kind::Push,
        Kind::Start,
        Kind::Routine,
    ]
    .into_iter()
    .map(|kind| (kind.name(), kind.max_ttl()))
    .collect();
    json!({ "now": NOW, "clock_skew": crate::CLOCK_SKEW, "max_ttl": max_ttl, "hpke_info": String::from_utf8_lossy(crate::HPKE_INFO), "cases": cases })
}

struct AuthorizeCase {
    name: &'static str,
    accepts: Vec<Kind>,
    applied: AppliedCommands,
    envelope: Envelope,
    expect: Result<Payload, Refusal>,
}

fn authorize_json(parties: &Parties) -> Value {
    let Parties {
        machine,
        phone,
        tablet,
        intruder,
        other_machine,
    } = parties;
    let trusted = parties.trusted();
    let all = vec![Kind::Answer, Kind::Push, Kind::Start, Kind::Routine];
    let start = |prompt: String| {
        Payload::Start(StartTask {
            repo: "greenhouse".into(),
            name: "invoice-rounding".into(),
            prompt,
        })
    };
    let from_phone = |case: &str, kind: Kind, payload: &Payload| {
        seal_case(
            case,
            command_header(case, kind, phone, machine),
            payload,
            phone,
            &[phone, machine],
        )
        .envelope
    };
    let case = |name, envelope, expect| AuthorizeCase {
        name,
        accepts: all.clone(),
        applied: AppliedCommands::default(),
        envelope,
        expect,
    };

    let answered = from_phone("authorize/answer", Kind::Answer, &answer());
    let mut applied = AppliedCommands::default();
    applied.record(
        &phone.id,
        NOW - 60,
        CommandResult {
            command: answered.header.id.clone(),
            outcome: Outcome::Done,
            text: "Answered refunds@greenhouse's question.".into(),
        },
    );
    let routine = Payload::Routine(RunRoutine {
        id: "nightly-deps".into(),
    });
    let mut long_lived = command_header("authorize/over-long-ttl", Kind::Answer, phone, machine);
    long_lived.exp = long_lived.at + Kind::Answer.max_ttl() + 1;
    let long_lived = seal_bypassing_checks(
        "authorize/over-long-ttl",
        long_lived,
        &serde_json::to_vec(&answer()).unwrap(),
        phone,
        &[phone, machine],
    );
    let mut future = answered.clone();
    future.header.v = 2;
    let forged_replay = seal_case(
        "authorize/forged-replay",
        command_header("authorize/answer", Kind::Answer, phone, machine),
        &answer(),
        intruder,
        &[machine],
    )
    .envelope;
    let expired_answer = seal_case(
        "authorize/expired-replay",
        stale(command_header(
            "authorize/expired-replay",
            Kind::Answer,
            phone,
            machine,
        )),
        &answer(),
        phone,
        &[phone, machine],
    )
    .envelope;
    let mut expired_applied = AppliedCommands::default();
    expired_applied.record(
        &phone.id,
        NOW - 4_000,
        CommandResult {
            command: expired_answer.header.id.clone(),
            outcome: Outcome::Done,
            text: "Answered refunds@greenhouse's question.".into(),
        },
    );
    let expired_result = expired_applied
        .result(&phone.id, &expired_answer.header.id)
        .unwrap()
        .clone();

    let cases = vec![
        case("control-device-answers", answered.clone(), Ok(answer())),
        AuthorizeCase {
            name: "same-command-twice-replays-the-stored-result",
            accepts: all.clone(),
            applied: applied.clone(),
            envelope: answered.clone(),
            expect: Err(Refusal::Replay(
                applied
                    .result(&phone.id, &answered.header.id)
                    .unwrap()
                    .clone(),
            )),
        },
        AuthorizeCase {
            name: "forged-signature-with-an-applied-id-is-dropped",
            accepts: all.clone(),
            applied: applied.clone(),
            envelope: forged_replay,
            expect: Err(Refusal::Drop(crate::Dropped::BadSignature)),
        },
        AuthorizeCase {
            name: "applied-id-that-has-expired-replays",
            accepts: all.clone(),
            applied: expired_applied,
            envelope: expired_answer,
            expect: Err(Refusal::Replay(expired_result)),
        },
        case(
            "control-device-approves-a-push",
            from_phone("authorize/push", Kind::Push, &push(HEAD)),
            Ok(push(HEAD)),
        ),
        case(
            "control-device-runs-a-routine",
            from_phone("authorize/routine", Kind::Routine, &routine),
            Ok(routine.clone()),
        ),
        case(
            "read-device-is-refused",
            seal_case(
                "authorize/read",
                command_header("authorize/read", Kind::Answer, tablet, machine),
                &answer(),
                tablet,
                &[tablet, machine],
            )
            .envelope,
            Err(Refusal::Answer(Reason::NotAllowed)),
        ),
        case(
            "expired-command-is-refused",
            seal_case(
                "authorize/expired",
                stale(command_header(
                    "authorize/expired",
                    Kind::Answer,
                    phone,
                    machine,
                )),
                &answer(),
                phone,
                &[phone, machine],
            )
            .envelope,
            Err(Refusal::Answer(Reason::Expired)),
        ),
        AuthorizeCase {
            name: "kind-the-machine-does-not-take-is-refused",
            accepts: vec![Kind::Answer],
            applied: AppliedCommands::default(),
            envelope: from_phone(
                "authorize/start",
                Kind::Start,
                &start("Fix the rounding in invoices.".into()),
            ),
            expect: Err(Refusal::Answer(Reason::NotAllowed)),
        },
        case(
            "over-long-prompt-is-refused",
            from_phone(
                "authorize/long",
                Kind::Start,
                &start("x".repeat(crate::MAX_PROMPT_CHARS + 1)),
            ),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        case(
            "address-that-is-not-task-at-repo-is-refused",
            from_phone("authorize/address", Kind::Answer, &answer_to("refunds")),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        case(
            "push-with-a-short-sha-is-refused",
            from_phone("authorize/short-sha", Kind::Push, &push("9f3c2a1")),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        case(
            "start-in-a-path-is-refused",
            from_phone(
                "authorize/path",
                Kind::Start,
                &Payload::Start(StartTask {
                    repo: "../etc".into(),
                    name: "x".into(),
                    prompt: "x".into(),
                }),
            ),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        case(
            "routine-id-with-spaces-is-refused",
            from_phone(
                "authorize/routine-id",
                Kind::Routine,
                &Payload::Routine(RunRoutine {
                    id: "nightly deps".into(),
                }),
            ),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        case(
            "command-not-sealed-to-the-machine-is-refused",
            seal_case(
                "authorize/unsealed",
                command_header("authorize/unsealed", Kind::Answer, phone, machine),
                &answer(),
                phone,
                &[phone],
            )
            .envelope,
            Err(Refusal::Answer(Reason::Undecryptable)),
        ),
        case(
            "unknown-device-is-dropped",
            seal_case(
                "authorize/unknown",
                command_header("authorize/unknown", Kind::Answer, intruder, machine),
                &answer(),
                intruder,
                &[intruder, machine],
            )
            .envelope,
            Err(Refusal::Drop(crate::Dropped::UnknownDevice)),
        ),
        case(
            "forged-signature-is-dropped",
            seal_case(
                "authorize/forged",
                command_header("authorize/forged", Kind::Answer, phone, machine),
                &answer(),
                intruder,
                &[machine],
            )
            .envelope,
            Err(Refusal::Drop(crate::Dropped::BadSignature)),
        ),
        case(
            "command-for-another-machine-is-dropped",
            seal_case(
                "authorize/other",
                command_header("authorize/other", Kind::Answer, phone, other_machine),
                &answer(),
                phone,
                &[phone, other_machine],
            )
            .envelope,
            Err(Refusal::Drop(crate::Dropped::WrongMachine)),
        ),
        case(
            "board-is-not-a-command",
            seal_case(
                "authorize/board",
                machine_header("authorize/board", Kind::Board, machine),
                &board(),
                machine,
                &[machine],
            )
            .envelope,
            Err(Refusal::Drop(crate::Dropped::NotACommand)),
        ),
        case(
            "over-long-ttl-is-refused",
            long_lived,
            Err(Refusal::Drop(crate::Dropped::Malformed)),
        ),
        case(
            "unsupported-version-is-dropped",
            future,
            Err(Refusal::Drop(crate::Dropped::UnsupportedVersion)),
        ),
    ];
    let rendered: Vec<Value> = cases
        .into_iter()
        .map(|case| {
            let gate = Machine { id: &machine.id, keys: &machine.keys, devices: &trusted, accepts: &case.accepts };
            let decided = authorize(&gate, &case.applied, &case.envelope, NOW).map(|accepted| (accepted.device.id.clone(), accepted.payload));
            let expected = case.expect.map(|payload| (case.envelope.header.from.clone(), payload));
            assert_eq!(decided, expected, "{}", case.name);
            let expect = match expected {
                Ok((device, payload)) => json!({ "accepted": device, "payload": payload }),
                Err(Refusal::Answer(reason)) => json!({ "answer": reason.key() }),
                Err(Refusal::Drop(dropped)) => json!({ "drop": dropped.key() }),
                Err(Refusal::Replay(result)) => json!({ "replay": result }),
            };
            json!({ "name": case.name, "accepts": case.accepts, "applied": case.applied, "envelope": case.envelope, "expect": expect })
        })
        .collect();
    json!({ "now": NOW, "machine": machine.name, "trusted": trusted, "cases": rendered })
}

fn signatures_json(parties: &Parties) -> Value {
    let cases: Vec<Value> = [&parties.machine, &parties.phone]
        .into_iter()
        .map(|party| {
            let message = format!("kelpie-hub signature vector for {}", party.name);
            let signature = SigningKey::from_bytes(&party.keys.to_bytes().0).sign(message.as_bytes());
            json!({ "signer": party.name, "message": b64(message.as_bytes()), "signature": b64(&signature.to_bytes()) })
        })
        .collect();
    json!({ "cases": cases })
}

struct PairingCase<'a> {
    name: &'static str,
    machine: &'a Party,
    device: &'a Party,
    revealed: &'a Party,
    code: &'static str,
    machine_code: &'static str,
    other_nonce: bool,
}

fn pairing_json(parties: &Parties) -> Value {
    let Parties {
        machine,
        phone,
        tablet,
        intruder,
        other_machine,
    } = parties;
    let flow = |name, machine, device, code| PairingCase {
        name,
        machine,
        device,
        revealed: device,
        code,
        machine_code: code,
        other_nonce: false,
    };
    let flows = [
        flow("phone-pairs", machine, phone, "K7QF-M2XD"),
        flow("tablet-pairs", machine, tablet, "K7QF-M2XD"),
        flow(
            "pairs-with-another-machine",
            other_machine,
            phone,
            "Q2ZR-8HWN",
        ),
        PairingCase {
            other_nonce: true,
            ..flow(
                "reveal-with-another-nonce-is-refused",
                machine,
                phone,
                "K7QF-M2XD",
            )
        },
        PairingCase {
            revealed: intruder,
            ..flow(
                "reveal-of-substituted-keys-is-refused",
                machine,
                phone,
                "K7QF-M2XD",
            )
        },
        PairingCase {
            machine_code: "Q2ZR-8HWN",
            ..flow(
                "machine-with-another-code-is-refused",
                machine,
                phone,
                "K7QF-M2XD",
            )
        },
    ];
    let cases: Vec<Value> = flows
        .into_iter()
        .map(|case| {
            let name = case.name;
            let device_nonce = seed(&format!("pairing/{name}/device-nonce"));
            let device_side =
                DevicePairing::with_nonce(case.device.public(), device_nonce, case.code);
            let commitment = device_side.commitment();
            let machine_side = MachinePairing::with_nonce(
                case.machine.public(),
                commitment,
                seed(&format!("pairing/{name}/machine-nonce")),
                case.machine_code,
            );
            let (_, machine_nonce) = machine_side.reply();
            let revealed_nonce = if case.other_nonce {
                seed(&format!("pairing/{name}/other-nonce"))
            } else {
                device_nonce
            };
            let expect =
                match machine_side.comparison_code(&case.revealed.public(), &revealed_nonce) {
                    Ok(sas) => {
                        assert_eq!(
                            device_side.comparison_code(&case.machine.public(), &machine_nonce),
                            sas,
                            "{name}"
                        );
                        json!({ "sas": sas })
                    }
                    Err(_) => json!({ "error": "commitment-mismatch" }),
                };
            json!({
                "name": name,
                "machine": case.machine.name,
                "device": case.device.name,
                "code": case.code,
                "machine_code": case.machine_code,
                "device_nonce": b64(&device_nonce),
                "commitment": b64(&commitment),
                "machine_nonce": b64(&machine_nonce),
                "revealed": { "device": case.revealed.name, "nonce": b64(&revealed_nonce) },
                "expect": expect,
            })
        })
        .collect();
    json!({ "commit_domain": "kelpie-pair-commit-v1", "sas_domain": "kelpie-pair-v1", "cases": cases })
}

fn keys_json(parties: &Parties) -> Value {
    let all: BTreeMap<&str, Value> = parties
        .all()
        .into_iter()
        .map(|party| (party.name, party.to_json()))
        .collect();
    json!({ "parties": all })
}

pub fn render() -> Vec<(&'static str, String)> {
    let parties = Parties::new();
    [
        ("keys.json", keys_json(&parties)),
        ("envelopes.json", envelopes_json(&parties)),
        ("authorize.json", authorize_json(&parties)),
        ("signatures.json", signatures_json(&parties)),
        ("pairing.json", pairing_json(&parties)),
    ]
    .into_iter()
    .map(|(name, value)| (name, serde_json::to_string_pretty(&value).unwrap() + "\n"))
    .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn vectors_on_disk_are_what_the_generator_writes() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vectors");
        for (name, text) in super::render() {
            let on_disk = std::fs::read_to_string(dir.join(name)).unwrap_or_default();
            assert!(
                on_disk == text,
                "vectors/{name} is stale: cargo run -p kelpie-hub-protocol --features test-vectors --bin vectors"
            );
        }
    }
}
