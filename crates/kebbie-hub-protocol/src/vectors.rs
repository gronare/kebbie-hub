use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::account::{
    ACCOUNT_COMMIT_DOMAIN, AccountHandoff, AccountKey, ENCRYPTION_FINGERPRINT_DOMAIN,
    HUB_PRF_SALT_DOMAIN, MACHINE_LIST_DOMAIN, MachineList, SIGNING_FINGERPRINT_DOMAIN, Unlock,
    WRAP_AD_DOMAIN, WRAP_KEY_INFO, WRAP_NONCE_LEN, WRAPPER_SIGN_DOMAIN, hub_prf_salt, pin_account,
    sign_wrapper, wrap_associated_data, wrap_key as account_wrap_key, wrapper_statement,
};
use crate::authorize::{AppliedCommands, Verified, Machine, Refusal, authorize};
use crate::backlog::{
    BACKLOG_BODY_MEDIA_TYPE, BacklogBody, BacklogRow, BacklogState, BodySection, Home, HomeKind, ItemKey,
    OpenQuestion, Priority,
};
use crate::blob::{BLOB_DOMAIN, seal_blob_with};
use crate::board::{
    Agent, BlobRef, AgentStatus, Board, Checkout, Counts, DeviceStanding, Git, Group, Item, Later, LaterState, NextPart,
    NextRow, Prompt, PromptOption, Question, QuestionOption, Report, RoutineSummary,
    SecondOpinion, Step, StepState, Steps, Verdict, VerdictOutcome,
};
use crate::connect::{CONNECT_FINGERPRINT_DOMAIN, ConnectSecret, machine_fingerprint};
use crate::crypto::{
    OpenError, PublicKeys, Randomness, SecretKeys, decrypt, key_commitment, open, seal_unchecked,
    seal_with, wrap_key,
};
use crate::device::{
    DEVICE_CERT_DOMAIN, DEVICE_FINGERPRINT_DOMAIN, DeviceCert, DeviceKey, DeviceKind, DeviceTrust,
    Trust, VerifiedDevice, device_fingerprint,
};
use crate::envelope::{Envelope, Header, Kind, Urgency};
use crate::ids::{Id, Prefix};
use crate::review::{
    AcceptStep, DiffLine, FileStatus, GuideNote, Hunk, LineKind, MAX_REVIEW_COMMENT_CHARS, MAX_REVIEW_COMMENTS,
    MAX_REVIEW_COMMENTS_BYTES, REVIEW_MEDIA_TYPE, Review, ReviewComment, ReviewComments, ReviewFile, ReviewOffer,
    ReviewOpen, ReviewRead, ReviewStep, Side,
};
use crate::message::{
    Answer, CommandResult, DeviceDecision, Idea, ItemAnswer, ItemEdit, Notice, NoticeCategory, Outcome, Payload,
    PromptAnswer, PushApproval, Reason, RunRoutine, StartItem, StartTask, Target,
};

pub const NOW: u64 = 1_761_070_000;
const AT: u64 = 1_761_069_600;
const HEAD: &str = "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a";
const QUESTION: &str = "1761069540-4411-0-question";
const CERTIFIED_AT: u64 = AT - 86_400;

struct Party {
    name: &'static str,
    id: Id,
    keys: SecretKeys,
}

impl Party {
    fn new(name: &'static str, prefix: Prefix) -> Party {
        let id = if prefix == Prefix::Account {
            Id::account(seed(&format!("{name}/id"))[..16].try_into().unwrap())
        } else {
            Id::ulid(
                prefix,
                AT * 1_000,
                seed(&format!("{name}/id"))[..10].try_into().unwrap(),
            )
            .unwrap()
        };
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

    fn signing(&self) -> SigningKey {
        SigningKey::from_bytes(&self.keys.to_bytes().0)
    }

    fn device_key(&self) -> DeviceKey {
        let (signing, encryption) = self.keys.to_bytes();
        DeviceKey::from_bytes(signing, encryption)
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
    account: Party,
    other_account: Party,
}

impl Parties {
    fn new() -> Parties {
        Parties {
            machine: Party::new("machine", Prefix::Machine),
            other_machine: Party::new("other-machine", Prefix::Machine),
            phone: Party::new("phone", Prefix::Device),
            tablet: Party::new("tablet", Prefix::Device),
            intruder: Party::new("intruder", Prefix::Device),
            account: Party::new("account", Prefix::Account),
            other_account: Party::new("other-account", Prefix::Account),
        }
    }

    fn all(&self) -> [&Party; 7] {
        [
            &self.machine,
            &self.other_machine,
            &self.phone,
            &self.tablet,
            &self.intruder,
            &self.account,
            &self.other_account,
        ]
    }

    fn account_key(&self) -> AccountKey {
        AccountKey::new(self.account.id.clone(), 1, self.account.public()).unwrap()
    }

    fn cert(&self, device: &Party, name: &str) -> DeviceCert {
        certify(&self.account, &self.account, device, name)
    }
}

fn certify(signer: &Party, account: &Party, device: &Party, name: &str) -> DeviceCert {
    DeviceCert::issue(
        &signer.keys,
        &account.id,
        device.id.clone(),
        name,
        DeviceKind::Web,
        device.public(),
        CERTIFIED_AT,
    )
    .unwrap()
}

fn seed(label: &str) -> [u8; 32] {
    Sha256::digest(format!("kebbie-hub test vector {label}")).into()
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
        kebbie: "0.24.0".into(),
        away_since: None,
        checkouts: vec![Checkout {
            repo: "shop".into(),
            free_worktrees: 2,
            busy_worktrees: 4,
        }],
        routines: vec![RoutineSummary {
            id: "nightly-deps".into(),
            title: "Nightly deps".into(),
            repo: "shop".into(),
            mode: "fix".into(),
            next_run: Some(1_761_100_200),
            last_outcome: Some("ok".into()),
        }],
        items: vec![Item {
            address: "refunds@shop".into(),
            group: Group::NeedsYou,
            urgency: 1,
            summary: "2 commits · 1 behind main".into(),
            next_for_you: Some("answer: Keep the old endpoint for a release?".into()),
            agent: Some(Agent {
                status: AgentStatus::Idle,
                kind: None,
                model: None,
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
                        comment: false,
                    },
                    QuestionOption {
                        label: "Remove it".into(),
                        meaning: "drop it in this task".into(),
                        comment: true,
                    },
                ],
                recommend: Some(1),
                kept_for_user: false,
                plan: None,
                plan_at: None,
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
            steps: None,
            title: None,
            profile: None,
            recipe: None,
            assessment: None,
            second_opinion: None,
            review: None,
        }],
        sessions_outside_tasks: 1,
        next: None,
        counts: None,
        later: None,
        accepts: None,
        devices: None,
        backlog: None,
    })
}

