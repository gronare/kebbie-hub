use serde::{Deserialize, Serialize};

use crate::board::Board;
use crate::envelope::Kind;
use crate::ids::Id;

pub const MAX_PROMPT_CHARS: usize = 4_000;
pub const MAX_OPTIONS: u8 = 4;

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
    pub outcome: Outcome,
    pub text: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultFields {
    command: Id,
    outcome: OutcomeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<Reason>,
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
            (OutcomeKind::Refused, Some(reason)) => Outcome::Refused { reason },
            (OutcomeKind::Refused, None) => return Err("a refusal needs a reason"),
            (_, Some(_)) => return Err("only a refusal carries a reason"),
        };
        Ok(CommandResult {
            command: fields.command,
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
        ResultFields {
            command: result.command,
            outcome,
            reason,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    AlreadySettled,
    HeadMoved,
    UnknownRepo,
    NotAllowed,
    Expired,
    Undecryptable,
    BadRequest,
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
        }
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
            Payload::Board(_) | Payload::Notice(_) | Payload::Result(_) => Ok(()),
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
        assert_eq!(serde_json::from_value::<Payload>(json).unwrap(), result);
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
                address: "refunds@greenhouse".into(),
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
                repo: "greenhouse".into(),
                name: "invoice-rounding".into(),
                prompt,
            })
        };
        assert_eq!(start("x".repeat(MAX_PROMPT_CHARS)).validate(), Ok(()));
        assert!(start("x".repeat(MAX_PROMPT_CHARS + 1)).validate().is_err());
        let push = Payload::Push(PushApproval {
            address: "refunds@greenhouse".into(),
            head: "9f3c2a1".into(),
            card_sha: None,
        });
        assert!(push.validate().is_err());
    }
}
