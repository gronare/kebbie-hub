use serde::{Deserialize, Serialize};

use crate::backlog::{HomeKind, ItemKey, is_section_key, is_text_hash};
use crate::board::{BlobRef, Board};
use crate::envelope::{Kind, MAX_COLLAPSE_CHARS};
use crate::ids::{Id, Prefix};
use crate::review::{AcceptStep, AcceptSubtask, ReviewComments, ReviewOpen, ReviewRead, ReviewSend};

pub const MAX_PROMPT_CHARS: usize = 4_000;
pub const MAX_OPTIONS: u8 = 4;
pub const MAX_IDEA_CHARS: usize = 2_000;
pub const MAX_ITEM_ANSWER_CHARS: usize = 2_000;
pub const MAX_ITEM_TITLE_CHARS: usize = 200;
pub const MAX_SECTION_CHARS: usize = 8_000;
pub const MAX_ITEM_EDIT_BYTES: usize = 24 * 1024;
pub const MAX_NOTICE_TITLE_CHARS: usize = 120;
pub const MAX_NOTICE_BODY_CHARS: usize = 600;
pub const MAX_NOTICE_BYTES: usize = 2_048;
pub const MAX_RETRACTS: usize = 8;
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
    ItemEdit(ItemEdit),
    DeviceDecision(DeviceDecision),
    ReviewOpen(ReviewOpen),
    ReviewComments(ReviewComments),
    ReviewRead(ReviewRead),
    AcceptStep(AcceptStep),
    AcceptSubtask(AcceptSubtask),
    FoundStart(FoundStart),
    FoundAdd(FoundAdd),
    FoundDismiss(FoundDismiss),
    FoundKeep(FoundKeep),
    ReviewSend(ReviewSend),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub category: NoticeCategory,
    pub title: String,
    pub body: String,
    pub open: Target,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retract: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoticeCategory {
    Questions,
    NewDevice,
    IdeaQuestions,
    Ready,
    CiFailed,
    Started,
}

impl NoticeCategory {
    pub const ALL: [NoticeCategory; 6] = [
        NoticeCategory::Questions,
        NoticeCategory::NewDevice,
        NoticeCategory::IdeaQuestions,
        NoticeCategory::Ready,
        NoticeCategory::CiFailed,
        NoticeCategory::Started,
    ];

    pub fn name(self) -> &'static str {
        match self {
            NoticeCategory::Questions => "questions",
            NoticeCategory::NewDevice => "new-device",
            NoticeCategory::IdeaQuestions => "idea-questions",
            NoticeCategory::Ready => "ready",
            NoticeCategory::CiFailed => "ci-failed",
            NoticeCategory::Started => "started",
        }
    }

    pub fn on_by_default(self) -> bool {
        self != NoticeCategory::Started
    }

    pub fn defaults() -> Vec<NoticeCategory> {
        NoticeCategory::ALL
            .into_iter()
            .filter(|category| category.on_by_default())
            .collect()
    }

    /// A collapse tag that starts with one of these and a hyphen names its notice's category, so a device that cannot open a notice still says what it is about.
    pub const COLLAPSE_PREFIXES: [(&'static str, NoticeCategory); 7] = [
        ("q", NoticeCategory::Questions),
        ("p", NoticeCategory::Questions),
        ("d", NoticeCategory::NewDevice),
        ("i", NoticeCategory::IdeaQuestions),
        ("r", NoticeCategory::Ready),
        ("c", NoticeCategory::CiFailed),
        ("s", NoticeCategory::Started),
    ];

    pub fn of_collapse(tag: &str) -> Option<NoticeCategory> {
        let (prefix, _) = tag.split_once('-')?;
        NoticeCategory::COLLAPSE_PREFIXES
            .iter()
            .find(|(named, _)| *named == prefix)
            .map(|(_, category)| *category)
    }
}