fn stepped_board() -> Payload {
    let Payload::Board(mut board) = board() else {
        unreachable!()
    };
    let step = |number: u32, title: &str, state: StepState, sha: Option<&str>| Step {
        number,
        title: title.into(),
        state,
        sha: sha.map(Into::into),
    };
    board.items = vec![Item {
        address: "payments@api".into(),
        group: Group::Working,
        urgency: 0,
        summary: "2 commits".into(),
        next_for_you: None,
        agent: Some(Agent {
            status: AgentStatus::Working,
            kind: None,
            model: None,
        }),
        report: None,
        question: None,
        prompt: None,
        git: Some(Git {
            upstream: "main".into(),
            ahead: 2,
            behind: 0,
            uncommitted: 0,
            approved: true,
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
        steps: Some(Steps {
            pushed: 1,
            items: vec![
                step(2, "Charge through a PaymentGateway", StepState::Committed, Some(HEAD)),
                step(3, "Add the new gateway", StepState::InProgress, None),
                step(4, "Remove the old payment client", StepState::Planned, None),
            ],
            required: None,
        }),
        title: None,
        profile: None,
        recipe: None,
        assessment: None,
        second_opinion: None,
        review: None,
    }];
    Payload::Board(board)
}

fn next_board() -> Payload {
    let Payload::Board(mut board) = stepped_board() else {
        unreachable!()
    };
    board.away_since = Some(1_761_060_000);
    let stepped = &mut board.items[0];
    stepped.title = Some("Charge through the gateway".into());
    stepped.profile = Some("task".into());
    stepped.recipe = Some("small fix".into());
    stepped.assessment = Some("review suggested: touches money and its tests".into());
    stepped.agent = Some(Agent {
        status: AgentStatus::Working,
        kind: Some("claude".into()),
        model: Some("opus".into()),
    });
    stepped.verdict = Some(Verdict {
        verdict: Some(VerdictOutcome::Ok),
        findings: Some(0),
        sha: Some(HEAD.into()),
        stale: false,
        running: false,
        held: Some(true),
    });
    stepped.second_opinion = Some(SecondOpinion {
        by: "codex-review".into(),
        points: Some(2),
        running: false,
    });
    if let Some(steps) = stepped.steps.as_mut() {
        steps.required = Some(false);
    }
    let mut prompted = stepped.clone();
    prompted.address = "hub@kebbie".into();
    prompted.group = Group::NeedsYou;
    prompted.title = Some("Clarify machine removal".into());
    prompted.agent = Some(Agent {
        status: AgentStatus::Blocked,
        kind: Some("codex".into()),
        model: None,
    });
    prompted.next_for_you = Some("answer Codex's prompt in its Space or through the lead".into());
    prompted.prompt = Some(Prompt {
        id: "3f2a9c1d".into(),
        question: "Would you like to run the following command?\nReason: May I record that this task is waiting on the lead?\n$ kebbie report --state blocked".into(),
        options: vec![
            PromptOption {
                label: "Yes, proceed (y)".into(),
                takes_text: false,
                key: Some("y".into()),
            },
            PromptOption {
                label: "No, and tell Codex what to do differently (esc)".into(),
                takes_text: false,
                key: Some("esc".into()),
            },
        ],
    });
    prompted.steps = None;
    prompted.verdict = None;
    prompted.second_opinion = None;
    board.items.insert(0, prompted);
    board.next = Some(vec![
        NextRow {
            part: NextPart::Stopped,
            address: "hub@kebbie".into(),
            rank: 1,
            line: "answer Codex's prompt".into(),
            since: Some(1_761_069_000),
            waits_behind: 0,
        },
        NextRow {
            part: NextPart::Push,
            address: "payments@api".into(),
            rank: 2,
            line: "2 commits · mail-dns waits on it".into(),
            since: Some(1_761_066_000),
            waits_behind: 1,
        },
    ]);
    board.counts = Some(Counts {
        working: 1,
        queued: 1,
        parked: 1,
    });
    board.later = Some(vec![
        Later {
            address: "mail-dns@shop".into(),
            title: Some("Mail DNS records".into()),
            state: LaterState::Queued,
            line: "waits for payments@api to push".into(),
            until: None,
            question: false,
        },
        Later {
            address: "editor@app".into(),
            title: None,
            state: LaterState::Parked,
            line: "nothing yet".into(),
            until: Some(1_761_100_000),
            question: true,
        },
    ]);
    board.accepts = Some(vec![Kind::Answer, Kind::Push, Kind::PromptAnswer]);
    let standing = |name: &'static str| {
        let party = Party::new(name, Prefix::Device);
        (party.id.clone(), device_fingerprint(&party.public().signing))
    };
    let ((phone, phone_print), (tablet, tablet_print), (intruder, intruder_print)) =
        (standing("phone"), standing("tablet"), standing("intruder"));
    board.devices = Some(BTreeMap::from([
        (phone, DeviceStanding::Approved { fingerprint: phone_print, at: AT - 3_600 }),
        (tablet, DeviceStanding::Waiting { fingerprint: tablet_print, until: Some(AT + 3_600) }),
        (intruder, DeviceStanding::Denied { fingerprint: intruder_print, at: AT - 60 }),
    ]));
    Payload::Board(board)
}

const VAULT_ITEM: &str = "Projects/refund-rounding.md";
const BLOCKING_QUESTION: &str = "Round half up, or to even like the ledger?";

fn section(key: &str, heading: &str, text: &str, required: bool) -> BodySection {
    BodySection { key: key.into(), heading: heading.into(), text: text.into(), required }
}

const GOAL: &str = "A refund of a whole invoice returns exactly what the invoice charged.\nPartial refunds keep rounding per line.";

fn backlog_body() -> BacklogBody {
    BacklogBody {
        sections: vec![
            section("background", "Background", "Refunds are rounded per line, invoices on the total, so a refund can differ from its invoice by a cent.", true),
            section("goal", "Goal", GOAL, true),
            section("what-we-know", "What we know", "- Invoices round the total since 2.12.\n- `Refund#amount` sums rounded lines.", false),
            section("questions", "Open questions", "", false),
            section("out-of-scope", "Out of scope", "Currency conversion.", false),
            section("source", "Source", "Support thread about a refund one cent short.", false),
        ],
        questions: vec![
            OpenQuestion {
                text: BLOCKING_QUESTION.into(),
                blocks: true,
                pending_answer: None,
            },
            OpenQuestion {
                text: "Should old refunds be recomputed?".into(),
                blocks: true,
                pending_answer: Some("No, only new ones.".into()),
            },
            OpenQuestion {
                text: "Show the rounding on the receipt?".into(),
                blocks: false,
                pending_answer: None,
            },
        ],
        questions_key: Some("questions".into()),
    }
}

fn backlog_blob() -> (BlobRef, Vec<u8>) {
    seal_blob_with(
        &serde_json::to_vec(&backlog_body()).unwrap(),
        BACKLOG_BODY_MEDIA_TYPE,
        seed("backlog/body/key"),
        seed("backlog/body/nonce")[..12].try_into().unwrap(),
    )
}

fn backlog_kinds() -> Vec<Kind> {
    vec![Kind::Answer, Kind::Push, Kind::PromptAnswer, Kind::Idea, Kind::ItemAnswer, Kind::StartItem, Kind::ItemEdit]
}

fn backlog_board() -> Payload {
    let Payload::Board(mut board) = next_board() else {
        unreachable!()
    };
    board.accepts = Some(backlog_kinds());
    board.backlog = Some(vec![
        BacklogRow {
            item: ItemKey::vault(VAULT_ITEM).unwrap(),
            title: "Refunds round like their invoice".into(),
            priority: Priority::High,
            repo: Some("shop".into()),
            state: BacklogState::Ready,
            filed: AT - 3 * 86_400,
            home: Home {
                kind: HomeKind::Vault,
                label: VAULT_ITEM.into(),
            },
            questions: 3,
            blocking: 1,
            goal: Some("A refund of a whole invoice returns exactly what the invoice charged.".into()),
            body: Some(backlog_blob().0),
        },
        BacklogRow {
            item: ItemKey::parse("local:01JB7Q2M3N8K").unwrap(),
            title: "Dark mode for the editor".into(),
            priority: Priority::Low,
            repo: Some("app".into()),
            state: BacklogState::Drafting,
            filed: AT - 86_400,
            home: Home {
                kind: HomeKind::Local,
                label: ".kebbie/backlog/dark-mode.md".into(),
            },
            questions: 0,
            blocking: 0,
            goal: None,
            body: None,
        },
        BacklogRow {
            item: ItemKey::parse("kept:idea-4411").unwrap(),
            title: "Mail the weekly summary".into(),
            priority: Priority::None,
            repo: None,
            state: BacklogState::ToWrite,
            filed: AT - 600,
            home: Home {
                kind: HomeKind::Kept,
                label: "kept by kebbie".into(),
            },
            questions: 0,
            blocking: 0,
            goal: None,
            body: None,
        },
    ]);
    Payload::Board(board)
}

fn idea(text: String) -> Payload {
    Payload::Idea(Idea {
        text,
        repo: Some("shop".into()),
    })
}

fn item_answer(answer: &str) -> Payload {
    Payload::ItemAnswer(ItemAnswer {
        item: ItemKey::vault(VAULT_ITEM).unwrap(),
        index: 0,
        question: BLOCKING_QUESTION.into(),
        answer: answer.into(),
    })
}

const EDITED_GOAL: &str = "A refund of a whole invoice returns exactly what the invoice charged.\nPartial refunds round per line, as today.";

fn item_edit(section: Option<&str>, text: &str, base: &str) -> Payload {
    Payload::ItemEdit(ItemEdit {
        item: ItemKey::vault(VAULT_ITEM).unwrap(),
        section: section.map(Into::into),
        text: text.into(),
        base: crate::text_hash(base),
    })
}

fn goal_edit() -> Payload {
    item_edit(Some("goal"), EDITED_GOAL, GOAL)
}

fn title_edit() -> Payload {
    item_edit(None, "Refunds round exactly like their invoice", "Refunds round like their invoice")
}

fn section_changed() -> Payload {
    Payload::Result(CommandResult {
        command: command_id("item-edit"),
        device: Party::new("phone", Prefix::Device).id,
        outcome: Outcome::Refused { reason: Reason::SectionChanged },
        text: "Not saved: the goal changed on the machine since you opened it.".into(),
        blob: None,
    })
}

fn start_item(anyway: bool) -> Payload {
    Payload::StartItem(StartItem {
        item: ItemKey::vault(VAULT_ITEM).unwrap(),
        anyway,
    })
}

fn questions_open() -> Payload {
    Payload::Result(CommandResult {
        command: command_id("start-item"),
        device: Party::new("phone", Prefix::Device).id,
        outcome: Outcome::Refused {
            reason: Reason::QuestionsOpen { blocking: 2 },
        },
        text: "Not started: 2 questions wait on you.".into(),
        blob: None,
    })
}

const REVIEWED: &str = "payments@api";
const GATEWAY_CONTENT: &str = "5d1c0a9e8f7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d";

fn review() -> Review {
    let line = |kind: LineKind, old: Option<u32>, new: Option<u32>, text: &str| DiffLine { kind, old, new, text: text.into() };
    Review {
        address: REVIEWED.into(),
        since: "main".into(),
        head: HEAD.into(),
        step: Some(ReviewStep {
            number: 2,
            title: "Charge through a PaymentGateway".into(),
            sha: HEAD.into(),
            subject: "Charge through a PaymentGateway".into(),
            body: "Orders charge through the gateway trait, so the new provider slots in next.".into(),
        }),
        guide: vec![
            GuideNote { path: "src/gateway.rs".into(), text: "`PaymentGateway` is the seam. Worth a look: `charge` takes cents, not a float.".into() },
            GuideNote { path: "src/orders.rs".into(), text: "Orders call the trait instead of the client.".into() },
        ],
        left_out: Some("Left out: the CHANGES.md line, which the release writes.".into()),
        files: vec![
            ReviewFile {
                path: "src/gateway.rs".into(),
                old_path: None,
                status: FileStatus::Added,
                add: 3,
                del: 0,
                content: GATEWAY_CONTENT.into(),
                read: false,
                omitted: false,
                hunks: vec![Hunk {
                    head: "@@ -0,0 +1,3 @@".into(),
                    lines: vec![
                        line(LineKind::Add, None, Some(1), "pub trait PaymentGateway {"),
                        line(LineKind::Add, None, Some(2), "    fn charge(&self, cents: u64) -> Result<Receipt>;"),
                        line(LineKind::Add, None, Some(3), "}"),
                    ],
                }],
            },
            ReviewFile {
                path: "src/orders.rs".into(),
                old_path: None,
                status: FileStatus::Modified,
                add: 1,
                del: 1,
                content: "0a1b2c3d4e5f60718293a4b5c6d7e8f901234567".into(),
                read: true,
                omitted: false,
                hunks: vec![Hunk {
                    head: "@@ -40,3 +40,3 @@ impl Order {".into(),
                    lines: vec![
                        line(LineKind::Context, Some(40), Some(40), "    pub fn pay(&self) -> Result<Receipt> {"),
                        line(LineKind::Del, Some(41), None, "        client::charge(self.total as f64)"),
                        line(LineKind::Add, None, Some(41), "        self.gateway.charge(self.total)"),
                        line(LineKind::Context, Some(42), Some(42), "    }"),
                    ],
                }],
            },
            ReviewFile {
                path: "assets/logo.png".into(),
                old_path: None,
                status: FileStatus::Modified,
                add: 0,
                del: 0,
                content: "f".repeat(64),
                read: false,
                omitted: true,
                hunks: Vec::new(),
            },
        ],
    }
}

fn review_blob() -> (BlobRef, Vec<u8>) {
    seal_blob_with(
        &serde_json::to_vec(&review()).unwrap(),
        REVIEW_MEDIA_TYPE,
        seed("review/blob/key"),
        seed("review/blob/nonce")[..12].try_into().unwrap(),
    )
}

fn review_board() -> Payload {
    let Payload::Board(mut board) = stepped_board() else {
        unreachable!()
    };
    board.accepts = Some(vec![Kind::Answer, Kind::Push, Kind::ReviewOpen, Kind::ReviewComments, Kind::ReviewRead, Kind::AcceptStep]);
    board.items[0].review = Some(ReviewOffer { step: Some(2), files: 3 });
    Payload::Board(board)
}

fn review_open() -> Payload {
    Payload::ReviewOpen(ReviewOpen { address: REVIEWED.into() })
}

fn review_comments() -> Payload {
    Payload::ReviewComments(ReviewComments {
        address: REVIEWED.into(),
        step: Some(2),
        comments: vec![
            ReviewComment { path: "src/orders.rs".into(), line: 41, side: Side::Old, sha: Some(HEAD.into()), text: "Did anything else call client::charge with a float?".into() },
            ReviewComment { path: "src/gateway.rs".into(), line: 2, side: Side::New, sha: Some(HEAD.into()), text: "Name the currency next to cents.".into() },
        ],
    })
}

fn review_read() -> Payload {
    Payload::ReviewRead(ReviewRead { address: REVIEWED.into(), path: "src/gateway.rs".into(), read: true, content: GATEWAY_CONTENT.into() })
}

fn accept_step() -> Payload {
    Payload::AcceptStep(AcceptStep { address: REVIEWED.into(), step: 2, sha: HEAD.into() })
}

fn review_ready() -> Payload {
    Payload::Result(CommandResult {
        command: command_id("review-open"),
        device: Party::new("phone", Prefix::Device).id,
        outcome: Outcome::Done,
        text: "The review of step 2 is ready.".into(),
        blob: Some(review_blob().0),
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
    answer_to("refunds@shop")
}

fn prompt_answer(key: &str) -> Payload {
    Payload::PromptAnswer(PromptAnswer {
        address: "hub@kebbie".into(),
        prompt: "3f2a9c1d".into(),
        key: key.into(),
    })
}

fn push(head: &str) -> Payload {
    Payload::Push(PushApproval {
        address: "refunds@shop".into(),
        head: head.into(),
        card_sha: None,
    })
}

fn notice(retract: Vec<String>) -> Payload {
    Payload::Notice(Notice {
        category: NoticeCategory::Questions,
        title: "refunds · shop".into(),
        body: "Keep the old endpoint for a release? The lead recommends keeping it: one release of overlap.".into(),
        open: Target::Task { address: "refunds@shop".into() },
        retract,
    })
}

fn device_notice(device: &Id) -> Payload {
    Payload::Notice(Notice {
        category: NoticeCategory::NewDevice,
        title: "A new device asks to send commands".into(),
        body: "iPad waits for approval on this machine.".into(),
        open: Target::Device { device: device.clone() },
        retract: Vec::new(),
    })
}

fn device_decision(device: &DeviceCert, approve: bool) -> Payload {
    Payload::DeviceDecision(DeviceDecision {
        device: device.device.clone(),
        fingerprint: device.fingerprint(),
        approve,
    })
}

fn collapsed_header(case: &str, machine: &Party) -> Header {
    let mut header = machine_header(case, Kind::Notice, machine);
    header.collapse = Some(b64(&seed(&format!("{case}/collapse"))));
    header
}

fn result() -> Payload {
    Payload::Result(CommandResult {
        command: command_id("answer"),
        device: Party::new("phone", Prefix::Device).id,
        outcome: Outcome::Refused {
            reason: Reason::AlreadySettled,
        },
        text: "That question no longer waits for an answer".into(),
        blob: None,
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
    let envelope = seal_with(header, payload, &sender.signing(), &recipients, &randomness).unwrap();
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
    seal_unchecked(header, plaintext, &sender.signing(), &recipients, &randomness)
}

fn command_header(case: &str, kind: Kind, cert: &DeviceCert, machine: &Party) -> Header {
    Header::command(command_id(case), kind, cert.clone(), machine.id.clone(), AT)
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
        account,
        other_account,
    } = parties;
    let phone_cert = parties.cert(phone, "iPhone 16");
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

    let account_board = seal_case(
        "account-board",
        machine_header("account-board", Kind::Board, machine),
        &board(),
        machine,
        &[machine, account],
    );
    cases.push(opens(
        "board-opened-by-the-account",
        &account_board,
        account,
        machine,
        board(),
    ));
    let stepped = seal_case(
        "stepped-board",
        machine_header("stepped-board", Kind::Board, machine),
        &stepped_board(),
        machine,
        &[machine, account],
    );
    cases.push(opens(
        "board-with-steps-opened-by-the-account",
        &stepped,
        account,
        machine,
        stepped_board(),
    ));
    let next = seal_case(
        "next-board",
        machine_header("next-board", Kind::Board, machine),
        &next_board(),
        machine,
        &[machine, account],
    );
    cases.push(opens(
        "board-with-next-opened-by-the-account",
        &next,
        account,
        machine,
        next_board(),
    ));
    cases.push(fails(
        "board-for-the-account-is-not-a-device-row",
        account_board.envelope.clone(),
        phone,
        machine,
        OpenError::NotARecipient,
        Some(OpenError::NotARecipient),
    ));
    let mut moved_account = account_board.envelope.clone();
    let account_row = moved_account.keys.remove(&account.id).unwrap();
    moved_account.keys.insert(phone.id.clone(), account_row);
    cases.push(fails(
        "account-row-moved-to-a-device-does-not-open",
        moved_account,
        phone,
        machine,
        OpenError::Undecryptable,
        Some(OpenError::Undecryptable),
    ));

    let answer_sealed = seal_case(
        "answer",
        command_header("answer", Kind::Answer, &phone_cert, machine),
        &answer(),
        phone,
        &[account, machine],
    );
    cases.push(opens(
        "answer-opened-by-the-machine",
        &answer_sealed,
        machine,
        phone,
        answer(),
    ));
    cases.push(opens(
        "answer-opened-by-its-own-account",
        &answer_sealed,
        account,
        phone,
        answer(),
    ));

    let prompt_sealed = seal_case(
        "prompt-answer",
        command_header("prompt-answer", Kind::PromptAnswer, &phone_cert, machine),
        &prompt_answer("y"),
        phone,
        &[account, machine],
    );
    cases.push(opens(
        "prompt-answer-opened-by-the-machine",
        &prompt_sealed,
        machine,
        phone,
        prompt_answer("y"),
    ));

    let backlog_sealed = seal_case(
        "backlog-board",
        machine_header("backlog-board", Kind::Board, machine),
        &backlog_board(),
        machine,
        &[machine, account],
    );
    cases.push(opens(
        "board-with-backlog-opened-by-the-account",
        &backlog_sealed,
        account,
        machine,
        backlog_board(),
    ));
    let idea_text = || "Mail a weekly summary of what shipped.".to_string();
    for (name, case, kind, payload) in [
        ("idea-opened-by-the-machine", "idea", Kind::Idea, idea(idea_text())),
        ("item-answer-opened-by-the-machine", "item-answer", Kind::ItemAnswer, item_answer("Half up, like invoices.")),
        ("start-item-opened-by-the-machine", "start-item", Kind::StartItem, start_item(false)),
        ("start-item-anyway-opened-by-the-machine", "start-item-anyway", Kind::StartItem, start_item(true)),
        ("item-edit-opened-by-the-machine", "item-edit", Kind::ItemEdit, goal_edit()),
        ("title-edit-opened-by-the-machine", "title-edit", Kind::ItemEdit, title_edit()),
    ] {
        let sealed = seal_case(case, command_header(case, kind, &phone_cert, machine), &payload, phone, &[account, machine]);
        cases.push(opens(name, &sealed, machine, phone, payload));
    }
    let refused_start = seal_case(
        "questions-open",
        machine_header("start-item", Kind::Result, machine),
        &questions_open(),
        machine,
        &[machine, account],
    );
    cases.push(opens(
        "start-refused-for-open-questions-opened-by-the-account",
        &refused_start,
        account,
        machine,
        questions_open(),
    ));
    let refused_edit = seal_case("section-changed", machine_header("item-edit", Kind::Result, machine), &section_changed(), machine, &[machine, account]);
    cases.push(opens("edit-refused-for-a-newer-section-opened-by-the-account", &refused_edit, account, machine, section_changed()));

    let reviewing = seal_case(
        "review-board",
        machine_header("review-board", Kind::Board, machine),
        &review_board(),
        machine,
        &[machine, account],
    );
    cases.push(opens("board-offering-a-review-opened-by-the-account", &reviewing, account, machine, review_board()));
    for (name, case, kind, payload) in [
        ("review-open-opened-by-the-machine", "review-open", Kind::ReviewOpen, review_open()),
        ("review-comments-opened-by-the-machine", "review-comments", Kind::ReviewComments, review_comments()),
        ("review-read-opened-by-the-machine", "review-read", Kind::ReviewRead, review_read()),
        ("accept-step-opened-by-the-machine", "accept-step", Kind::AcceptStep, accept_step()),
    ] {
        let sealed = seal_case(case, command_header(case, kind, &phone_cert, machine), &payload, phone, &[account, machine]);
        cases.push(opens(name, &sealed, machine, phone, payload));
    }
    let ready = seal_case(
        "review-ready",
        machine_header("review-open", Kind::Result, machine),
        &review_ready(),
        machine,
        &[machine, account],
    );
    cases.push(opens("review-result-naming-its-blob-opened-by-the-account", &ready, account, machine, review_ready()));

    let mut notice_header = machine_header("notice", Kind::Notice, machine);
    notice_header.urgency = Some(Urgency::High);
    notice_header.collapse = Some(b64(&seed("notice/collapse")));
    let notice_sealed = seal_case(
        "notice",
        notice_header,
        &notice(Vec::new()),
        machine,
        &[phone, tablet],
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
        notice(Vec::new()),
    ));

    let new_device_sealed = seal_case(
        "new-device-notice",
        collapsed_header("new-device-notice", machine),
        &device_notice(&tablet.id),
        machine,
        &[phone],
    );
    cases.push(opens(
        "notice-of-a-device-to-approve",
        &new_device_sealed,
        phone,
        machine,
        device_notice(&tablet.id),
    ));
    let mut uncollapsed = machine_header("uncollapsed", Kind::Notice, machine);
    uncollapsed.urgency = Some(Urgency::Normal);
    cases.push(fails(
        "notice-without-a-collapse-tag",
        seal_bypassing_checks(
            "uncollapsed",
            uncollapsed,
            &serde_json::to_vec(&notice(Vec::new())).unwrap(),
            machine,
            &[phone],
        ),
        phone,
        machine,
        OpenError::Malformed,
        None,
    ));

    let retract = notice(vec![b64(&seed("notice/collapse"))]);
    let mut retract_header = machine_header("retract", Kind::Notice, machine);
    retract_header.urgency = Some(Urgency::Normal);
    retract_header.collapse = Some(b64(&seed("notice/collapse")));
    let retract_sealed = seal_case(
        "retract",
        retract_header,
        &retract,
        machine,
        &[phone],
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
        &[machine, account],
    );
    cases.push(opens(
        "result-opened-by-the-account",
        &result_sealed,
        account,
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
        stale(command_header("expired", Kind::Answer, &phone_cert, machine)),
        &answer(),
        phone,
        &[account, machine],
    );
    cases.push(fails(
        "expired",
        expired.envelope,
        machine,
        phone,
        OpenError::Expired,
        None,
    ));

    let mut early_header = command_header("not-yet-valid", Kind::Answer, &phone_cert, machine);
    early_header.at = NOW + 3_600;
    early_header.exp = NOW + 7_200;
    let early = seal_case(
        "not-yet-valid",
        early_header,
        &answer(),
        phone,
        &[account, machine],
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
        command_header("forged", Kind::Answer, &phone_cert, machine),
        &answer(),
        other_account,
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
        other_account,
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
    let mut long_lived = command_header("over-long-ttl", Kind::Answer, &phone_cert, machine);
    long_lived.exp = long_lived.at + Kind::Answer.max_ttl() + 1;
    cases.push(fails(
        "over-long-ttl",
        seal_bypassing_checks(
            "over-long-ttl",
            long_lived,
            &answer_bytes,
            phone,
            &[account, machine],
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
    cases.push(fails(
        "account-as-command-sender",
        seal_bypassing_checks(
            "account-answer",
            Header::new(command_id("account-answer"), Kind::Answer, account.id.clone(), machine.id.clone(), AT),
            &answer_bytes,
            account,
            &[account, machine],
        ),
        machine,
        account,
        OpenError::Malformed,
        None,
    ));
    let mut uncertified = command_header("uncertified", Kind::Answer, &phone_cert, machine);
    uncertified.cert = None;
    cases.push(fails(
        "command-without-a-certificate",
        seal_bypassing_checks("uncertified", uncertified, &answer_bytes, phone, &[account, machine]),
        machine,
        phone,
        OpenError::Malformed,
        None,
    ));
    let mut tablet_named = command_header("cert-of-another-device", Kind::Answer, &phone_cert, machine);
    tablet_named.from = tablet.id.clone();
    cases.push(fails(
        "sender-is-not-the-certified-device",
        seal_bypassing_checks("cert-of-another-device", tablet_named, &answer_bytes, tablet, &[account, machine]),
        machine,
        tablet,
        OpenError::Malformed,
        None,
    ));
    let mut certified_board = machine_header("certified-board", Kind::Board, machine);
    certified_board.cert = Some(phone_cert.clone());
    cases.push(fails(
        "certificate-on-a-board",
        seal_bypassing_checks("certified-board", certified_board, &board_bytes, machine, &[machine, account]),
        account,
        machine,
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
    let mut urgent_answer = command_header("urgent-answer", Kind::Answer, &phone_cert, machine);
    urgent_answer.urgency = Some(Urgency::High);
    cases.push(fails(
        "urgency-on-an-answer",
        seal_bypassing_checks(
            "urgent-answer",
            urgent_answer,
            &answer_bytes,
            phone,
            &[account, machine],
        ),
        machine,
        phone,
        OpenError::Malformed,
        None,
    ));
    let mut long_collapse = machine_header("long-collapse", Kind::Notice, machine);
    long_collapse.collapse = Some("a".repeat(65));
    let notice_bytes = serde_json::to_vec(&notice(Vec::new())).unwrap();
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
        command_header("kind-mismatch", Kind::Answer, &phone_cert, machine),
        &push_bytes,
        phone,
        &[account, machine],
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
    leaked_board["items"][0]["path"] = json!("/Users/someone/projects/shop");
    let mut extra_result = serde_json::to_value(result()).unwrap();
    extra_result["detail"] = json!("seen by the hub");
    let mut reasonless = serde_json::to_value(result()).unwrap();
    reasonless.as_object_mut().unwrap().remove("reason");
    let mut url_target = serde_json::to_value(notice(Vec::new())).unwrap();
    url_target["open"] = json!({ "to": "url", "url": "https://example.com/" });
    let mut absolute_home = serde_json::to_value(backlog_board()).unwrap();
    absolute_home["backlog"][0]["home"]["label"] = json!("/Users/someone/vault/Projects/refund-rounding.md");
    let mut counted_elsewhere = serde_json::to_value(result()).unwrap();
    counted_elsewhere["blocking"] = json!(2);
    let mut uncounted = serde_json::to_value(questions_open()).unwrap();
    uncounted.as_object_mut().unwrap().remove("blocking");
    let strays: [(&'static str, &str, Kind, Value); 7] = [
        (
            "backlog-row-with-an-absolute-home",
            "stray/backlog",
            Kind::Board,
            absolute_home,
        ),
        (
            "result-counting-questions-for-another-reason",
            "stray/counted",
            Kind::Result,
            counted_elsewhere,
        ),
        (
            "open-questions-refusal-without-a-count",
            "stray/uncounted",
            Kind::Result,
            uncounted,
        ),
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
            "notice-opening-a-url",
            "stray/notice",
            Kind::Notice,
            url_target,
        ),
    ];
    for (name, label, kind, payload) in strays {
        let header = if kind == Kind::Notice { collapsed_header(label, machine) } else { machine_header(label, kind, machine) };
        let envelope = seal_bypassing_checks(
            label,
            header,
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
        Kind::PromptAnswer,
        Kind::Idea,
        Kind::ItemAnswer,
        Kind::StartItem,
        Kind::ItemEdit,
        Kind::DeviceDecision,
        Kind::ReviewOpen,
        Kind::ReviewComments,
        Kind::ReviewRead,
        Kind::AcceptStep,
    ]
    .into_iter()
    .map(|kind| (kind.name(), kind.max_ttl()))
    .collect();
    json!({ "now": NOW, "clock_skew": crate::CLOCK_SKEW, "max_ttl": max_ttl, "hpke_info": String::from_utf8_lossy(crate::HPKE_INFO), "cases": cases })
}

#[derive(Clone)]
enum CaseTrust {
    Account,
    Devices {
        approved: Vec<(Id, [u8; 32])>,
        denied: Vec<Id>,
    },
}

impl DeviceTrust for CaseTrust {
    fn trust(&self, device: &VerifiedDevice, _: u64) -> Trust {
        match self {
            CaseTrust::Account => Trust::Approved,
            CaseTrust::Devices { approved, denied } => {
                if denied.contains(device.id()) {
                    Trust::Denied
                } else if approved.iter().any(|(id, signing)| id == device.id() && signing == device.signing()) {
                    Trust::Approved
                } else {
                    Trust::Waiting
                }
            }
        }
    }
}

impl CaseTrust {
    fn to_json(&self) -> Value {
        match self {
            CaseTrust::Account => json!("account"),
            CaseTrust::Devices { approved, denied } => json!({
                "approved": approved.iter().map(|(device, signing)| json!({ "device": device, "signing": b64(signing) })).collect::<Vec<_>>(),
                "denied": denied,
            }),
        }
    }
}

struct AuthorizeCase {
    name: &'static str,
    accepts: Vec<Kind>,
    trust: CaseTrust,
    applied: AppliedCommands,
    envelope: Envelope,
    expect: Result<Verified, Refusal>,
}

fn verified(cert: &DeviceCert, account: &AccountKey) -> VerifiedDevice {
    assert!(account.verifies_device(cert));
    VerifiedDevice::new(cert.clone())
}

fn authorize_json(parties: &Parties) -> Value {
    let Parties {
        machine,
        phone,
        tablet,
        intruder,
        other_machine,
        account,
        other_account,
    } = parties;
    let account_key = parties.account_key();
    let phone_cert = parties.cert(phone, "iPhone 16");
    let tablet_cert = parties.cert(tablet, "iPad");
    let phone_device = verified(&phone_cert, &account_key);
    let all = vec![Kind::Answer, Kind::Push, Kind::Start, Kind::Routine, Kind::PromptAnswer];
    let start = |prompt: String| {
        Payload::Start(StartTask {
            repo: "shop".into(),
            name: "invoice-rounding".into(),
            prompt,
        })
    };
    let from_phone = |case: &str, kind: Kind, payload: &Payload| {
        seal_case(
            case,
            command_header(case, kind, &phone_cert, machine),
            payload,
            phone,
            &[account, machine],
        )
        .envelope
    };
    let case = |name, envelope, expect| AuthorizeCase {
        name,
        accepts: all.clone(),
        trust: CaseTrust::Account,
        applied: AppliedCommands::default(),
        envelope,
        expect,
    };
    let applies = |payload: Payload| -> Result<Verified, Refusal> {
        Ok(Verified {
            device: phone_device.clone(),
            payload,
        })
    };
    let only = |approved: &[(&DeviceCert, &Party)], denied: &[&Party]| CaseTrust::Devices {
        approved: approved.iter().map(|(cert, key)| (cert.device.clone(), key.public().signing)).collect(),
        denied: denied.iter().map(|party| party.id.clone()).collect(),
    };

    let answered = from_phone("authorize/answer", Kind::Answer, &answer());
    let mut applied = AppliedCommands::default();
    applied.record(
        NOW - 60,
        CommandResult {
            command: answered.header.id.clone(),
            device: phone.id.clone(),
            outcome: Outcome::Done,
            text: "Answered refunds@shop's question.".into(),
            blob: None,
        },
    );
    let routine = Payload::Routine(RunRoutine {
        id: "nightly-deps".into(),
    });
    let mut long_lived = command_header("authorize/over-long-ttl", Kind::Answer, &phone_cert, machine);
    long_lived.exp = long_lived.at + Kind::Answer.max_ttl() + 1;
    let long_lived = seal_bypassing_checks(
        "authorize/over-long-ttl",
        long_lived,
        &serde_json::to_vec(&answer()).unwrap(),
        phone,
        &[account, machine],
    );
    let mut lingering = command_header("authorize/prompt-over-ten-minutes", Kind::PromptAnswer, &phone_cert, machine);
    lingering.exp = lingering.at + 601;
    let lingering = seal_bypassing_checks(
        "authorize/prompt-over-ten-minutes",
        lingering,
        &serde_json::to_vec(&prompt_answer("y")).unwrap(),
        phone,
        &[account, machine],
    );
    let mut future = answered.clone();
    future.header.v = 2;
    let forged_replay = seal_case(
        "authorize/forged-replay",
        command_header("authorize/answer", Kind::Answer, &phone_cert, machine),
        &answer(),
        other_account,
        &[machine],
    )
    .envelope;
    let same_id_from_tablet = seal_case(
        "authorize/same-id-from-tablet",
        command_header("authorize/answer", Kind::Answer, &tablet_cert, machine),
        &answer(),
        tablet,
        &[account, machine],
    )
    .envelope;
    let expired = |case: &str| {
        seal_case(
            case,
            stale(command_header(case, Kind::Answer, &phone_cert, machine)),
            &answer(),
            phone,
            &[account, machine],
        )
        .envelope
    };
    let expired_answer = expired("authorize/expired-replay");
    let mut expired_applied = AppliedCommands::default();
    expired_applied.record(
        NOW - 4_000,
        CommandResult {
            command: expired_answer.header.id.clone(),
            device: phone.id.clone(),
            outcome: Outcome::Done,
            text: "Answered refunds@shop's question.".into(),
            blob: None,
        },
    );
    let expired_result = expired_applied
        .result(&phone.id, &expired_answer.header.id)
        .unwrap()
        .clone();
    let foreign_cert = certify(other_account, other_account, intruder, "Stranger's phone");
    let claimed_cert = certify(other_account, account, intruder, "Stranger's phone");
    let mut renamed_cert = phone_cert.clone();
    renamed_cert.name = "Work iPhone".into();
    let signed_with = |case: &str, cert: &DeviceCert, signer: &Party| {
        seal_case(case, command_header(case, Kind::Answer, cert, machine), &answer(), signer, &[account, machine]).envelope
    };
    let rekeyed_tablet = certify(account, account, tablet, "iPad");
    let mut rekeyed_tablet_trust = only(&[], &[]);
    if let CaseTrust::Devices { approved, .. } = &mut rekeyed_tablet_trust {
        approved.push((rekeyed_tablet.device.clone(), intruder.public().signing));
    }

    let lingering_idea = {
        let mut header = command_header("authorize/idea-over-an-hour", Kind::Idea, &phone_cert, machine);
        header.exp = header.at + 3_601;
        seal_bypassing_checks(
            "authorize/idea-over-an-hour",
            header,
            &serde_json::to_vec(&idea("x".into())).unwrap(),
            phone,
            &[account, machine],
        )
    };
    let backlog_case = |name, envelope, expect| AuthorizeCase {
        name,
        accepts: backlog_kinds(),
        trust: CaseTrust::Account,
        applied: AppliedCommands::default(),
        envelope,
        expect,
    };
    let start_item_from_phone = from_phone("authorize/start-item", Kind::StartItem, &start_item(false));
    let deciding: Vec<Kind> = all.iter().copied().chain([Kind::DeviceDecision]).collect();
    let decision_case = |name, trust, envelope, expect| AuthorizeCase {
        name,
        accepts: deciding.clone(),
        trust,
        applied: AppliedCommands::default(),
        envelope,
        expect,
    };
    let approve_tablet = device_decision(&tablet_cert, true);
    let mut misprinted = tablet_cert.clone();
    misprinted.signing = phone.public().signing;
    let misprinted = Payload::DeviceDecision(DeviceDecision {
        fingerprint: misprinted.fingerprint().to_lowercase(),
        ..DeviceDecision { device: tablet.id.clone(), fingerprint: String::new(), approve: true }
    });
    let decision_cases = vec![
        decision_case(
            "approved-device-approves-a-waiting-device",
            only(&[(&phone_cert, phone)], &[]),
            from_phone("authorize/approve-tablet", Kind::DeviceDecision, &approve_tablet),
            applies(approve_tablet.clone()),
        ),
        decision_case(
            "approved-device-denies-a-waiting-device",
            only(&[(&phone_cert, phone)], &[]),
            from_phone("authorize/deny-tablet", Kind::DeviceDecision, &device_decision(&tablet_cert, false)),
            applies(device_decision(&tablet_cert, false)),
        ),
        decision_case(
            "device-decision-from-a-device-still-waiting-is-not-approved",
            only(&[(&tablet_cert, tablet)], &[]),
            from_phone("authorize/waiting-decides", Kind::DeviceDecision, &approve_tablet),
            Err(Refusal::Answer(Reason::NotApproved)),
        ),
        decision_case(
            "device-decision-on-the-sending-device-is-refused",
            only(&[(&phone_cert, phone)], &[]),
            from_phone("authorize/decides-itself", Kind::DeviceDecision, &device_decision(&phone_cert, true)),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        decision_case(
            "device-decision-with-a-malformed-fingerprint-is-refused",
            only(&[(&phone_cert, phone)], &[]),
            from_phone("authorize/misprinted", Kind::DeviceDecision, &misprinted),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        decision_case(
            "device-decision-from-a-denied-device-is-refused",
            only(&[], &[phone]),
            from_phone("authorize/denied-decides", Kind::DeviceDecision, &approve_tablet),
            Err(Refusal::Answer(Reason::Denied)),
        ),
        AuthorizeCase {
            name: "device-decision-to-a-machine-that-takes-approvals-only-at-the-machine-is-refused",
            accepts: all.clone(),
            trust: only(&[(&phone_cert, phone)], &[]),
            applied: AppliedCommands::default(),
            envelope: from_phone("authorize/approve-at-machine", Kind::DeviceDecision, &approve_tablet),
            expect: Err(Refusal::Answer(Reason::NotAllowed)),
        },
    ];
    let backlog_cases = vec![
        backlog_case(
            "device-files-an-idea",
            from_phone("authorize/idea", Kind::Idea, &idea("Mail a weekly summary of what shipped.".into())),
            applies(idea("Mail a weekly summary of what shipped.".into())),
        ),
        backlog_case(
            "device-answers-an-items-question",
            from_phone("authorize/item-answer", Kind::ItemAnswer, &item_answer("Half up, like invoices.")),
            applies(item_answer("Half up, like invoices.")),
        ),
        backlog_case(
            "device-edits-an-items-section",
            from_phone("authorize/item-edit", Kind::ItemEdit, &goal_edit()),
            applies(goal_edit()),
        ),
        backlog_case(
            "title-edit-over-two-lines-is-refused",
            from_phone("authorize/title-two-lines", Kind::ItemEdit, &item_edit(None, "Refunds round\nlike invoices", "Refunds round like their invoice")),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        backlog_case(
            "device-starts-an-item-that-lives-a-day",
            start_item_from_phone.clone(),
            applies(start_item(false)),
        ),
        backlog_case(
            "device-starts-an-item-anyway",
            from_phone("authorize/start-item-anyway", Kind::StartItem, &start_item(true)),
            applies(start_item(true)),
        ),
        AuthorizeCase {
            name: "start-item-to-a-machine-that-takes-plain-starts-only-is-refused",
            accepts: all.clone(),
            trust: CaseTrust::Account,
            applied: AppliedCommands::default(),
            envelope: start_item_from_phone,
            expect: Err(Refusal::Answer(Reason::NotAllowed)),
        },
        backlog_case(
            "idea-over-its-length-is-refused",
            from_phone("authorize/long-idea", Kind::Idea, &idea("x".repeat(crate::MAX_IDEA_CHARS + 1))),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        backlog_case(
            "blank-answer-to-an-items-question-is-refused",
            from_phone("authorize/blank-item-answer", Kind::ItemAnswer, &item_answer("  ")),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        backlog_case(
            "start-item-naming-a-path-on-the-machine-is-refused",
            seal_bypassing_checks(
                "authorize/start-item-path",
                command_header("authorize/start-item-path", Kind::StartItem, &phone_cert, machine),
                &serde_json::to_vec(&json!({ "type": "start-item", "item": "vault:/Users/someone/vault/Projects/x.md" })).unwrap(),
                phone,
                &[account, machine],
            ),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        backlog_case(
            "idea-living-past-an-hour-is-dropped",
            lingering_idea,
            Err(Refusal::Drop(crate::Dropped::Malformed)),
        ),
    ];

    let cases = vec![
        case("device-answers", answered.clone(), applies(answer())),
        AuthorizeCase {
            name: "same-command-twice-replays-the-stored-result",
            accepts: all.clone(),
            trust: CaseTrust::Account,
            applied: applied.clone(),
            envelope: answered.clone(),
            expect: Err(Refusal::Replay(Box::new(
                applied
                    .result(&phone.id, &answered.header.id)
                    .unwrap()
                    .clone(),
            ))),
        },
        AuthorizeCase {
            name: "same-command-id-from-another-device-is-not-a-replay",
            accepts: all.clone(),
            trust: CaseTrust::Account,
            applied: applied.clone(),
            envelope: same_id_from_tablet,
            expect: Ok(Verified { device: verified(&tablet_cert, &account_key), payload: answer() }),
        },
        AuthorizeCase {
            name: "forged-signature-with-an-applied-id-is-dropped",
            accepts: all.clone(),
            trust: CaseTrust::Account,
            applied: applied.clone(),
            envelope: forged_replay,
            expect: Err(Refusal::Drop(crate::Dropped::BadSignature)),
        },
        AuthorizeCase {
            name: "applied-id-that-has-expired-replays",
            accepts: all.clone(),
            trust: CaseTrust::Account,
            applied: expired_applied,
            envelope: expired_answer,
            expect: Err(Refusal::Replay(Box::new(expired_result))),
        },
        AuthorizeCase {
            name: "device-the-machine-approved-answers",
            accepts: all.clone(),
            trust: only(&[(&phone_cert, phone)], &[]),
            applied: AppliedCommands::default(),
            envelope: answered.clone(),
            expect: applies(answer()),
        },
        AuthorizeCase {
            name: "device-the-machine-has-not-approved-waits-with-its-command",
            accepts: all.clone(),
            trust: only(&[(&tablet_cert, tablet)], &[]),
            applied: AppliedCommands::default(),
            envelope: answered.clone(),
            expect: Err(Refusal::Waiting(Box::new(Verified { device: phone_device.clone(), payload: answer() }))),
        },
        AuthorizeCase {
            name: "device-approved-under-another-key-waits",
            accepts: all.clone(),
            trust: rekeyed_tablet_trust,
            applied: AppliedCommands::default(),
            envelope: signed_with("authorize/rekeyed", &rekeyed_tablet, tablet),
            expect: Err(Refusal::Waiting(Box::new(Verified { device: verified(&rekeyed_tablet, &account_key), payload: answer() }))),
        },
        AuthorizeCase {
            name: "command-that-expired-waiting-for-approval-is-not-approved",
            accepts: all.clone(),
            trust: only(&[], &[]),
            applied: AppliedCommands::default(),
            envelope: expired("authorize/expired-waiting"),
            expect: Err(Refusal::Answer(Reason::NotApproved)),
        },
        AuthorizeCase {
            name: "device-the-machine-denied-is-refused",
            accepts: all.clone(),
            trust: only(&[(&phone_cert, phone)], &[phone]),
            applied: AppliedCommands::default(),
            envelope: answered.clone(),
            expect: Err(Refusal::Answer(Reason::Denied)),
        },
        case(
            "device-approves-a-push",
            from_phone("authorize/push", Kind::Push, &push(HEAD)),
            applies(push(HEAD)),
        ),
        case(
            "device-runs-a-routine",
            from_phone("authorize/routine", Kind::Routine, &routine),
            applies(routine.clone()),
        ),
        case(
            "device-answers-an-agents-prompt",
            from_phone("authorize/prompt", Kind::PromptAnswer, &prompt_answer("y")),
            applies(prompt_answer("y")),
        ),
        AuthorizeCase {
            name: "prompt-answer-to-a-machine-that-does-not-take-it-is-refused",
            accepts: vec![Kind::Answer, Kind::Push],
            trust: CaseTrust::Account,
            applied: AppliedCommands::default(),
            envelope: from_phone("authorize/prompt", Kind::PromptAnswer, &prompt_answer("y")),
            expect: Err(Refusal::Answer(Reason::NotAllowed)),
        },
        case(
            "prompt-answer-living-past-ten-minutes-is-dropped",
            lingering,
            Err(Refusal::Drop(crate::Dropped::Malformed)),
        ),
        case(
            "prompt-answer-with-keystrokes-for-a-key-is-refused",
            from_phone("authorize/prompt-keys", Kind::PromptAnswer, &prompt_answer("y enter")),
            Err(Refusal::Answer(Reason::BadRequest)),
        ),
        AuthorizeCase {
            name: "push-to-a-machine-that-takes-answers-only-is-refused",
            accepts: vec![Kind::Answer, Kind::PromptAnswer],
            trust: CaseTrust::Account,
            applied: AppliedCommands::default(),
            envelope: from_phone("authorize/push-to-answers", Kind::Push, &push(HEAD)),
            expect: Err(Refusal::Answer(Reason::NotAllowed)),
        },
        AuthorizeCase {
            name: "answer-to-a-machine-that-takes-nothing-is-refused",
            accepts: Vec::new(),
            trust: CaseTrust::Account,
            applied: AppliedCommands::default(),
            envelope: from_phone("authorize/answer-to-nothing", Kind::Answer, &answer()),
            expect: Err(Refusal::Answer(Reason::NotAllowed)),
        },
        case(
            "expired-command-is-refused",
            expired("authorize/expired"),
            Err(Refusal::Answer(Reason::Expired)),
        ),
        AuthorizeCase {
            name: "kind-the-machine-does-not-take-is-refused",
            accepts: vec![Kind::Answer],
            trust: CaseTrust::Account,
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
                command_header("authorize/unsealed", Kind::Answer, &phone_cert, machine),
                &answer(),
                phone,
                &[account],
            )
            .envelope,
            Err(Refusal::Answer(Reason::Undecryptable)),
        ),
        case(
            "device-of-another-account-is-dropped",
            seal_case(
                "authorize/other-account",
                command_header("authorize/other-account", Kind::Answer, &foreign_cert, machine),
                &answer(),
                intruder,
                &[other_account, machine],
            )
            .envelope,
            Err(Refusal::Drop(crate::Dropped::WrongAccount)),
        ),
        case(
            "certificate-another-key-signed-for-the-account-is-dropped",
            signed_with("authorize/claimed-cert", &claimed_cert, intruder),
            Err(Refusal::Drop(crate::Dropped::BadCertificate)),
        ),
        case(
            "certificate-renamed-after-signing-is-dropped",
            signed_with("authorize/renamed-cert", &renamed_cert, phone),
            Err(Refusal::Drop(crate::Dropped::BadCertificate)),
        ),
        case(
            "command-from-the-account-key-is-dropped",
            seal_bypassing_checks(
                "authorize/account",
                Header::new(command_id("authorize/account"), Kind::Answer, account.id.clone(), machine.id.clone(), AT),
                &serde_json::to_vec(&answer()).unwrap(),
                account,
                &[account, machine],
            ),
            Err(Refusal::Drop(crate::Dropped::Malformed)),
        ),
        case(
            "envelope-signed-by-another-key-than-its-certificate-is-dropped",
            signed_with("authorize/forged", &phone_cert, intruder),
            Err(Refusal::Drop(crate::Dropped::BadSignature)),
        ),
        case(
            "command-for-another-machine-is-dropped",
            seal_case(
                "authorize/other",
                command_header("authorize/other", Kind::Answer, &phone_cert, other_machine),
                &answer(),
                phone,
                &[account, other_machine],
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
        .chain(backlog_cases)
        .chain(decision_cases)
        .map(|case| {
            let gate = Machine { id: &machine.id, keys: &machine.keys, account: &account_key, accepts: &case.accepts, trust: &case.trust };
            let decided = authorize(&gate, &case.applied, &case.envelope, NOW);
            assert_eq!(decided, case.expect, "{}", case.name);
            let expect = match case.expect {
                Ok(verified) => json!({ "device": verified.device.id(), "payload": verified.payload }),
                Err(Refusal::Waiting(held)) => json!({ "waiting": held.device.id(), "payload": held.payload }),
                Err(Refusal::Answer(reason)) => json!({ "answer": reason.key() }),
                Err(Refusal::Drop(dropped)) => json!({ "drop": dropped.key() }),
                Err(Refusal::Replay(result)) => json!({ "replay": result }),
            };
            json!({ "name": case.name, "accepts": case.accepts, "trust": case.trust.to_json(), "applied": case.applied, "envelope": case.envelope, "expect": expect })
        })
        .collect();
    json!({ "now": NOW, "machine": machine.name, "account": account_key, "cases": rendered })
}

fn devices_json(parties: &Parties) -> Value {
    let Parties { machine, phone, tablet, intruder, account, other_account, .. } = parties;
    let key = parties.account_key();
    let phone_cert = parties.cert(phone, "iPhone 16");
    let tablet_cert = parties.cert(tablet, "iPad");
    let certs: Vec<Value> = [(phone, &phone_cert), (tablet, &tablet_cert)]
        .into_iter()
        .map(|(party, cert)| {
            assert!(key.verifies_device(cert));
            assert_eq!(cert.recipient().1, party.device_key().public());
            json!({ "device": party.name, "cert": cert, "statement": b64(&cert.statement().unwrap()), "fingerprint": cert.fingerprint() })
        })
        .collect();
    let mut renamed = phone_cert.clone();
    renamed.name = "Work iPhone".into();
    let mut rekeyed = phone_cert.clone();
    rekeyed.signing = intruder.public().signing;
    let mut reencrypted = phone_cert.clone();
    reencrypted.encryption = intruder.public().encryption;
    let mut later = phone_cert.clone();
    later.created_at += 1;
    let refusals: Vec<Value> = [
        ("renamed-after-signing", renamed),
        ("another-device-key", rekeyed),
        ("another-encryption-key", reencrypted),
        ("another-creation-time", later),
        ("signed-by-another-account-key", certify(other_account, account, phone, "iPhone 16")),
        ("issued-by-another-account", certify(other_account, other_account, phone, "iPhone 16")),
    ]
    .into_iter()
    .map(|(name, cert)| {
        assert!(!key.verifies_device(&cert), "{name}");
        json!({ "name": name, "cert": cert })
    })
    .collect();
    let ids: Vec<Value> = ["phone", "tablet"]
        .into_iter()
        .map(|name| {
            let random: [u8; 10] = seed(&format!("{name}/id"))[..10].try_into().unwrap();
            let id = Id::ulid(Prefix::Device, AT * 1_000, random).unwrap();
            json!({ "millis": AT * 1_000, "random": b64(&random), "id": id })
        })
        .collect();
    let answer_bytes = serde_json::to_vec(&answer()).unwrap();
    let sealed = |case: &str, cert: &DeviceCert, signer: &Party| {
        seal_bypassing_checks(case, command_header(case, Kind::Answer, cert, machine), &answer_bytes, signer, &[account, machine])
    };
    let mut renamed_header = command_header("devices/renamed", Kind::Answer, &phone_cert, machine);
    renamed_header.cert.as_mut().unwrap().name = "Work iPhone".into();
    let commands: Vec<Value> = [
        ("from-the-phone", sealed("devices/phone", &phone_cert, phone)),
        ("from-the-tablet", sealed("devices/tablet", &tablet_cert, tablet)),
        ("from-a-device-of-another-account", sealed("devices/foreign", &certify(other_account, other_account, intruder, "Stranger's phone"), intruder)),
        ("certificate-renamed-after-signing", seal_bypassing_checks("devices/renamed", renamed_header, &answer_bytes, phone, &[account, machine])),
        ("signed-by-another-key-than-its-certificate", sealed("devices/forged", &phone_cert, intruder)),
        ("expired", seal_bypassing_checks("devices/expired", stale(command_header("devices/expired", Kind::Answer, &phone_cert, machine)), &answer_bytes, phone, &[account, machine])),
    ]
    .into_iter()
    .map(|(name, envelope)| {
        let expect = match crate::verify_command(&envelope, &key, NOW) {
            Ok(device) => json!({ "device": device.id(), "fingerprint": device.fingerprint() }),
            Err(error) => json!({ "error": error.key() }),
        };
        let opened = crate::open_command(&envelope, &account.id, &account.keys, &key, NOW);
        assert_eq!(opened.is_ok(), expect.get("device").is_some(), "{name}");
        json!({ "name": name, "envelope": envelope, "header_bytes": b64(&envelope.header.bytes()), "expect": expect })
    })
    .collect();
    json!({
        "cert_domain": String::from_utf8_lossy(DEVICE_CERT_DOMAIN),
        "fingerprint_domain": String::from_utf8_lossy(DEVICE_FINGERPRINT_DOMAIN),
        "now": NOW,
        "account": account.name,
        "key": key,
        "machine": machine.name,
        "certs": certs,
        "refusals": refusals,
        "ids": ids,
        "commands": commands,
        "payload": answer(),
    })
}

fn signatures_json(parties: &Parties) -> Value {
    let cases: Vec<Value> = [&parties.machine, &parties.phone]
        .into_iter()
        .map(|party| {
            let message = format!("kebbie-hub signature vector for {}", party.name);
            let signature = SigningKey::from_bytes(&party.keys.to_bytes().0).sign(message.as_bytes());
            json!({ "signer": party.name, "message": b64(message.as_bytes()), "signature": b64(&signature.to_bytes()) })
        })
        .collect();
    json!({ "cases": cases })
}

fn connect_json(parties: &Parties) -> Value {
    let Parties { machine, intruder, .. } = parties;
    let connect = ConnectSecret::with_secret(&machine.public(), seed("connect/secret"));
    json!({
        "domain": String::from_utf8_lossy(CONNECT_FINGERPRINT_DOMAIN),
        "machine": machine.name,
        "fingerprint": b64(&machine_fingerprint(&machine.public())),
        "other_fingerprint": b64(&machine_fingerprint(&intruder.public())),
        "link": connect.link("https://hub.example.com/connect?code=K7QF-M2XD"),
        "code": connect.pairing_code(),
    })
}

fn accounts_json(parties: &Parties) -> Value {
    let account = &parties.account;
    let machine = &parties.machine;
    let version = 1;
    let key = AccountKey::new(account.id.clone(), version, account.public()).unwrap();
    let credential = seed("account/credential")[..20].to_vec();
    let salt = seed("account/salt");
    let prf_output = seed("account/prf");
    let nonce: [u8; WRAP_NONCE_LEN] = seed("account/wrap-nonce")[..WRAP_NONCE_LEN].try_into().unwrap();
    let unlock = Unlock {
        account: &account.id,
        credential: &credential,
        version,
        prf_output: &prf_output,
        salt: &salt,
    };
    let wrapped = unlock.wrap_with_nonce(&account.keys, nonce).unwrap();
    assert_eq!(unlock.unwrap(&wrapped).unwrap().public(), account.public());
    let other_credential = seed("account/other-credential")[..20].to_vec();
    let other_salt = seed("account/other-salt");
    let other_prf = seed("account/other-prf");
    let intruder_account = Id::account(seed("account/intruder")[..16].try_into().unwrap());
    let refusals: Vec<Value> = [
        ("another-prf-output", Unlock { prf_output: &other_prf, ..unlock }),
        ("another-salt", Unlock { salt: &other_salt, ..unlock }),
        ("another-credential", Unlock { credential: &other_credential, ..unlock }),
        ("another-account", Unlock { account: &intruder_account, ..unlock }),
        ("another-key-version", Unlock { version: 2, ..unlock }),
    ]
    .into_iter()
    .map(|(name, changed)| {
        assert!(changed.unwrap(&wrapped).is_err(), "{name}");
        json!({
            "name": name,
            "account": changed.account,
            "credential": b64(changed.credential),
            "key_version": changed.version,
            "prf_output": b64(changed.prf_output),
            "salt": b64(changed.salt),
            "expect": "wrong-account-key",
        })
    })
    .collect();
    let fingerprints = key.fingerprints();
    let wrapper_signature = sign_wrapper(&account.keys, &key, &credential, &wrapped).unwrap();
    let list = MachineList::default()
        .with(parties.other_machine.id.clone(), parties.other_machine.public())
        .with(machine.id.clone(), machine.public());
    let list_signature = list.sign(&account.keys, &account.id).unwrap();
    let removed = list.without(&parties.other_machine.id);
    let removed_signature = removed.sign(&account.keys, &account.id).unwrap();
    let connect = ConnectSecret::with_secret(&machine.public(), seed("connect/secret"));
    let code = connect.pairing_code();
    let handoff_nonce = seed("account/handoff-nonce");
    let handoff = AccountHandoff::with_nonce(key.clone(), handoff_nonce, &code);
    let commitment = handoff.commitment();
    let swapped = AccountKey::new(account.id.clone(), version, parties.intruder.public()).unwrap();
    let reveals: Vec<Value> = [
        ("pins-the-committed-keys", &key, code.as_str()),
        ("swapped-keys-are-refused", &swapped, code.as_str()),
        ("another-code-is-refused", &key, "guessed-code"),
    ]
    .into_iter()
    .map(|(name, revealed, machine_code)| {
        let expect = match pin_account(&commitment, revealed, &handoff_nonce, machine_code) {
            Ok(pinned) => json!({ "pinned": pinned }),
            Err(_) => json!({ "error": "commitment-mismatch" }),
        };
        json!({ "name": name, "revealed": revealed, "machine_code": machine_code, "expect": expect })
    })
    .collect();
    json!({
        "wrap_key_info": String::from_utf8_lossy(WRAP_KEY_INFO),
        "wrap_ad_domain": String::from_utf8_lossy(WRAP_AD_DOMAIN),
        "wrapper_sign_domain": String::from_utf8_lossy(WRAPPER_SIGN_DOMAIN),
        "machine_list_domain": String::from_utf8_lossy(MACHINE_LIST_DOMAIN),
        "signing_fingerprint_domain": String::from_utf8_lossy(SIGNING_FINGERPRINT_DOMAIN),
        "encryption_fingerprint_domain": String::from_utf8_lossy(ENCRYPTION_FINGERPRINT_DOMAIN),
        "commit_domain": String::from_utf8_lossy(ACCOUNT_COMMIT_DOMAIN),
        "account": account.name,
        "key": key,
        "wrap": {
            "credential": b64(&credential),
            "salt": b64(&salt),
            "prf_output": b64(&prf_output),
            "wrap_key": b64(&account_wrap_key(&prf_output, &salt)),
            "associated_data": b64(&wrap_associated_data(&account.id, &credential, version).unwrap()),
            "nonce": b64(&nonce),
            "wrapped": b64(&wrapped),
            "refusals": refusals,
        },
        "hub_prf_salt": {
            "domain": String::from_utf8_lossy(HUB_PRF_SALT_DOMAIN),
            "salt": b64(&hub_prf_salt()),
            "prf_output": b64(&prf_output),
            "wrap_key": b64(&account_wrap_key(&prf_output, &hub_prf_salt())),
        },
        "fingerprints": fingerprints,
        "wrapper": {
            "statement": b64(&wrapper_statement(&key, &credential, &wrapped).unwrap()),
            "signature": b64(&wrapper_signature),
        },
        "machines": {
            "list": list,
            "statement": b64(&list.statement(&account.id).unwrap()),
            "signature": b64(&list_signature),
            "removed": removed,
            "removed_signature": b64(&removed_signature),
            "trusted": machine.name,
            "removed_machine": parties.other_machine.name,
        },
        "handoff": {
            "code": code,
            "nonce": b64(&handoff_nonce),
            "commitment": b64(&commitment),
            "reveals": reveals,
        },
    })
}

fn keys_json(parties: &Parties) -> Value {
    let all: BTreeMap<&str, Value> = parties
        .all()
        .into_iter()
        .map(|party| (party.name, party.to_json()))
        .collect();
    json!({ "parties": all })
}

pub const SAMPLE_PLAN: &str = "Move refunds from the old charge endpoint to the gateway's `refunds.create`, so each refund books its own ledger entry when the gateway confirms it.

## Steps

1. Add `Gateway#refund` with one idempotency key per refund.
2. Book the refund ledger entry on the gateway's confirmation, not on the request.
3. Point the portal's refund button at the new path.
4. Keep `/charges/:id/refund` answering for one release and log every call.

## What the customer sees

Nothing new. A refund shows on the billing page the minute the gateway confirms it.

## Risks

- An app version older than 2.14 still calls the old endpoint.
- A double tap sends two requests; the idempotency key stops the second refund.

## Paths

| Path | Today | After |
| --- | --- | --- |
| `/charges/:id/refund` | books on request | logs, then forwards |
| `/refunds` | none | books on confirm |

## Request

```
POST /v1/refunds
{ \"charge\": \"ch_3P9xQ2\", \"amount\": 12500,
  \"idempotency_key\": \"refund-8812-1\" }
```

## Tests

- [ ] A second refund with the same key books nothing
- [ ] The old endpoint logs and forwards
- [ ] The ledger entry is dated on confirmation
";

fn backlog_json() -> Value {
    let (blob, ciphertext) = backlog_blob();
    let keys = [
        ("vault:Projects/refund-rounding.md", true),
        ("vault:Projects/Två ord.md", true),
        ("local:01JB7Q2M3N8K", true),
        ("kept:idea-4411", true),
        ("Projects/refund-rounding.md", false),
        ("vault:/Users/someone/vault/Projects/x.md", false),
        ("vault:~/vault/x.md", false),
        ("vault:Projects/../../etc/passwd", false),
        ("vault:Projects//x.md", false),
        ("vault:C:\\vault\\x.md", false),
        ("vault:host:/x.md", false),
        ("vault:Projects/x\n.md", false),
        ("local:../x", false),
        ("local:", false),
        ("kept:a b", false),
        ("task:refunds", false),
    ];
    let keys: Vec<Value> = keys
        .into_iter()
        .map(|(key, valid)| {
            assert_eq!(ItemKey::parse(key).is_ok(), valid, "{key}");
            json!({ "key": key, "valid": valid })
        })
        .collect();
    let hashes: Vec<Value> = ["", GOAL, "Två ord · €"].into_iter().map(|text| json!({ "text": text, "hash": crate::text_hash(text) })).collect();
    let edit = serde_json::to_value(goal_edit()).unwrap();
    let with = |key: &str, value: Value| {
        let mut changed = edit.clone();
        changed[key] = value;
        changed
    };
    let edits = [
        ("a section edit", edit.clone(), true),
        ("a title edit", serde_json::to_value(title_edit()).unwrap(), true),
        ("a section emptied", with("text", json!("")), true),
        ("a section at its length", with("text", json!("x".repeat(crate::MAX_SECTION_CHARS))), true),
        ("a section past its length", with("text", json!("x".repeat(crate::MAX_SECTION_CHARS + 1))), false),
        ("a section too large to send", with("text", json!("😀".repeat(crate::MAX_SECTION_CHARS))), false),
        ("a section with a control character", with("text", json!("a\u{7}b")), false),
        ("a section key in capitals", with("section", json!("Goal")), false),
        ("a base that is not a SHA-256", with("base", json!("5d1c0a9e8f7b6c5d4e3f2a1b0c9d8e7f6a5b4c3d")), false),
        ("an empty title", json!({ "type": "item-edit", "item": edit["item"], "section": null, "text": " ", "base": edit["base"] }), false),
        ("a title past its length", json!({ "type": "item-edit", "item": edit["item"], "section": null, "text": "x".repeat(crate::MAX_ITEM_TITLE_CHARS + 1), "base": edit["base"] }), false),
        ("an edit without its base", json!({ "type": "item-edit", "item": edit["item"], "section": "goal", "text": "x" }), false),
        ("an edit of an item by a path on the machine", with("item", json!("vault:/Users/someone/vault/Projects/x.md")), false),
    ];
    let edits: Vec<Value> = edits
        .into_iter()
        .map(|(name, payload, valid)| {
            let parsed = serde_json::from_value::<Payload>(payload.clone());
            assert_eq!(parsed.is_ok_and(|payload| payload.validate().is_ok()), valid, "{name}");
            json!({ "name": name, "payload": payload, "valid": valid })
        })
        .collect();
    let body = serde_json::to_value(backlog_body()).unwrap();
    let change = |pointer: &str, value: Value| {
        let mut changed = body.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        changed.pointer_mut(parent).unwrap()[key] = value;
        changed
    };
    let mut twice = body.clone();
    twice["sections"].as_array_mut().unwrap().push(body["sections"][0].clone());
    let bodies = [
        ("the body", body.clone(), true),
        ("questions after the last section", change("/questions_key", json!(null)), true),
        ("no sections", json!({ "sections": [], "questions": [], "questions_key": null }), true),
        ("questions under a section it does not have", change("/questions_key", json!("risks")), false),
        ("a section listed twice", twice, false),
        ("a key with a space", change("/sections/0/key", json!("what we know")), false),
        ("an empty heading", change("/sections/0/heading", json!(" ")), false),
        ("a heading on two lines", change("/sections/0/heading", json!("Back\nground")), false),
        ("a section without required", { let mut changed = body.clone(); changed["sections"][0].as_object_mut().unwrap().remove("required"); changed }, false),
        ("the fixed fields of before", json!({ "background": "", "goal": "", "what_we_know": "", "questions": [], "out_of_scope": "", "source": "" }), false),
    ];
    let bodies: Vec<Value> = bodies
        .into_iter()
        .map(|(name, body, valid)| {
            assert_eq!(serde_json::from_value::<BacklogBody>(body.clone()).is_ok(), valid, "{name}");
            json!({ "name": name, "body": body, "valid": valid })
        })
        .collect();
    json!({
        "media_type": BACKLOG_BODY_MEDIA_TYPE,
        "max_label_bytes": crate::MAX_LABEL_BYTES,
        "max_idea_chars": crate::MAX_IDEA_CHARS,
        "max_item_answer_chars": crate::MAX_ITEM_ANSWER_CHARS,
        "max_item_title_chars": crate::MAX_ITEM_TITLE_CHARS,
        "max_section_chars": crate::MAX_SECTION_CHARS,
        "max_item_edit_bytes": crate::MAX_ITEM_EDIT_BYTES,
        "max_heading_chars": crate::MAX_HEADING_CHARS,
        "text_hashes": hashes,
        "edits": edits,
        "bodies": bodies,
        "body": backlog_body(),
        "blob": blob,
        "ciphertext": b64(&ciphertext),
        "item_keys": keys,
    })
}

fn review_json() -> Value {
    let (blob, ciphertext) = review_blob();
    let comment = json!({ "path": "src/orders.rs", "line": 41, "side": "old", "sha": HEAD, "text": "Why the cast?" });
    let comments = |list: Value| json!({ "type": "review-comments", "address": REVIEWED, "step": 2, "comments": list });
    let with = |key: &str, value: Value| {
        let mut changed = comment.clone();
        changed[key] = value;
        comments(json!([changed]))
    };
    let payloads = [
        ("comments on old and new lines", review_comments_json(), true),
        ("a comment with neither step nor sha", json!({ "type": "review-comments", "address": REVIEWED, "comments": [{ "path": "src/gateway.rs", "line": 1, "side": "new", "text": "Fine." }] }), true),
        ("no comments", comments(json!([])), false),
        ("one comment too many", comments(json!(vec![comment.clone(); MAX_REVIEW_COMMENTS + 1])), false),
        ("an empty comment", with("text", json!("  ")), false),
        ("a comment past its length", with("text", json!("x".repeat(MAX_REVIEW_COMMENT_CHARS + 1))), false),
        ("a comment holding a terminal escape", with("text", json!("Clear it\u{1b}[2J")), false),
        ("a comment over two lines with a tab", with("text", json!("Why the cast?\n\tIt drops cents.")), true),
        ("comments too large to send together", comments(json!(vec![{ let mut big = comment.clone(); big["text"] = json!("€".repeat(MAX_REVIEW_COMMENT_CHARS)); big }; 5])), false),
        ("line zero", with("line", json!(0)), false),
        ("a side that is neither old nor new", with("side", json!("both")), false),
        ("a short sha", with("sha", json!("9f3c2a1")), false),
        ("an absolute path", with("path", json!("/Users/someone/api/src/orders.rs")), false),
        ("a path out of the repo", with("path", json!("../secrets.env")), false),
        ("a field kebbie does not know", with("range", json!([40, 42])), false),
        ("a read mark", serde_json::to_value(review_read()).unwrap(), true),
        ("a read mark on a 64-character object id", json!({ "type": "review-read", "address": REVIEWED, "path": "src/gateway.rs", "read": false, "content": "a".repeat(64) }), true),
        ("a read mark on a short hash", json!({ "type": "review-read", "address": REVIEWED, "path": "src/gateway.rs", "read": true, "content": "5d1c0a9" }), false),
        ("a read mark without its content", json!({ "type": "review-read", "address": REVIEWED, "path": "src/gateway.rs", "read": true }), false),
        ("an accept", serde_json::to_value(accept_step()).unwrap(), true),
        ("an accept of step zero", json!({ "type": "accept-step", "address": REVIEWED, "step": 0, "sha": HEAD }), false),
        ("an accept of a short sha", json!({ "type": "accept-step", "address": REVIEWED, "step": 2, "sha": "9f3c2a1" }), false),
        ("a review asked for", serde_json::to_value(review_open()).unwrap(), true),
        ("a review asked for a task without its repo", json!({ "type": "review-open", "address": "payments" }), false),
    ];
    let payloads: Vec<Value> = payloads
        .into_iter()
        .map(|(name, payload, valid)| {
            let parsed = serde_json::from_value::<Payload>(payload.clone());
            assert_eq!(parsed.is_ok_and(|payload| payload.validate().is_ok()), valid, "{name}");
            json!({ "name": name, "payload": payload, "valid": valid })
        })
        .collect();
    let body = serde_json::to_value(review()).unwrap();
    let change = |pointer: &str, value: Value| {
        let mut changed = body.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        changed.pointer_mut(parent).unwrap()[key] = value;
        changed
    };
    let bodies = [
        ("the review", body.clone(), true),
        ("an absolute path", change("/files/0/path", json!("/Users/someone/api/src/gateway.rs")), false),
        ("counts that differ from the lines", change("/files/1/add", json!(2)), false),
        ("an added line with an old number", change("/files/0/hunks/0/lines/0/old", json!(1)), false),
        ("an omitted file with hunks", change("/files/0/omitted", json!(true)), false),
        ("a short head", change("/head", json!("9f3c2a1")), false),
        ("a content hash in capitals", change("/files/1/content", json!("0A1B2C3D4E5F60718293A4B5C6D7E8F901234567")), false),
        ("a renamed file without its old path", change("/files/1/status", json!("renamed")), false),
    ];
    let bodies: Vec<Value> = bodies
        .into_iter()
        .map(|(name, body, valid)| {
            let parsed = serde_json::from_value::<Review>(body.clone());
            assert_eq!(parsed.is_ok_and(|review| review.check().is_ok()), valid, "{name}");
            json!({ "name": name, "body": body, "valid": valid })
        })
        .collect();
    json!({
        "media_type": REVIEW_MEDIA_TYPE,
        "max_comments": MAX_REVIEW_COMMENTS,
        "max_comment_chars": MAX_REVIEW_COMMENT_CHARS,
        "max_comments_bytes": MAX_REVIEW_COMMENTS_BYTES,
        "review": review(),
        "blob": blob,
        "ciphertext": b64(&ciphertext),
        "payloads": payloads,
        "bodies": bodies,
    })
}

fn review_comments_json() -> Value {
    serde_json::to_value(review_comments()).unwrap()
}

fn blobs_json() -> Value {
    let sealed = |label: &str| {
        seal_blob_with(
            SAMPLE_PLAN.as_bytes(),
            "text/markdown",
            seed(&format!("blob/{label}/key")),
            seed(&format!("blob/{label}/nonce"))[..12].try_into().unwrap(),
        )
    };
    let (blob, ciphertext) = sealed("plan");
    let (other, _) = sealed("other");
    let mut flipped = ciphertext.clone();
    flipped[20] ^= 1;
    let case = |name: &str, blob: &BlobRef, ciphertext: &[u8], error: Option<&str>| {
        json!({ "name": name, "blob": blob, "ciphertext": b64(ciphertext), "error": error })
    };
    json!({
        "domain": String::from_utf8(BLOB_DOMAIN.to_vec()).unwrap(),
        "plaintext": SAMPLE_PLAN,
        "cases": [
            case("opens", &blob, &ciphertext, None),
            case("a flipped byte no longer matches the id", &blob, &flipped, Some("wrong-id")),
            case("another blob's key does not open it", &BlobRef { key: other.key.clone(), ..blob.clone() }, &ciphertext, Some("undecryptable")),
            case("a size that differs from the ciphertext", &BlobRef { bytes: 3, ..blob.clone() }, &ciphertext, Some("wrong-size")),
        ],
    })
}

fn notices_json() -> Value {
    let tags = ["q-0fc378cba90d48b44e5fe170", "p-1a2b", "d-1a2b", "i-1a2b", "r-a94051d90047441d5ac99a97", "c-1a2b", "s-1a2b", "q-", "x-1a2b", "Q-1a2b", "q", "question-1", "-q", "rq-1"];
    let fits = [
        (NoticeCategory::Questions, "q-1a2b", true),
        (NoticeCategory::Questions, "p-1a2b", true),
        (NoticeCategory::Ready, "q-1a2b", false),
        (NoticeCategory::NewDevice, "r-1a2b", false),
        (NoticeCategory::Ready, "ready-1", true),
        (NoticeCategory::Started, "q", true),
    ];
    let notice_of = |category| Notice { category, title: "refunds · shop".into(), body: "Ready to push.".into(), open: Target::Task { address: "refunds@shop".into() }, retract: Vec::new() };
    json!({
        "prefixes": NoticeCategory::COLLAPSE_PREFIXES.iter().map(|(prefix, category)| json!({ "prefix": prefix, "category": category })).collect::<Vec<_>>(),
        "tags": tags.iter().map(|tag| json!({ "tag": tag, "category": NoticeCategory::of_collapse(tag) })).collect::<Vec<_>>(),
        "fits": fits.iter().map(|(category, tag, fits)| {
            assert_eq!(notice_of(*category).fits_collapse(tag), *fits, "{tag}");
            json!({ "category": category, "tag": tag, "fits": fits })
        }).collect::<Vec<_>>(),
    })
}

pub fn render() -> Vec<(&'static str, String)> {
    let parties = Parties::new();
    [
        ("keys.json", keys_json(&parties)),
        ("envelopes.json", envelopes_json(&parties)),
        ("authorize.json", authorize_json(&parties)),
        ("signatures.json", signatures_json(&parties)),
        ("connect.json", connect_json(&parties)),
        ("accounts.json", accounts_json(&parties)),
        ("devices.json", devices_json(&parties)),
        ("blobs.json", blobs_json()),
        ("backlog.json", backlog_json()),
        ("review.json", review_json()),
        ("notices.json", notices_json()),
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
                "vectors/{name} is stale: cargo run -p kebbie-hub-protocol --features test-vectors --bin vectors"
            );
        }
    }
}
