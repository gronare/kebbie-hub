use serde::{Deserialize, Serialize};

use crate::backlog::ItemKey;
use crate::board::Board;
use crate::envelope::Kind;
use crate::ids::{Id, Prefix};

pub const MAX_PROMPT_CHARS: usize = 4_000;
pub const MAX_OPTIONS: u8 = 4;
pub const MAX_IDEA_CHARS: usize = 2_000;
pub const MAX_ITEM_ANSWER_CHARS: usize = 2_000;
const MAX_PROMPT_ID_CHARS: usize = 64;
const MAX_OPTION_KEY_CHARS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Payload {
    Board(Board),
    Notice(Notice),
    Result(CommandResult),
    Answer(Answer),
    Push(PushApproval),
    Start(StartTask),
    Routine(RunRoutine),
    PromptAnswer(PromptAnswer),
    Idea(Idea),
    ItemAnswer(ItemAnswer),
    StartItem(StartItem),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub event: Event,
    pub address: Option<String>,
    pub title: String,
    pub body: String,
    pub question: Option<String>,
    pub options: Vec<String>,
    pub recommend: Option<u8>,
    pub board_rev: u64,
    pub retract: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Event {
    NeedsYou,
    Ready,
    Question,
    Summary,
    CiFailed,
    CiGreen,
    ApiError,
    RoutineFindings,
    RoutineFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ResultFields", into = "ResultFields")]
pub struct CommandResult {
    pub command: Id,
    pub device: Id,
    pub outcome: Outcome,
    pub text: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultFields {
    command: Id,
    device: Id,
    outcome: OutcomeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<ReasonKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blocking: Option<u32>,
    text: String,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum OutcomeKind {
    Done,
    Refused,
    Failed,
}

impl TryFrom<ResultFields> for CommandResult {
    type Error = &'static str;

    fn try_from(fields: ResultFields) -> Result<CommandResult, &'static str> {
        let outcome = match (fields.outcome, fields.reason) {
            (OutcomeKind::Done, None) => Outcome::Done,
            (OutcomeKind::Failed, None) => Outcome::Failed,
            (OutcomeKind::Refused, Some(key)) => Outcome::Refused {
                reason: Reason::from_key(key, fields.blocking)?,
            },
            (OutcomeKind::Refused, None) => return Err("a refusal needs a reason"),
            (_, Some(_)) => return Err("only a refusal carries a reason"),
        };
        if fields.blocking.is_some() && !matches!(outcome, Outcome::Refused { reason: Reason::QuestionsOpen { .. } }) {
            return Err("only a refusal for open questions counts them");
        }
        if fields.device.prefix() != Prefix::Device {
            return Err("device is not a device id");
        }
        Ok(CommandResult {
            command: fields.command,
            device: fields.device,
            outcome,
            text: fields.text,
        })
    }
}

impl From<CommandResult> for ResultFields {
    fn from(result: CommandResult) -> ResultFields {
        let (outcome, reason) = match result.outcome {
            Outcome::Done => (OutcomeKind::Done, None),
            Outcome::Failed => (OutcomeKind::Failed, None),
            Outcome::Refused { reason } => (OutcomeKind::Refused, Some(reason)),
        };
        let blocking = match reason {
            Some(Reason::QuestionsOpen { blocking }) => Some(blocking),
            _ => None,
        };
        ResultFields {
            command: result.command,
            device: result.device,
            outcome,
            reason: reason.map(Reason::to_key),
            blocking,
            text: result.text,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Refused { reason: Reason },
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    AlreadySettled,
    HeadMoved,
    UnknownRepo,
    NotAllowed,
    Expired,
    Undecryptable,
    BadRequest,
    NotApproved,
    Denied,
    UnknownItem,
    QuestionChanged,
    QuestionsOpen { blocking: u32 },
    AlreadyStarted,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ReasonKey {
    AlreadySettled,
    HeadMoved,
    UnknownRepo,
    NotAllowed,
    Expired,
    Undecryptable,
    BadRequest,
    NotApproved,
    Denied,
    UnknownItem,
    QuestionChanged,
    QuestionsOpen,
    AlreadyStarted,
}

impl Reason {
    pub fn key(self) -> &'static str {
        match self {
            Reason::AlreadySettled => "already-settled",
            Reason::HeadMoved => "head-moved",
            Reason::UnknownRepo => "unknown-repo",
            Reason::NotAllowed => "not-allowed",
            Reason::Expired => "expired",
            Reason::Undecryptable => "undecryptable",
            Reason::BadRequest => "bad-request",
            Reason::NotApproved => "not-approved",
            Reason::Denied => "denied",
            Reason::UnknownItem => "unknown-item",
            Reason::QuestionChanged => "question-changed",
            Reason::QuestionsOpen { .. } => "questions-open",
            Reason::AlreadyStarted => "already-started",
        }
    }

    fn to_key(self) -> ReasonKey {
        match self {
            Reason::AlreadySettled => ReasonKey::AlreadySettled,
            Reason::HeadMoved => ReasonKey::HeadMoved,
            Reason::UnknownRepo => ReasonKey::UnknownRepo,
            Reason::NotAllowed => ReasonKey::NotAllowed,
            Reason::Expired => ReasonKey::Expired,
            Reason::Undecryptable => ReasonKey::Undecryptable,
            Reason::BadRequest => ReasonKey::BadRequest,
            Reason::NotApproved => ReasonKey::NotApproved,
            Reason::Denied => ReasonKey::Denied,
            Reason::UnknownItem => ReasonKey::UnknownItem,
            Reason::QuestionChanged => ReasonKey::QuestionChanged,
            Reason::QuestionsOpen { .. } => ReasonKey::QuestionsOpen,
            Reason::AlreadyStarted => ReasonKey::AlreadyStarted,
        }
    }

    fn from_key(key: ReasonKey, blocking: Option<u32>) -> Result<Reason, &'static str> {
        Ok(match key {
            ReasonKey::AlreadySettled => Reason::AlreadySettled,
            ReasonKey::HeadMoved => Reason::HeadMoved,
            ReasonKey::UnknownRepo => Reason::UnknownRepo,
            ReasonKey::NotAllowed => Reason::NotAllowed,
            ReasonKey::Expired => Reason::Expired,
            ReasonKey::Undecryptable => Reason::Undecryptable,
            ReasonKey::BadRequest => Reason::BadRequest,
            ReasonKey::NotApproved => Reason::NotApproved,
            ReasonKey::Denied => Reason::Denied,
            ReasonKey::UnknownItem => Reason::UnknownItem,
            ReasonKey::QuestionChanged => Reason::QuestionChanged,
            ReasonKey::QuestionsOpen => match blocking {
                Some(blocking) if blocking > 0 => Reason::QuestionsOpen { blocking },
                _ => return Err("a refusal for open questions counts at least one"),
            },
            ReasonKey::AlreadyStarted => Reason::AlreadyStarted,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub address: String,
    pub question: String,
    pub choice: Option<u8>,
    pub comment: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushApproval {
    pub address: String,
    pub head: String,
    pub card_sha: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartTask {
    pub repo: String,
    pub name: String,
    pub prompt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRoutine {
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptAnswer {
    pub address: String,
    pub prompt: String,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Idea {
    pub text: String,
    #[serde(default)]
    pub repo: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemAnswer {
    pub item: ItemKey,
    pub index: u32,
    pub question: String,
    pub answer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartItem {
    pub item: ItemKey,
    #[serde(default)]
    pub anyway: bool,
}

impl Payload {
    pub fn kind(&self) -> Kind {
        match self {
            Payload::Board(_) => Kind::Board,
            Payload::Notice(_) => Kind::Notice,
            Payload::Result(_) => Kind::Result,
            Payload::Answer(_) => Kind::Answer,
            Payload::Push(_) => Kind::Push,
            Payload::Start(_) => Kind::Start,
            Payload::Routine(_) => Kind::Routine,
            Payload::PromptAnswer(_) => Kind::PromptAnswer,
            Payload::Idea(_) => Kind::Idea,
            Payload::ItemAnswer(_) => Kind::ItemAnswer,
            Payload::StartItem(_) => Kind::StartItem,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Payload::Answer(answer) => {
                check(is_address(&answer.address), "address is not task@repo")?;
                check(!answer.question.is_empty(), "the question id is empty")?;
                match answer.choice {
                    Some(choice) => check(choice < MAX_OPTIONS, "choice is past the last option"),
                    None => check(
                        !answer.comment.trim().is_empty(),
                        "a free answer needs a comment",
                    ),
                }
            }
            Payload::Push(push) => {
                check(is_address(&push.address), "address is not task@repo")?;
                check(is_full_sha(&push.head), "head is not a full sha")?;
                check(
                    push.card_sha.as_deref().is_none_or(is_full_sha),
                    "card_sha is not a full sha",
                )
            }
            Payload::Start(start) => {
                check(is_token(&start.repo), "repo is not a plain name")?;
                check(is_token(&start.name), "name is not a plain name")?;
                check(!start.prompt.trim().is_empty(), "the prompt is empty")?;
                check(
                    start.prompt.chars().count() <= MAX_PROMPT_CHARS,
                    "the prompt is too long",
                )
            }
            Payload::Routine(routine) => {
                check(is_token(&routine.id), "routine id is not a plain name")
            }
            Payload::PromptAnswer(answer) => {
                check(is_address(&answer.address), "address is not task@repo")?;
                check(is_word(&answer.prompt, MAX_PROMPT_ID_CHARS), "prompt is not a prompt id")?;
                check(is_word(&answer.key, MAX_OPTION_KEY_CHARS), "key is not an option's key")
            }
            Payload::Idea(idea) => {
                check(is_text(&idea.text, MAX_IDEA_CHARS), "the idea is empty or too long")?;
                check(idea.repo.as_deref().is_none_or(is_token), "repo is not a plain name")
            }
            Payload::ItemAnswer(answer) => {
                check(is_text(&answer.question, MAX_PROMPT_CHARS), "the question is empty or too long")?;
                check(is_text(&answer.answer, MAX_ITEM_ANSWER_CHARS), "the answer is empty or too long")
            }
            Payload::StartItem(_) | Payload::Board(_) | Payload::Notice(_) | Payload::Result(_) => Ok(()),
        }
    }
}

fn check(holds: bool, problem: &'static str) -> Result<(), &'static str> {
    if holds { Ok(()) } else { Err(problem) }
}

fn is_token(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

fn is_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max
}

fn is_word(text: &str, max: usize) -> bool {
    (1..=max).contains(&text.len())
        && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn is_address(text: &str) -> bool {
    text.split_once('@')
        .is_some_and(|(task, repo)| is_token(task) && is_token(repo))
}

fn is_full_sha(text: &str) -> bool {
    text.len() == 40
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_outcome_sits_beside_the_command() {
        let result = Payload::Result(CommandResult {
            command: Id::command(1, [1; 10]),
            device: Id::ulid(Prefix::Device, 1, [2; 10]).unwrap(),
            outcome: Outcome::Refused {
                reason: Reason::HeadMoved,
            },
            text: "HEAD moved".to_string(),
        });
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(
            (
                json["type"].as_str(),
                json["outcome"].as_str(),
                json["reason"].as_str()
            ),
            (Some("result"), Some("refused"), Some("head-moved"))
        );
        assert_eq!(serde_json::from_value::<Payload>(json.clone()).unwrap(), result);
        let mut from_account = json;
        from_account["device"] = serde_json::json!(Id::account([3; 16]));
        assert!(serde_json::from_value::<Payload>(from_account).is_err(), "a result names the device that sent the command");
    }

    #[test]
    fn commands_refuse_unknown_fields() {
        let json = serde_json::json!({ "type": "routine", "id": "nightly-deps", "mode": "fix" });
        assert!(serde_json::from_value::<Payload>(json).is_err());
    }

    #[test]
    fn validate_holds_the_limits() {
        let answer = |choice, comment: &str| {
            Payload::Answer(Answer {
                address: "refunds@shop".into(),
                question: "q1".into(),
                choice,
                comment: comment.into(),
            })
        };
        assert_eq!(answer(Some(3), "").validate(), Ok(()));
        assert!(answer(Some(4), "").validate().is_err());
        assert!(answer(None, " ").validate().is_err());
        let start = |prompt: String| {
            Payload::Start(StartTask {
                repo: "shop".into(),
                name: "invoice-rounding".into(),
                prompt,
            })
        };
        assert_eq!(start("x".repeat(MAX_PROMPT_CHARS)).validate(), Ok(()));
        assert!(start("x".repeat(MAX_PROMPT_CHARS + 1)).validate().is_err());
        let push = Payload::Push(PushApproval {
            address: "refunds@shop".into(),
            head: "9f3c2a1".into(),
            card_sha: None,
        });
        assert!(push.validate().is_err());
        let prompt = |prompt: &str, key: &str| {
            Payload::PromptAnswer(PromptAnswer {
                address: "refunds@shop".into(),
                prompt: prompt.into(),
                key: key.into(),
            })
        };
        assert_eq!(prompt("3f9a1c2e", "esc").validate(), Ok(()));
        assert_eq!(prompt("3f9a1c2e", "1").validate(), Ok(()));
        assert!(prompt("", "1").validate().is_err());
        assert!(prompt("3f9a1c2e", "").validate().is_err());
        assert!(prompt("3f9a1c2e", "y\n").validate().is_err(), "a key is never keystrokes of its own");
        assert!(prompt("3f9a1c2e", &"y".repeat(17)).validate().is_err());
    }

    #[test]
    fn a_refusal_for_open_questions_counts_them_and_no_other_result_does() {
        let refused = |reason| {
            Payload::Result(CommandResult {
                command: Id::command(1, [1; 10]),
                device: Id::ulid(Prefix::Device, 1, [2; 10]).unwrap(),
                outcome: Outcome::Refused { reason },
                text: "Not started".into(),
            })
        };
        let open = refused(Reason::QuestionsOpen { blocking: 2 });
        let json = serde_json::to_value(&open).unwrap();
        assert_eq!((json["reason"].as_str(), json["blocking"].as_u64()), (Some("questions-open"), Some(2)));
        assert_eq!(serde_json::from_value::<Payload>(json.clone()).unwrap(), open);
        let started = serde_json::to_value(refused(Reason::AlreadyStarted)).unwrap();
        assert_eq!((started["reason"].as_str(), started.get("blocking")), (Some("already-started"), None));
        for (key, value) in [("blocking", serde_json::json!(0)), ("blocking", serde_json::json!(null)), ("reason", serde_json::json!("already-started"))] {
            let mut wrong = json.clone();
            wrong[key] = value;
            assert!(serde_json::from_value::<Payload>(wrong.clone()).is_err(), "{wrong}");
        }
    }

    #[test]
    fn backlog_commands_hold_their_limits_and_lifetimes() {
        let idea = |text: String, repo: Option<&str>| Payload::Idea(Idea { text, repo: repo.map(Into::into) });
        assert_eq!(idea("x".repeat(MAX_IDEA_CHARS), None).validate(), Ok(()));
        assert!(idea("x".repeat(MAX_IDEA_CHARS + 1), None).validate().is_err());
        assert!(idea(" ".into(), None).validate().is_err());
        assert!(idea("x".into(), Some("../shop")).validate().is_err());
        let bare = serde_json::json!({ "type": "idea", "text": "Mail a weekly summary" });
        assert_eq!(serde_json::from_value::<Payload>(bare).unwrap(), idea("Mail a weekly summary".into(), None));
        let answer = |answer: &str| {
            Payload::ItemAnswer(ItemAnswer { item: ItemKey::parse("kept:idea-1").unwrap(), index: 0, question: "Which?".into(), answer: answer.into() })
        };
        assert_eq!(answer("This one").validate(), Ok(()));
        assert!(answer("").validate().is_err());
        assert!(answer(&"x".repeat(MAX_ITEM_ANSWER_CHARS + 1)).validate().is_err());
        let plain = serde_json::json!({ "type": "start-item", "item": "kept:idea-1" });
        assert_eq!(serde_json::from_value::<Payload>(plain).unwrap(), Payload::StartItem(StartItem { item: ItemKey::parse("kept:idea-1").unwrap(), anyway: false }));
        let path = serde_json::json!({ "type": "start-item", "item": "local:../../etc" });
        assert!(serde_json::from_value::<Payload>(path).is_err());
        assert_eq!(
            [Kind::Idea, Kind::ItemAnswer, Kind::StartItem].map(|kind| (kind.is_command(), kind.default_ttl(), kind.max_ttl())),
            [(true, 3_600, 3_600), (true, 3_600, 3_600), (true, 86_400, 86_400)]
        );
    }

    #[test]
    fn a_prompt_answer_lives_ten_minutes_at_most() {
        assert_eq!(
            (Kind::PromptAnswer.default_ttl(), Kind::PromptAnswer.max_ttl()),
            (600, 600)
        );
        let json = serde_json::json!({ "type": "prompt-answer", "address": "refunds@shop", "prompt": "3f9a1c2e", "key": "y" });
        assert_eq!(
            serde_json::from_value::<Payload>(json).unwrap().kind(),
            Kind::PromptAnswer
        );
    }
}