impl Notice {
    pub fn fits_collapse(&self, tag: &str) -> bool {
        NoticeCategory::of_collapse(tag).is_none_or(|named| named == self.category)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "to", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Target {
    Task { address: String },
    Item { item: ItemKey },
    Device { device: Id },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceDecision {
    pub device: Id,
    pub fingerprint: String,
    pub approve: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ResultFields", into = "ResultFields")]
pub struct CommandResult {
    pub command: Id,
    pub device: Id,
    pub outcome: Outcome,
    pub text: String,
    pub blob: Option<BlobRef>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blob: Option<BlobRef>,
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
        if fields.blob.is_some() && outcome != Outcome::Done {
            return Err("only a result that is done names a blob");
        }
        Ok(CommandResult {
            command: fields.command,
            device: fields.device,
            outcome,
            text: fields.text,
            blob: fields.blob,
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
            blob: result.blob,
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
    UnknownDevice,
    SectionChanged,
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
    UnknownDevice,
    SectionChanged,
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
            Reason::UnknownDevice => "unknown-device",
            Reason::SectionChanged => "section-changed",
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
            Reason::UnknownDevice => ReasonKey::UnknownDevice,
            Reason::SectionChanged => ReasonKey::SectionChanged,
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
            ReasonKey::UnknownDevice => Reason::UnknownDevice,
            ReasonKey::SectionChanged => Reason::SectionChanged,
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

/// Pushes the task's push order from the top through `head`: the task's HEAD, or the last commit of its pushable run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "PushWire")]
pub struct PushApproval {
    pub address: String,
    pub head: String,
    pub card_sha: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PushWire {
    address: String,
    head: String,
    card_sha: Option<String>,
    #[serde(default)]
    through_step: Option<u32>,
}

impl TryFrom<PushWire> for PushApproval {
    type Error = &'static str;

    fn try_from(wire: PushWire) -> Result<Self, Self::Error> {
        check(wire.through_step != Some(0), "steps count from one")?;
        Ok(PushApproval { address: wire.address, head: wire.head, card_sha: wire.card_sha })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoundStart {
    pub item: ItemKey,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoundAdd {
    pub item: ItemKey,
    pub task: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoundDismiss {
    pub item: ItemKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoundKeep {
    pub item: ItemKey,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemEdit {
    pub item: ItemKey,
    /// The body section to replace; none replaces the item's title.
    pub section: Option<String>,
    pub text: String,
    /// The text_hash of the text this edit was made on.
    pub base: String,
}

impl ItemEdit {
    pub fn check(&self) -> Result<(), &'static str> {
        check(is_text_hash(&self.base), "base is not a hex SHA-256")?;
        match &self.section {
            None => check(is_line(&self.text, MAX_ITEM_TITLE_CHARS), "a title is empty, too long or not one line")?,
            Some(key) => {
                check(is_section_key(key), "section is not a section key")?;
                check(self.text.chars().count() <= MAX_SECTION_CHARS, "the section is too long")?;
                check(!self.text.chars().any(|char| char.is_control() && !matches!(char, '\n' | '\t')), "the section holds control characters")?;
            }
        }
        check(serde_json::to_vec(self).expect("an edit always serializes").len() <= MAX_ITEM_EDIT_BYTES, "the edit is too large to send")
    }
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
            Payload::ItemEdit(_) => Kind::ItemEdit,
            Payload::DeviceDecision(_) => Kind::DeviceDecision,
            Payload::ReviewOpen(_) => Kind::ReviewOpen,
            Payload::ReviewComments(_) => Kind::ReviewComments,
            Payload::ReviewRead(_) => Kind::ReviewRead,
            Payload::AcceptStep(_) => Kind::AcceptStep,
            Payload::AcceptSubtask(_) => Kind::AcceptSubtask,
            Payload::FoundStart(_) => Kind::FoundStart,
            Payload::FoundAdd(_) => Kind::FoundAdd,
            Payload::FoundDismiss(_) => Kind::FoundDismiss,
            Payload::FoundKeep(_) => Kind::FoundKeep,
            Payload::ReviewSend(_) => Kind::ReviewSend,
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
            Payload::Notice(notice) => {
                check(is_line(&notice.title, MAX_NOTICE_TITLE_CHARS), "the title is empty, too long or not one line")?;
                check(is_text(&notice.body, MAX_NOTICE_BODY_CHARS) && !notice.body.chars().any(|char| char.is_control() && char != '\n'), "the body is empty, too long or holds control characters")?;
                check(notice.retract.len() <= MAX_RETRACTS, "too many notices to retract")?;
                check(notice.retract.iter().all(|tag| is_collapse(tag)), "a retract is not a collapse tag")?;
                match &notice.open {
                    Target::Task { address } => check(is_address(address), "address is not task@repo")?,
                    Target::Device { device } => check(device.prefix() == Prefix::Device, "device is not a device id")?,
                    Target::Item { .. } => {}
                }
                check(
                    serde_json::to_vec(self).expect("a payload always serializes").len() <= MAX_NOTICE_BYTES,
                    "the notice is too large to push",
                )
            }
            Payload::DeviceDecision(decision) => {
                check(decision.device.prefix() == Prefix::Device, "device is not a device id")?;
                check(is_fingerprint(&decision.fingerprint), "fingerprint is not a device fingerprint")
            }
            Payload::ItemEdit(edit) => edit.check(),
            Payload::ReviewOpen(open) => open.check(),
            Payload::ReviewComments(comments) => comments.check(),
            Payload::ReviewRead(read) => read.check(),
            Payload::AcceptStep(accept) => accept.check(),
            Payload::AcceptSubtask(accept) => accept.check(),
            Payload::FoundStart(start) => {
                check(start.item.home() == HomeKind::Found, "the item is not one tasks found")?;
                check(start.name.as_deref().is_none_or(is_token), "name is not a plain name")
            }
            Payload::FoundAdd(add) => {
                check(add.item.home() == HomeKind::Found, "the item is not one tasks found")?;
                check(is_address(&add.task), "task is not task@repo")
            }
            Payload::FoundDismiss(dismiss) => check(dismiss.item.home() == HomeKind::Found, "the item is not one tasks found"),
            Payload::FoundKeep(keep) => check(keep.item.home() == HomeKind::Found, "the item is not one tasks found"),
            Payload::ReviewSend(send) => send.check(),
            Payload::Board(board) => board.check(),
            Payload::StartItem(_) | Payload::Result(_) => Ok(()),
        }
    }
}

pub(crate) fn check(holds: bool, problem: &'static str) -> Result<(), &'static str> {
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

pub(crate) fn is_text(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max
}

pub(crate) fn is_line(text: &str, max: usize) -> bool {
    is_text(text, max) && !text.chars().any(char::is_control)
}

pub(crate) fn is_collapse(text: &str) -> bool {
    (1..=MAX_COLLAPSE_CHARS).contains(&text.len())
        && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn is_fingerprint(text: &str) -> bool {
    text.len() == 9
        && text.bytes().enumerate().all(|(index, byte)| {
            if index == 4 { byte == b'-' } else { matches!(byte, b'0'..=b'9' | b'A'..=b'F') }
        })
}

fn is_word(text: &str, max: usize) -> bool {
    (1..=max).contains(&text.len())
        && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

pub(crate) fn is_address(text: &str) -> bool {
    text.split_once('@')
        .is_some_and(|(task, repo)| is_token(task) && is_token(repo))
}

pub(crate) fn is_full_sha(text: &str) -> bool {
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
            blob: None,
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
        let push = |head: &str| Payload::Push(PushApproval { address: "refunds@shop".into(), head: head.into(), card_sha: None });
        assert!(push("9f3c2a1").validate().is_err());
        assert_eq!(push(&"a".repeat(40)).validate(), Ok(()));
        let older = serde_json::json!({ "type": "push", "address": "refunds@shop", "head": "a".repeat(40), "card_sha": null, "through_step": 2 });
        assert_eq!(serde_json::from_value::<Payload>(older.clone()).unwrap(), push(&"a".repeat(40)), "an app not yet updated names the step its head ends, and the head alone says how far");
        let mut zero = older;
        zero["through_step"] = serde_json::json!(0);
        assert!(serde_json::from_value::<Payload>(zero).is_err(), "steps count from one");
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
                blob: None,
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
    fn an_item_edit_replaces_a_section_or_the_title_from_a_base_and_lives_an_hour() {
        let edit = |section: Option<&str>, text: &str| Payload::ItemEdit(ItemEdit { item: ItemKey::parse("kept:idea-1").unwrap(), section: section.map(Into::into), text: text.into(), base: crate::text_hash("before") });
        assert_eq!(edit(Some("goal"), "New goal").validate(), Ok(()));
        assert_eq!(edit(Some("goal"), "").validate(), Ok(()), "only the template knows a section is required");
        assert_eq!(edit(None, "A new title").validate(), Ok(()));
        assert!(edit(None, "").validate().is_err());
        assert!(edit(None, "Two\nlines").validate().is_err());
        assert!(edit(Some("Goal"), "x").validate().is_err());
        assert!(edit(Some("goal"), &"x".repeat(MAX_SECTION_CHARS + 1)).validate().is_err());
        assert!(edit(Some("goal"), &"😀".repeat(MAX_SECTION_CHARS)).validate().is_err(), "the edit is too large to send");
        let Payload::ItemEdit(mut short) = edit(Some("goal"), "x") else { unreachable!() };
        short.base = "abc".into();
        assert!(Payload::ItemEdit(short).validate().is_err());
        assert_eq!((Kind::ItemEdit.name(), Kind::ItemEdit.is_command(), Kind::ItemEdit.default_ttl(), Kind::ItemEdit.max_ttl()), ("item-edit", true, 3_600, 3_600));
        assert_eq!(Reason::SectionChanged.key(), "section-changed");
    }

    #[test]
    fn a_notice_is_one_line_a_body_and_an_in_app_target_that_fits_a_push() {
        let notice = |title: &str, body: &str, open: Target, retract: Vec<String>| {
            Payload::Notice(Notice { category: NoticeCategory::Questions, title: title.into(), body: body.into(), open, retract })
        };
        let task = || Target::Task { address: "refunds@shop".into() };
        let plain = notice("refunds · shop", "Keep the old endpoint?\nThe lead says keep it.", task(), vec!["q-refunds".into()]);
        assert_eq!(plain.validate(), Ok(()));
        let json = serde_json::to_value(&plain).unwrap();
        assert_eq!((json["category"].as_str(), json["open"]["to"].as_str()), (Some("questions"), Some("task")));
        assert_eq!(serde_json::from_value::<Payload>(json).unwrap(), plain);
        assert!(notice("refunds\nshop", "x", task(), vec![]).validate().is_err());
        assert!(notice(" ", "x", task(), vec![]).validate().is_err());
        assert!(notice("x", "", task(), vec![]).validate().is_err());
        assert!(notice("x", "a\u{7}b", task(), vec![]).validate().is_err());
        assert!(notice(&"x".repeat(MAX_NOTICE_TITLE_CHARS + 1), "x", task(), vec![]).validate().is_err());
        assert!(notice("x", &"x".repeat(MAX_NOTICE_BODY_CHARS + 1), task(), vec![]).validate().is_err());
        assert!(notice("x", &"€".repeat(MAX_NOTICE_BODY_CHARS), task(), vec![]).validate().is_ok());
        assert!(notice("x", "x", task(), vec!["a".repeat(65)]).validate().is_err());
        assert!(notice("x", "x", task(), vec!["a b".into()]).validate().is_err());
        assert!(notice("x", "x", task(), vec!["a".into(); MAX_RETRACTS + 1]).validate().is_err());
        assert!(notice("x", "x", Target::Task { address: "refunds".into() }, vec![]).validate().is_err());
        assert!(notice("x", "x", Target::Device { device: Id::account([1; 16]) }, vec![]).validate().is_err());
        let big = notice(&"€".repeat(MAX_NOTICE_TITLE_CHARS), &"€".repeat(MAX_NOTICE_BODY_CHARS), task(), vec!["a".repeat(64); MAX_RETRACTS]);
        assert!(big.validate().is_err(), "the notice is too large to push");
        for open in [serde_json::json!({ "to": "url", "url": "https://example.com/" }), serde_json::json!({ "to": "task", "address": "refunds@shop", "url": "x" })] {
            let json = serde_json::json!({ "type": "notice", "category": "ready", "title": "x", "body": "x", "open": open });
            assert!(serde_json::from_value::<Payload>(json).is_err());
        }
    }

    #[test]
    fn categories_are_on_by_default_except_a_start_going_through() {
        assert_eq!(NoticeCategory::defaults().len(), NoticeCategory::ALL.len() - 1);
        assert!(!NoticeCategory::defaults().contains(&NoticeCategory::Started));
        assert_eq!(NoticeCategory::ALL.map(NoticeCategory::name), ["questions", "new-device", "idea-questions", "ready", "ci-failed", "started"]);
    }

    #[test]
    fn a_device_decision_names_a_device_by_id_and_fingerprint() {
        let decision = |device: Id, fingerprint: &str| Payload::DeviceDecision(DeviceDecision { device, fingerprint: fingerprint.into(), approve: true });
        let tablet = Id::ulid(Prefix::Device, 1, [2; 10]).unwrap();
        assert_eq!(decision(tablet.clone(), "0C11-5E9A").validate(), Ok(()));
        assert!(decision(tablet.clone(), "0c11-5e9a").validate().is_err());
        assert!(decision(tablet, "0C115E9A").validate().is_err());
        assert!(decision(Id::account([1; 16]), "0C11-5E9A").validate().is_err());
        assert_eq!((Kind::DeviceDecision.is_command(), Kind::DeviceDecision.default_ttl(), Kind::DeviceDecision.max_ttl()), (true, 3_600, 3_600));
        assert_eq!(serde_json::to_value(decision(Id::ulid(Prefix::Device, 1, [2; 10]).unwrap(), "0C11-5E9A")).unwrap()["type"], "device-decision");
    }

    #[test]
    fn review_commands_live_an_hour_and_only_a_result_that_is_done_names_a_blob() {
        let kinds = [Kind::ReviewOpen, Kind::ReviewComments, Kind::ReviewRead, Kind::AcceptStep, Kind::AcceptSubtask];
        assert_eq!(kinds.map(|kind| (kind.name(), kind.is_command(), kind.default_ttl(), kind.max_ttl())), [
            ("review-open", true, 3_600, 3_600),
            ("review-comments", true, 3_600, 3_600),
            ("review-read", true, 3_600, 3_600),
            ("accept-step", true, 3_600, 3_600),
            ("accept-subtask", true, 3_600, 3_600),
        ]);
        let open = serde_json::json!({ "type": "review-open", "address": "retry-push@kebbie" });
        assert_eq!(serde_json::from_value::<Payload>(open).unwrap().kind(), Kind::ReviewOpen);
        let blob = serde_json::json!({ "id": format!("blb_{}", "ab".repeat(32)), "key": "k".repeat(43), "media_type": crate::REVIEW_MEDIA_TYPE, "bytes": 2048 });
        let device = Id::ulid(Prefix::Device, 1, [2; 10]).unwrap();
        let done = serde_json::json!({ "type": "result", "command": Id::command(1, [1; 10]), "device": device, "outcome": "done", "text": "The review is ready.", "blob": blob });
        let Payload::Result(result) = serde_json::from_value::<Payload>(done.clone()).unwrap() else { panic!("a result") };
        assert_eq!(result.blob.as_ref().map(|blob| blob.bytes), Some(2048));
        assert_eq!(serde_json::to_value(Payload::Result(result)).unwrap(), done);
        let mut refused = done;
        refused["outcome"] = serde_json::json!("refused");
        refused["reason"] = serde_json::json!("not-allowed");
        assert!(serde_json::from_value::<Payload>(refused).is_err());
    }

    #[test]
    fn found_items_are_started_added_to_a_task_or_dismissed_by_their_key() {
        let found = || ItemKey::parse("found:f3").unwrap();
        let start = |name: Option<&str>| Payload::FoundStart(FoundStart { item: found(), name: name.map(Into::into) });
        assert_eq!((start(None).validate(), start(Some("remote-log-outage")).validate()), (Ok(()), Ok(())));
        assert!(start(Some("Remote log")).validate().is_err());
        assert!(Payload::FoundStart(FoundStart { item: ItemKey::parse("kept:idea-1").unwrap(), name: None }).validate().is_err(), "only an item tasks found");
        let add = |task: &str| Payload::FoundAdd(FoundAdd { item: found(), task: task.into() });
        assert_eq!(add("lint-index@mindin").validate(), Ok(()));
        assert!(add("lint-index").validate().is_err());
        assert_eq!(Payload::FoundDismiss(FoundDismiss { item: found() }).validate(), Ok(()));
        assert!(Payload::FoundDismiss(FoundDismiss { item: ItemKey::vault("Projects/x.md").unwrap() }).validate().is_err());
        assert_eq!(serde_json::to_value(start(None)).unwrap(), serde_json::json!({ "type": "found-start", "item": "found:f3" }));
        assert!(serde_json::from_value::<Payload>(serde_json::json!({ "type": "found-dismiss", "item": "found:f3", "reason": "noise" })).is_err());
        assert_eq!(
            [Kind::FoundStart, Kind::FoundAdd, Kind::FoundDismiss].map(|kind| (kind.name(), kind.is_command(), kind.default_ttl(), kind.max_ttl())),
            [("found-start", true, 86_400, 86_400), ("found-add", true, 3_600, 3_600), ("found-dismiss", true, 3_600, 3_600)]
        );
    }

    #[test]
    fn a_noticed_line_is_kept_by_its_key_and_a_task_is_sent_to_a_full_review_by_its_address() {
        let keep = |item: &str| Payload::FoundKeep(FoundKeep { item: ItemKey::parse(item).unwrap() });
        assert_eq!(keep("found:f3").validate(), Ok(()));
        assert!(keep("kept:idea-1").validate().is_err(), "only an item tasks found");
        assert_eq!(serde_json::to_value(keep("found:f3")).unwrap(), serde_json::json!({ "type": "found-keep", "item": "found:f3" }));
        let send = |address: &str, subtask: Option<&str>| Payload::ReviewSend(crate::ReviewSend { address: address.into(), subtask: subtask.map(Into::into) });
        assert_eq!((send("payments@api", None).validate(), send("payments@api", Some("gateway")).validate()), (Ok(()), Ok(())));
        assert!(send("payments", None).validate().is_err());
        assert!(send("payments@api", Some("../gateway")).validate().is_err());
        assert_eq!(serde_json::to_value(send("payments@api", None)).unwrap(), serde_json::json!({ "type": "review-send", "address": "payments@api" }));
        assert_eq!(
            [Kind::FoundKeep, Kind::ReviewSend].map(|kind| (kind.name(), kind.is_command(), kind.default_ttl(), kind.max_ttl())),
            [("found-keep", true, 3_600, 3_600), ("review-send", true, 3_600, 3_600)]
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
