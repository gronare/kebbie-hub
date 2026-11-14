use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize};

use crate::backlog::BacklogRow;
use crate::envelope::Kind;
use crate::ids::{Id, Prefix};
use crate::message::{check, is_full_sha, is_line, is_text};
use crate::review::ReviewOffer;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    pub rev: u64,
    pub kebbie: String,
    pub away_since: Option<u64>,
    pub checkouts: Vec<Checkout>,
    pub routines: Vec<RoutineSummary>,
    pub items: Vec<Item>,
    pub sessions_outside_tasks: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<Vec<NextRow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<Counts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub later: Option<Vec<Later>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepts: Option<Vec<Kind>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "device_ids")]
    pub devices: Option<BTreeMap<Id, DeviceStanding>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "crate::backlog::backlog_rows")]
    pub backlog: Option<Vec<BacklogRow>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DeviceStanding {
    Waiting { fingerprint: String, until: Option<u64> },
    Approved { fingerprint: String, at: u64 },
    Denied { fingerprint: String, at: u64 },
}

fn device_ids<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<BTreeMap<Id, DeviceStanding>>, D::Error> {
    let devices = Option::<BTreeMap<Id, DeviceStanding>>::deserialize(deserializer)?;
    if devices
        .iter()
        .flat_map(BTreeMap::keys)
        .any(|id| id.prefix() != Prefix::Device)
    {
        return Err(serde::de::Error::custom("devices are keyed by device id"));
    }
    Ok(devices)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextRow {
    pub part: NextPart,
    pub address: String,
    pub rank: u32,
    pub line: String,
    pub since: Option<u64>,
    pub waits_behind: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NextPart {
    Stopped,
    Push,
    Read,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub working: u32,
    pub queued: u32,
    pub parked: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Later {
    pub address: String,
    pub title: Option<String>,
    pub state: LaterState,
    pub line: String,
    pub until: Option<u64>,
    pub question: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LaterState {
    Queued,
    Parked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkout {
    pub repo: String,
    pub free_worktrees: u32,
    pub busy_worktrees: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineSummary {
    pub id: String,
    pub title: String,
    pub repo: String,
    pub mode: String,
    pub next_run: Option<u64>,
    pub last_outcome: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "ItemWire")]
pub struct Item {
    pub address: String,
    pub group: Group,
    pub urgency: u8,
    pub summary: String,
    pub next_for_you: Option<String>,
    pub agent: Option<Agent>,
    pub report: Option<Report>,
    pub question: Option<Question>,
    pub prompt: Option<Prompt>,
    pub git: Option<Git>,
    pub head: Option<String>,
    pub verdict: Option<Verdict>,
    pub push_permission: Option<PushPermission>,
    pub push_waits: Option<String>,
    pub follows_main: bool,
    pub ci: Option<Ci>,
    pub recover: Option<Recover>,
    pub routine: Option<String>,
    pub card: Option<CardRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtasks: Option<Vec<Subtask>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<Vec<OrderRow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushable: Option<Pushable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_accept: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub obstacle: Option<Obstacle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stages: Option<Vec<Stage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kebbie_questions: Option<Vec<KebbieQuestion>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assessment: Option<Assessment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_opinion: Option<SecondOpinion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ReviewOffer>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemWire {
    address: String,
    group: Group,
    urgency: u8,
    summary: String,
    next_for_you: Option<String>,
    agent: Option<Agent>,
    report: Option<Report>,
    question: Option<Question>,
    prompt: Option<Prompt>,
    git: Option<Git>,
    head: Option<String>,
    verdict: Option<Verdict>,
    push_permission: Option<PushPermission>,
    push_waits: Option<String>,
    follows_main: bool,
    ci: Option<Ci>,
    recover: Option<Recover>,
    routine: Option<String>,
    card: Option<CardRef>,
    #[serde(default)]
    subtasks: Option<Vec<Subtask>>,
    #[serde(default)]
    order: Option<Vec<OrderRowWire>>,
    #[serde(default)]
    pushable: Option<Pushable>,
    #[serde(default)]
    require_accept: Option<bool>,
    #[serde(default)]
    obstacle: Option<Obstacle>,
    #[serde(default)]
    stages: Option<Vec<Stage>>,
    #[serde(default)]
    kebbie_questions: Option<Vec<KebbieQuestion>>,
    #[serde(default)]
    steps: Option<Steps>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    recipe: Option<String>,
    #[serde(default)]
    assessment: Option<Assessment>,
    #[serde(default)]
    second_opinion: Option<SecondOpinion>,
    #[serde(default)]
    review: Option<ReviewOffer>,
}

impl TryFrom<ItemWire> for Item {
    type Error = &'static str;

    fn try_from(wire: ItemWire) -> Result<Self, Self::Error> {
        let (subtasks, order, pushable, require_accept) = match wire.steps {
            None => {
                let order = wire.order.map(|rows| rows.into_iter().map(|row| row.read(wire.subtasks.as_deref().unwrap_or_default())).collect());
                (wire.subtasks, order, wire.pushable, wire.require_accept)
            }
            Some(_) if wire.subtasks.is_some() || wire.order.is_some() || wire.pushable.is_some() || wire.require_accept.is_some() || wire.stages.is_some() => return Err("an item carries subtasks or, from an older machine, steps"),
            Some(steps) => {
                let (subtasks, order, pushable) = steps.as_subtasks();
                (Some(subtasks), Some(order), pushable, steps.required)
            }
        };
        Ok(Item {
            address: wire.address,
            group: wire.group,
            urgency: wire.urgency,
            summary: wire.summary,
            next_for_you: wire.next_for_you,
            agent: wire.agent,
            report: wire.report,
            question: wire.question,
            prompt: wire.prompt,
            git: wire.git,
            head: wire.head,
            verdict: wire.verdict,
            push_permission: wire.push_permission,
            push_waits: wire.push_waits,
            follows_main: wire.follows_main,
            ci: wire.ci,
            recover: wire.recover,
            routine: wire.routine,
            card: wire.card,
            subtasks,
            order,
            pushable,
            require_accept,
            obstacle: wire.obstacle,
            stages: wire.stages,
            kebbie_questions: wire.kebbie_questions,
            title: wire.title,
            profile: wire.profile,
            recipe: wire.recipe,
            assessment: wire.assessment,
            second_opinion: wire.second_opinion,
            review: wire.review,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecondOpinion {
    pub by: String,
    pub points: Option<u32>,
    pub running: bool,
}

pub const MAX_ASSESSMENT_REASON_CHARS: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Assessment {
    Said(String),
    Judged(Judgement),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Judgement {
    pub word: AssessmentWord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssessmentWord {
    Recommended,
    NotNeeded,
    Assessing,
}

impl Assessment {
    fn check(&self) -> Result<(), &'static str> {
        let Assessment::Judged(judged) = self else {
            return Ok(());
        };
        check(judged.reason.as_ref().is_none_or(|reason| is_line(reason, MAX_ASSESSMENT_REASON_CHARS)), "an assessment's reason is empty, too long or not one line")?;
        match judged.word {
            AssessmentWord::Recommended => check(judged.reason.is_some(), "a recommended review says why"),
            AssessmentWord::Assessing => check(judged.reason.is_none() && judged.at.is_none(), "an assessment still running has no reason and no time yet"),
            AssessmentWord::NotNeeded => Ok(()),
        }
    }
}

pub const MAX_SUBTASK_ID_CHARS: usize = 32;
pub const MAX_ADDED_REASON_CHARS: usize = 500;
pub const MAX_OBSTACLE_CHARS: usize = 2_000;
pub const MAX_MOVED_REASON_CHARS: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subtask {
    pub id: String,
    pub title: String,
    pub state: SubtaskState,
    pub commits: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<Added>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Rules>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Added {
    pub at: u64,
    pub reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubtaskState {
    Planned,
    InProgress,
    Ready,
    Accepted,
    Pushed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderRow {
    pub sha: String,
    pub subject: String,
    pub subtask: String,
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<CommitTests>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved: Option<Moved>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderRowWire {
    sha: String,
    subject: String,
    subtask: String,
    #[serde(default)]
    accepted: Option<bool>,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    tests: Option<CommitTests>,
    #[serde(default)]
    moved: Option<Moved>,
    #[serde(default)]
    stage: Option<String>,
}

impl OrderRowWire {
    fn read(self, subtasks: &[Subtask]) -> OrderRow {
        let accepted = self.accepted.unwrap_or_else(|| subtasks.iter().any(|subtask| subtask.id == self.subtask && subtask.state == SubtaskState::Accepted));
        OrderRow { sha: self.sha, subject: self.subject, subtask: self.subtask, accepted, group: self.group, tests: self.tests, moved: self.moved, stage: self.stage }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitTests {
    pub state: TestsState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TestsState {
    Passed,
    Failed,
    Running,
    NotRun,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Moved {
    pub from: u32,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pushable {
    pub count: u32,
    pub through: String,
    pub subtasks: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Obstacle {
    pub id: String,
    pub state: ObstacleState,
    pub text: String,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<ObstacleDecision>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ObstacleState {
    Investigating,
    Asked,
    WithLead,
    Decided,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObstacleDecision {
    pub kind: DecisionKind,
    pub by: DecidedBy,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionKind {
    Workaround,
    Subtask,
    Task,
    Ask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecidedBy {
    Investigator,
    Lead,
    You,
    Task,
}

pub const MAX_STAGES: usize = 16;
pub const MAX_STAGE_TITLE_CHARS: usize = 200;
pub const MAX_STAGE_DETAIL_CHARS: usize = 500;
pub const MAX_STAGE_NOTE_CHARS: usize = 500;

/// A boundary in the push order: nothing after a stage goes out until every stage before it is met.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub id: String,
    pub title: String,
    pub state: StageState,
    pub requirement: Requirement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub met_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StageState {
    Locked,
    Open,
    Pushed,
    Waiting,
    Met,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub kind: RequirementKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequirementKind {
    Deploy,
    Check,
    Verified,
}

pub const MAX_KEBBIE_QUESTIONS: usize = 4;
pub const MAX_KEBBIE_QUESTION_ID_CHARS: usize = 64;
pub const MAX_KEBBIE_QUESTION_CHARS: usize = 2_000;
pub const MAX_MEANING_CHARS: usize = 300;
pub const VERIFY_OPTIONS: [&str; 3] = ["Verified", "Not yet", "Something is wrong"];
pub const CLOSE_OPTIONS: [&str; 2] = ["Close", "Not yet"];

/// A question kebbie asks itself, beside the task's own: it never holds the task, and its answer is kept for the machine while it sleeps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KebbieQuestion {
    pub id: String,
    pub kind: KebbieQuestionKind,
    pub text: String,
    pub options: Vec<QuestionOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KebbieQuestionKind {
    Verify,
    Close,
}

impl KebbieQuestionKind {
    pub fn labels(self) -> &'static [&'static str] {
        match self {
            KebbieQuestionKind::Verify => &VERIFY_OPTIONS,
            KebbieQuestionKind::Close => &CLOSE_OPTIONS,
        }
    }

    fn takes_comment(self, index: usize) -> bool {
        self == KebbieQuestionKind::Verify && index == 2
    }
}

impl Stage {
    pub fn is_pushed(&self) -> bool {
        self.through.is_some()
    }

    fn check(&self) -> Result<(), &'static str> {
        check(is_subtask_id(&self.id), "a stage's id is not a short plain name")?;
        check(is_line(&self.title, MAX_STAGE_TITLE_CHARS), "a stage's title is empty, too long or not one line")?;
        match (self.requirement.kind, &self.requirement.detail) {
            (RequirementKind::Deploy, detail) => check(detail.is_none(), "a deploy requirement has no detail")?,
            (_, detail) => check(detail.as_deref().is_some_and(|detail| is_line(detail, MAX_STAGE_DETAIL_CHARS)), "a check or a verification says on one line what it holds to")?,
        }
        check(self.through.as_deref().is_none_or(is_full_sha), "a stage's last pushed commit is not a full sha")?;
        check(self.through.is_some() == self.pushed_at.is_some(), "a stage pushed says when, and only a pushed one")?;
        let pushed = matches!(self.state, StageState::Pushed | StageState::Waiting | StageState::Met | StageState::Failed);
        check(self.is_pushed() == pushed, "a stage is locked or open until something of it is pushed")?;
        check((self.state == StageState::Met) == self.met_at.is_some(), "a stage says when it was met, and only a met one")?;
        check(self.note.as_deref().is_none_or(|note| is_line(note, MAX_STAGE_NOTE_CHARS)), "a stage's note is empty, too long or not one line")
    }
}

impl KebbieQuestion {
    fn check(&self, stages: &[Stage]) -> Result<(), &'static str> {
        let id = self.id.as_bytes();
        check((1..=MAX_KEBBIE_QUESTION_ID_CHARS).contains(&id.len()) && id.iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')), "a question's id is not a short plain name")?;
        check(is_obstacle_text(&self.text) && self.text.chars().count() <= MAX_KEBBIE_QUESTION_CHARS, "a question's text is empty, too long or holds control characters")?;
        let labels = self.kind.labels();
        check(self.options.len() == labels.len(), "kebbie's question offers its kind's fixed answers")?;
        for (index, (option, label)) in self.options.iter().zip(labels).enumerate() {
            check(option.label == *label && option.comment == self.kind.takes_comment(index), "kebbie's question offers its kind's fixed answers")?;
            check(is_line(&option.meaning, MAX_MEANING_CHARS), "an answer's meaning is empty, too long or not one line")?;
        }
        let Some(stage) = &self.stage else {
            return Ok(());
        };
        check(self.kind == KebbieQuestionKind::Verify, "only a verification names a stage")?;
        let stage = stages.iter().find(|one| &one.id == stage).ok_or("a verification names a stage the task does not have")?;
        check(stage.requirement.kind == RequirementKind::Verified && stage.state == StageState::Waiting, "a stage is asked about while it waits on your verification")
    }
}

pub fn is_subtask_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=MAX_SUBTASK_ID_CHARS).contains(&bytes.len()) && bytes[0].is_ascii_alphanumeric() && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

impl Subtask {
    pub fn head(&self) -> Option<&str> {
        self.commits.last().map(String::as_str)
    }

    fn check(&self) -> Result<(), &'static str> {
        check(is_subtask_id(&self.id), "a subtask's id is not a short plain name")?;
        check(self.commits.iter().all(|sha| is_full_sha(sha)), "a subtask's commit is not a full sha")?;
        match self.state {
            SubtaskState::Planned => check(self.commits.is_empty(), "a planned subtask holds no commits")?,
            SubtaskState::Ready | SubtaskState::Accepted | SubtaskState::Pushed => check(!self.commits.is_empty(), "a subtask to review, accepted or pushed holds a commit")?,
            SubtaskState::InProgress => {}
        }
        check(self.added.as_ref().is_none_or(|added| is_line(&added.reason, MAX_ADDED_REASON_CHARS)), "a subtask's reason for being added is empty, too long or not one line")?;
        self.rules.as_ref().map_or(Ok(()), Rules::check)
    }
}

impl Obstacle {
    fn check(&self) -> Result<(), &'static str> {
        check(is_subtask_id(&self.id), "an obstacle's id is not a short plain name")?;
        check(is_obstacle_text(&self.text), "an obstacle's text is empty, too long or holds control characters")?;
        let Some(decision) = &self.decision else {
            return check(self.state != ObstacleState::Decided, "a decided obstacle carries its decision");
        };
        check(self.state != ObstacleState::Investigating, "an obstacle still being investigated has no decision")?;
        check(decision.by != DecidedBy::Task, "an obstacle is decided by the investigator, the lead or you, never the task")?;
        check((decision.kind == DecisionKind::Ask) == (self.state == ObstacleState::Asked), "an obstacle is asked of you exactly when its decision is to ask")?;
        check(is_obstacle_text(&decision.text), "a decision's text is empty, too long or holds control characters")
    }
}

fn is_obstacle_text(text: &str) -> bool {
    is_text(text, MAX_OBSTACLE_CHARS) && !text.chars().any(|char| char.is_control() && char != '\n')
}

impl Item {
    pub fn subtask(&self, id: &str) -> Option<&Subtask> {
        self.subtasks.iter().flatten().find(|subtask| subtask.id == id)
    }

    fn check_plan(&self) -> Result<(), &'static str> {
        let subtasks = self.subtasks.as_deref().unwrap_or_default();
        check(self.subtasks.is_some() || (self.order.is_none() && self.pushable.is_none()), "a push order belongs to a task's subtasks")?;
        check(self.order.is_some() || self.pushable.is_none(), "a pushable run is a part of the push order")?;
        let mut ids = BTreeSet::new();
        let mut shas = BTreeSet::new();
        for subtask in subtasks {
            subtask.check()?;
            check(ids.insert(subtask.id.as_str()), "a subtask is listed once")?;
            check(subtask.commits.iter().all(|sha| shas.insert(sha.as_str())), "a commit belongs to one subtask")?;
        }
        let order = self.order.as_deref().unwrap_or_default();
        let mut placed = BTreeSet::new();
        let mut closed: BTreeSet<&str> = BTreeSet::new();
        let mut open: Option<&str> = None;
        for row in order {
            check(is_full_sha(&row.sha), "a commit in the push order is not a full sha")?;
            check(placed.insert(row.sha.as_str()), "a commit sits once in the push order")?;
            let owner = self.subtask(&row.subtask).ok_or("a commit in the push order names a subtask the task does not have")?;
            check(owner.commits.contains(&row.sha), "a commit in the push order is not one of its subtask's")?;
            check(owner.state != SubtaskState::Pushed, "a pushed subtask has nothing left in the push order")?;
            check(row.accepted || owner.state != SubtaskState::Accepted, "every commit of an accepted subtask is accepted")?;
            check(row.tests.as_ref().is_none_or(|tests| tests.state != TestsState::NotRun || tests.at.is_none()), "a commit not tested carries no time of its test run")?;
            check(row.moved.as_ref().is_none_or(|moved| moved.from >= 1 && is_line(&moved.reason, MAX_MOVED_REASON_CHARS)), "a moved commit says from which place and why, on one line")?;
            let group = row.group.as_deref();
            check(group.is_none_or(is_subtask_id), "a group's id is not a short plain name")?;
            if open != group {
                if let Some(left) = open {
                    closed.insert(left);
                }
                check(group.is_none_or(|group| !closed.contains(group)), "a group's commits sit together in the push order")?;
                open = group;
            }
        }
        for group in order.iter().filter_map(|row| row.group.as_deref()) {
            check(order.iter().filter(|row| row.group.as_deref() == Some(group)).count() >= 2, "a group holds two commits or more")?;
        }
        let Some(pushable) = &self.pushable else {
            return Ok(());
        };
        let count = pushable.count as usize;
        check((1..=order.len()).contains(&count), "a pushable run holds one to all of the push order")?;
        check(order[count - 1].sha == pushable.through, "a pushable run ends at its last commit")?;
        check(run_of(&order[..count]) == pushable.subtasks, "a pushable run names the subtasks of its commits in order")?;
        check(order[..count].iter().all(|row| row.accepted), "every commit in a pushable run is accepted")?;
        check(!splits_a_group(order, count), "a pushable run never splits a group")?;
        let stages = self.stages.as_deref().unwrap_or_default();
        check(order[..count].iter().all(|row| opens(stages, row.stage.as_deref())), "a pushable run stops where a stage before it is not met")
    }

    fn check_stages(&self) -> Result<(), &'static str> {
        let order = self.order.as_deref().unwrap_or_default();
        let Some(stages) = self.stages.as_deref() else {
            return check(order.iter().all(|row| row.stage.is_none()), "a commit names a stage the task does not have");
        };
        check(self.subtasks.is_some(), "stages belong to a task's subtasks")?;
        check((1..=MAX_STAGES).contains(&stages.len()), "a task holds one to sixteen stages")?;
        let mut ids = BTreeSet::new();
        for (index, stage) in stages.iter().enumerate() {
            stage.check()?;
            check(ids.insert(stage.id.as_str()), "a stage is listed once")?;
            if !stage.is_pushed() {
                let held = stages[..index].iter().any(|earlier| earlier.state != StageState::Met);
                check(held == (stage.state == StageState::Locked), "a stage is locked exactly while a stage before it is not met")?;
            }
        }
        let place = |id: &str| stages.iter().position(|stage| stage.id == id);
        let mut last = 0;
        let mut of_subtask: BTreeMap<&str, &str> = BTreeMap::new();
        let mut of_group: BTreeMap<&str, &str> = BTreeMap::new();
        for row in order {
            let stage = row.stage.as_deref().ok_or("every commit of a task in stages names its stage")?;
            let at = place(stage).ok_or("a commit names a stage the task does not have")?;
            check(at >= last, "a stage's commits sit after the stages before it")?;
            last = at;
            check(*of_subtask.entry(row.subtask.as_str()).or_insert(stage) == stage, "a subtask's commits sit in one stage")?;
            if let Some(group) = row.group.as_deref() {
                check(*of_group.entry(group).or_insert(stage) == stage, "a group's commits sit in one stage")?;
            }
        }
        Ok(())
    }

    fn check_questions(&self) -> Result<(), &'static str> {
        let Some(questions) = self.kebbie_questions.as_deref() else {
            return Ok(());
        };
        check(questions.len() <= MAX_KEBBIE_QUESTIONS, "a task holds four of kebbie's questions at most")?;
        let mut ids: BTreeSet<&str> = self.question.iter().map(|question| question.id.as_str()).collect();
        for question in questions {
            question.check(self.stages.as_deref().unwrap_or_default())?;
            check(ids.insert(question.id.as_str()), "a question is asked once")?;
        }
        Ok(())
    }
}

/// Sets each stage nothing of which is pushed yet to locked or open, by whether every stage before it is met.
pub fn unlock_stages(stages: &mut [Stage]) {
    let mut held = false;
    for stage in stages {
        if !stage.is_pushed() {
            stage.state = if held { StageState::Locked } else { StageState::Open };
        }
        held |= stage.state != StageState::Met;
    }
}

fn opens(stages: &[Stage], stage: Option<&str>) -> bool {
    stage.is_none_or(|id| stages.iter().take_while(|stage| stage.id != id).all(|stage| stage.state == StageState::Met))
}

pub fn pushable_of(order: &[OrderRow], stages: &[Stage]) -> Option<Pushable> {
    let mut count = order.iter().take_while(|row| row.accepted && opens(stages, row.stage.as_deref())).count();
    while count > 0 && splits_a_group(order, count) {
        count -= 1;
    }
    let last = order[..count].last()?;
    Some(Pushable { count: count as u32, through: last.sha.clone(), subtasks: run_of(&order[..count]) })
}

fn splits_a_group(order: &[OrderRow], count: usize) -> bool {
    let (run, rest) = order.split_at(count);
    run.iter().filter_map(|row| row.group.as_deref()).any(|group| rest.iter().any(|row| row.group.as_deref() == Some(group)))
}

fn run_of(run: &[OrderRow]) -> Vec<String> {
    let mut subtasks: Vec<String> = Vec::new();
    for row in run {
        if !subtasks.contains(&row.subtask) {
            subtasks.push(row.subtask.clone());
        }
    }
    subtasks
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Steps {
    #[serde(rename = "pushed")]
    _pushed: u32,
    items: Vec<Step>,
    #[serde(default)]
    required: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    number: u32,
    title: String,
    state: StepState,
    #[serde(default)]
    commits: Option<Vec<StepCommit>>,
    #[serde(default)]
    sha: Option<String>,
    #[serde(default)]
    rules: Option<Rules>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepCommit {
    sha: String,
    subject: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StepState {
    Planned,
    InProgress,
    #[serde(alias = "committed")]
    Ready,
    Accepted,
    Pushed,
}

impl From<StepState> for SubtaskState {
    fn from(state: StepState) -> SubtaskState {
        match state {
            StepState::Planned => SubtaskState::Planned,
            StepState::InProgress => SubtaskState::InProgress,
            StepState::Ready => SubtaskState::Ready,
            StepState::Accepted => SubtaskState::Accepted,
            StepState::Pushed => SubtaskState::Pushed,
        }
    }
}

impl Steps {
    fn as_subtasks(&self) -> (Vec<Subtask>, Vec<OrderRow>, Option<Pushable>) {
        let commits = |step: &Step| -> Vec<(String, String)> {
            match (&step.commits, &step.sha) {
                (Some(commits), _) => commits.iter().map(|commit| (commit.sha.clone(), commit.subject.clone())).collect(),
                (None, sha) => sha.iter().map(|sha| (sha.clone(), step.title.clone())).collect(),
            }
        };
        let subtasks: Vec<Subtask> = self.items.iter().map(|step| Subtask {
            id: step.number.to_string(),
            title: step.title.clone(),
            state: step.state.into(),
            commits: commits(step).into_iter().map(|(sha, _)| sha).collect(),
            added: None,
            rules: step.rules.clone(),
        }).collect();
        let order: Vec<OrderRow> = self.items.iter().filter(|step| !matches!(step.state, StepState::Pushed)).flat_map(|step| {
            commits(step).into_iter().map(|(sha, subject)| OrderRow { sha, subject, subtask: step.number.to_string(), accepted: matches!(step.state, StepState::Accepted), group: None, tests: None, moved: None, stage: None })
        }).collect();
        let pushable = pushable_of(&order, &[]);
        (subtasks, order, pushable)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Group {
    #[serde(rename = "NEEDS YOU")]
    NeedsYou,
    #[serde(rename = "WORKING")]
    Working,
    #[serde(rename = "TO REVIEW")]
    ToReview,
    #[serde(rename = "READY TO PUSH")]
    ReadyToPush,
    #[serde(rename = "CI RUNNING")]
    CiRunning,
    #[serde(rename = "WAITING FOR WORK")]
    Idle,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub status: AgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentStatus {
    Working,
    Blocked,
    Idle,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub state: String,
    pub next: Option<String>,
    pub summary: String,
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub id: String,
    pub text: String,
    pub options: Vec<QuestionOption>,
    pub recommend: Option<u8>,
    pub kept_for_user: bool,
    pub plan: Option<BlobRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_at: Option<u64>,
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionOption {
    pub label: String,
    pub meaning: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub comment: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    pub id: String,
    pub question: String,
    pub options: Vec<PromptOption>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptOption {
    pub label: String,
    pub takes_text: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, from = "GitWire")]
pub struct Git {
    pub upstream: String,
    pub ahead: u32,
    pub behind: u32,
    pub uncommitted: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Rules>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GitWire {
    upstream: String,
    ahead: u32,
    behind: u32,
    uncommitted: u32,
    #[serde(default)]
    rules: Option<Rules>,
    #[serde(default, rename = "approved")]
    _approved: Option<bool>,
}

impl From<GitWire> for Git {
    fn from(wire: GitWire) -> Git {
        Git { upstream: wire.upstream, ahead: wire.ahead, behind: wire.behind, uncommitted: wire.uncommitted, rules: wire.rules }
    }
}

pub const MAX_RULE_NAMES: usize = 32;
pub const MAX_RULE_NAME_CHARS: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub state: RulesState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breaks: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    #[serde(default, rename = "override", skip_serializing_if = "Option::is_none")]
    pub overridden: Option<RulesOverride>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RulesState {
    Checking,
    Ok,
    Breaks,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesOverride {
    pub by: OverrideBy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverrideBy {
    User,
    Lead,
}

impl Rules {
    pub fn check(&self) -> Result<(), &'static str> {
        match self.state {
            RulesState::Breaks => check(self.breaks.is_some_and(|breaks| breaks >= 1), "a rules check that breaks counts at least one rule")?,
            _ => check(self.breaks.is_none() && self.names.is_empty(), "only a rules check that breaks counts or names rules")?,
        }
        check(self.names.len() <= MAX_RULE_NAMES && self.names.len() as u64 <= u64::from(self.breaks.unwrap_or(0)), "more rule names than broken rules")?;
        check(self.names.iter().all(|name| is_line(name, MAX_RULE_NAME_CHARS)), "a rule name is empty, too long or not one line")?;
        check(self.overridden.is_none() || self.state != RulesState::Ok, "a rules check that passed has nothing to override")
    }
}

impl Board {
    pub fn check(&self) -> Result<(), &'static str> {
        self.items.iter().try_for_each(|item| {
            item.git.iter().filter_map(|git| git.rules.as_ref()).try_for_each(Rules::check)?;
            item.obstacle.as_ref().map_or(Ok(()), Obstacle::check)?;
            item.assessment.as_ref().map_or(Ok(()), Assessment::check)?;
            item.review.as_ref().map_or(Ok(()), |offer| check(offer.subtask.as_deref().is_none_or(is_subtask_id), "a review offer names a subtask by its id"))?;
            item.check_plan()?;
            item.check_stages()?;
            item.check_questions()
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    pub verdict: Option<VerdictOutcome>,
    pub findings: Option<u32>,
    pub sha: Option<String>,
    pub stale: bool,
    pub running: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerdictOutcome {
    Ok,
    Changes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PushPermission {
    Task,
    Repo,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ci {
    pub state: CiState,
    pub failure: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CiState {
    Running,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recover {
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardRef {
    pub sha: String,
    pub current: bool,
    pub blob: BlobRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobRef {
    pub id: Id,
    pub key: String,
    pub media_type: String,
    pub bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_with_a_local_path_is_refused() {
        let item = serde_json::json!({
            "address": "refunds@shop", "group": "WORKING", "urgency": 0, "summary": "1 commit", "next_for_you": null,
            "agent": null, "report": null, "question": null, "prompt": null, "git": null, "head": null, "verdict": null,
            "push_permission": null, "push_waits": null, "follows_main": false, "ci": null, "recover": null, "routine": null, "card": null,
        });
        assert!(serde_json::from_value::<Item>(item.clone()).is_ok());
        let mut leaked = item;
        leaked["path"] = serde_json::json!("/Users/someone/projects/shop");
        assert!(serde_json::from_value::<Item>(leaked).is_err());
    }

    fn plain() -> serde_json::Value {
        serde_json::json!({
            "address": "payments@api", "group": "WORKING", "urgency": 0, "summary": "2 commits", "next_for_you": null,
            "agent": null, "report": null, "question": null, "prompt": null, "git": null, "head": null, "verdict": null,
            "push_permission": null, "push_waits": null, "follows_main": false, "ci": null, "recover": null, "routine": null, "card": null,
        })
    }

    #[test]
    fn an_item_from_a_kebbie_without_subtasks_parses_and_is_written_back_without_the_fields() {
        let item: Item = serde_json::from_value(plain()).unwrap();
        assert_eq!((item.subtasks.as_ref(), item.order.as_ref(), item.pushable.as_ref(), item.obstacle.as_ref()), (None, None, None, None));
        assert_eq!(serde_json::to_value(&item).unwrap(), plain());
    }

    fn sha(digit: char) -> String {
        digit.to_string().repeat(40)
    }

    fn planned() -> serde_json::Value {
        let mut item = plain();
        item["subtasks"] = serde_json::json!([
            { "id": "env", "title": "Clean test environment", "state": "accepted", "commits": [sha('1'), sha('2')] },
            { "id": "fm", "title": "Frontmatter in the index", "state": "accepted", "commits": [sha('3')], "added": { "at": 1_761_069_120u64, "reason": "The index has no frontmatter" } },
            { "id": "lint", "title": "Lint from the index", "state": "ready", "commits": [sha('4'), sha('6')], "rules": { "state": "ok" } },
            { "id": "ci", "title": "CI on Python 3.14", "state": "accepted", "commits": [sha('5')] },
            { "id": "split", "title": "Split notes.py", "state": "in-progress", "commits": [sha('7')] },
            { "id": "wal", "title": "SQLite in WAL mode", "state": "planned", "commits": [] },
            { "id": "docs", "title": "Docs", "state": "pushed", "commits": [sha('0')] },
        ]);
        item["order"] = serde_json::json!([
            { "sha": sha('1'), "subject": "Give the tests their own vault", "subtask": "env", "accepted": true },
            { "sha": sha('2'), "subject": "Clean the vault between tests", "subtask": "env", "accepted": true },
            { "sha": sha('3'), "subject": "Keep frontmatter in the index", "subtask": "fm", "accepted": true },
            { "sha": sha('4'), "subject": "Read lint's fields from the index", "subtask": "lint", "accepted": false },
            { "sha": sha('5'), "subject": "Run CI on Python 3.14", "subtask": "ci", "accepted": true },
            { "sha": sha('6'), "subject": "Drop lint's fetch of every note", "subtask": "lint", "accepted": false },
            { "sha": sha('7'), "subject": "Move the backend factory", "subtask": "split", "accepted": false },
        ]);
        item["pushable"] = serde_json::json!({ "count": 3, "through": sha('3'), "subtasks": ["env", "fm"] });
        item["require_accept"] = serde_json::json!(true);
        item
    }

    fn checked(item: serde_json::Value) -> Result<(), String> {
        let item: Item = serde_json::from_value(item).map_err(|error| error.to_string())?;
        let board = Board { items: vec![item], ..serde_json::from_value(plain_board()).unwrap() };
        board.check().map_err(str::to_string)
    }

    #[test]
    fn a_task_carries_its_subtasks_its_push_order_and_the_run_that_can_go_out_now() {
        let item: Item = serde_json::from_value(planned()).unwrap();
        assert_eq!(checked(planned()), Ok(()));
        assert_eq!(serde_json::to_value(&item).unwrap(), planned());
        let lint = item.subtask("lint").unwrap();
        assert_eq!((lint.state, lint.head()), (SubtaskState::Ready, Some(sha('6').as_str())), "a subtask's commits need not sit together, and its head is its last");
        assert_eq!(item.subtask("fm").unwrap().added.as_ref().map(|added| added.reason.as_str()), Some("The index has no frontmatter"));
        assert_eq!(item.pushable.as_ref().map(|run| (run.count, run.through.as_str())), Some((3, sha('3').as_str())));
        let mut none = planned();
        none.as_object_mut().unwrap().remove("pushable");
        assert_eq!(checked(none), Ok(()), "nothing may be pushable yet");
        let mut later = planned();
        later["subtasks"][2]["state"] = serde_json::json!("accepted");
        later["order"][3]["accepted"] = serde_json::json!(true);
        later["order"][5]["accepted"] = serde_json::json!(true);
        later["pushable"] = serde_json::json!({ "count": 6, "through": sha('6'), "subtasks": ["env", "fm", "lint", "ci"] });
        assert_eq!(checked(later), Ok(()), "accepting lint frees ci, which waited behind it");
    }

    #[test]
    fn a_push_order_and_its_run_hold_together_with_the_subtasks() {
        let wrong: [(&str, serde_json::Value); 21] = [
            ("/subtasks/0/id", serde_json::json!("a b")),
            ("/subtasks/0/id", serde_json::json!("x".repeat(MAX_SUBTASK_ID_CHARS + 1))),
            ("/subtasks/1/id", serde_json::json!("env")),
            ("/subtasks/0/commits/0", serde_json::json!("1111111")),
            ("/subtasks/1/commits/0", serde_json::json!(sha('1'))),
            ("/subtasks/5/commits", serde_json::json!([sha('8')])),
            ("/subtasks/2/commits", serde_json::json!([])),
            ("/subtasks/0/state", serde_json::json!("merged")),
            ("/subtasks/1/added/reason", serde_json::json!("Two\nlines")),
            ("/subtasks/1/added/reason", serde_json::json!(" ")),
            ("/order/0/subtask", serde_json::json!("nope")),
            ("/order/0/subtask", serde_json::json!("fm")),
            ("/order/1/sha", serde_json::json!(sha('1'))),
            ("/order/6/subtask", serde_json::json!("docs")),
            ("/pushable/count", serde_json::json!(0)),
            ("/pushable/count", serde_json::json!(8)),
            ("/pushable/through", serde_json::json!(sha('2'))),
            ("/pushable/subtasks", serde_json::json!(["fm", "env"])),
            ("/pushable", serde_json::json!({ "count": 4, "through": sha('4'), "subtasks": ["env", "fm", "lint"] })),
            ("/subtasks/0/rules", serde_json::json!({ "state": "breaks" })),
            ("/order/0/accepted", serde_json::json!(false)),
        ];
        for (pointer, value) in wrong {
            let mut item = planned();
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            let parent = item.pointer_mut(parent).unwrap();
            match parent {
                serde_json::Value::Array(list) => list[key.parse::<usize>().unwrap()] = value.clone(),
                _ => parent[key] = value.clone(),
            }
            assert!(checked(item).is_err(), "{pointer} = {value}");
        }
        let mut alone = plain();
        alone["order"] = serde_json::json!([]);
        assert!(checked(alone).is_err(), "a push order belongs to subtasks");
        let mut unordered = planned();
        unordered.as_object_mut().unwrap().remove("order");
        assert!(checked(unordered).is_err(), "a pushable run is a part of the push order");
        let mut extra = planned();
        extra["order"][0]["merged"] = serde_json::json!(true);
        assert!(checked(extra).is_err(), "a row carries only its allow-listed fields");
    }

    fn with(mut item: serde_json::Value, changes: &[(&str, serde_json::Value)]) -> serde_json::Value {
        for (pointer, value) in changes {
            *item.pointer_mut(pointer).unwrap_or_else(|| panic!("{pointer}")) = value.clone();
        }
        item
    }

    fn row(item: &serde_json::Value, index: usize, key: &str, value: serde_json::Value) -> serde_json::Value {
        let mut item = item.clone();
        item["order"][index][key] = value;
        item
    }

    fn computed(item: &serde_json::Value) -> Option<(u32, String)> {
        let item: Item = serde_json::from_value(item.clone()).unwrap();
        pushable_of(item.order.as_deref().unwrap_or_default(), item.stages.as_deref().unwrap_or_default()).map(|run| (run.count, run.through))
    }

    #[test]
    fn a_ready_subtask_may_hold_accepted_commits_and_the_run_goes_as_far_as_every_commit_is_accepted() {
        let partly = with(planned(), &[("/order/3/accepted", serde_json::json!(true))]);
        let item: Item = serde_json::from_value(partly.clone()).unwrap();
        assert_eq!(item.subtask("lint").map(|lint| lint.state), Some(SubtaskState::Ready), "one of lint's two commits is accepted, so lint is still to review");
        assert_eq!(computed(&partly), Some((5, sha('5'))), "the run passes lint's accepted commit and stops at its unaccepted one");
        assert_eq!(checked(partly.clone()), Ok(()), "a run shorter than it could be still holds");
        let longer = with(partly.clone(), &[("/pushable", serde_json::json!({ "count": 5, "through": sha('5'), "subtasks": ["env", "fm", "lint", "ci"] }))]);
        assert_eq!(checked(longer), Ok(()), "a subtask in the run need not be accepted, only its commits in the run");
        let past = with(partly, &[("/pushable", serde_json::json!({ "count": 6, "through": sha('6'), "subtasks": ["env", "fm", "lint", "ci"] }))]);
        assert!(checked(past).is_err(), "a run never passes a commit that is not accepted");
        assert_eq!(computed(&planned()), Some((3, sha('3'))));
        let mut in_progress = with(planned(), &[("/subtasks/2/state", serde_json::json!("in-progress")), ("/order/3/accepted", serde_json::json!(true))]);
        assert_eq!(checked(in_progress.clone()), Ok(()), "a subtask that got a new commit after it was accepted keeps the accepted one");
        in_progress["subtasks"][2]["state"] = serde_json::json!("accepted");
        assert!(checked(in_progress).is_err(), "every commit of an accepted subtask is accepted");
        let none = with(planned(), &[("/order/0/accepted", serde_json::json!(false)), ("/subtasks/0/state", serde_json::json!("ready"))]);
        assert_eq!(computed(&none), None, "nothing is pushable when the first commit is not accepted");
    }

    #[test]
    fn a_group_goes_out_whole_and_its_commits_sit_together() {
        let grouped = row(&row(&planned(), 2, "group", serde_json::json!("index")), 3, "group", serde_json::json!("index"));
        assert_eq!(computed(&grouped), Some((2, sha('2'))), "fm's accepted commit waits for lint's, which goes out with it");
        assert!(checked(grouped.clone()).is_err(), "a run through fm would split the group");
        let held = with(grouped.clone(), &[("/pushable", serde_json::json!({ "count": 2, "through": sha('2'), "subtasks": ["env"] }))]);
        assert_eq!(checked(held.clone()), Ok(()));
        let item: Item = serde_json::from_value(held.clone()).unwrap();
        assert_eq!(item.order.as_ref().unwrap()[2].group.as_deref(), Some("index"));
        assert_eq!(serde_json::to_value(&item).unwrap(), held);
        let whole = with(grouped, &[("/order/3/accepted", serde_json::json!(true))]);
        assert_eq!(computed(&whole), Some((5, sha('5'))), "once both are accepted the group goes out whole");
        let apart = row(&row(&planned(), 0, "group", serde_json::json!("g")), 2, "group", serde_json::json!("g"));
        assert!(checked(apart).is_err(), "a group's commits sit together in the push order");
        let lone = row(&planned(), 0, "group", serde_json::json!("g"));
        assert!(checked(lone).is_err(), "a group holds two commits or more");
        let named = row(&row(&planned(), 0, "group", serde_json::json!("a b")), 1, "group", serde_json::json!("a b"));
        assert!(checked(named).is_err(), "a group's id is a short plain name");
        let two = row(&row(&row(&row(&planned(), 0, "group", serde_json::json!("a")), 1, "group", serde_json::json!("a")), 2, "group", serde_json::json!("b")), 3, "group", serde_json::json!("b"));
        assert_eq!(computed(&two), Some((2, sha('2'))), "two groups side by side each go out whole");
    }

    #[test]
    fn a_commit_says_where_its_tests_stand_and_nothing_waits_on_them() {
        for (state, at) in [("passed", Some(1_761_069_000u64)), ("failed", Some(1_761_069_000)), ("running", Some(1_761_069_000)), ("running", None), ("not-run", None)] {
            let tests = at.map_or_else(|| serde_json::json!({ "state": state }), |at| serde_json::json!({ "state": state, "at": at }));
            let one = row(&planned(), 0, "tests", tests);
            assert_eq!(checked(one.clone()), Ok(()), "{state}");
            assert_eq!(serde_json::to_value(serde_json::from_value::<Item>(one.clone()).unwrap()).unwrap(), one);
        }
        let failed = row(&planned(), 0, "tests", serde_json::json!({ "state": "failed" }));
        assert_eq!(computed(&failed), Some((3, sha('3'))), "a failed test is advisory, the run still goes through it");
        for wrong in [serde_json::json!({ "state": "not-run", "at": 1 }), serde_json::json!({ "state": "stale" }), serde_json::json!({ "state": "passed", "by": "ci" }), serde_json::json!("passed")] {
            assert!(checked(row(&planned(), 0, "tests", wrong.clone())).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_commit_kebbie_moved_up_says_from_where_and_why() {
        let moved = row(&planned(), 2, "moved", serde_json::json!({ "from": 4, "reason": "Prepares lint's read from the index" }));
        assert_eq!(checked(moved.clone()), Ok(()));
        let item: Item = serde_json::from_value(moved.clone()).unwrap();
        assert_eq!(item.order.as_ref().unwrap()[2].moved, Some(Moved { from: 4, reason: "Prepares lint's read from the index".into() }));
        assert_eq!(serde_json::to_value(&item).unwrap(), moved);
        for wrong in [serde_json::json!({ "from": 0, "reason": "Prepares lint" }), serde_json::json!({ "from": 4, "reason": " " }), serde_json::json!({ "from": 4, "reason": "Two\nlines" }), serde_json::json!({ "from": 4, "reason": "x".repeat(MAX_MOVED_REASON_CHARS + 1) }), serde_json::json!({ "from": 4 })] {
            assert!(checked(row(&planned(), 2, "moved", wrong.clone())).is_err(), "{wrong}");
        }
    }

    #[test]
    fn a_push_order_from_a_machine_not_yet_updated_reads_each_commit_as_accepted_with_its_subtask() {
        let mut older = planned();
        for row in older["order"].as_array_mut().unwrap() {
            row.as_object_mut().unwrap().remove("accepted");
        }
        let item: Item = serde_json::from_value(older.clone()).unwrap();
        assert_eq!(item.order.as_ref().unwrap().iter().map(|row| row.accepted).collect::<Vec<_>>(), [true, true, true, false, true, false, false]);
        assert_eq!(checked(older), Ok(()));
        assert_eq!(serde_json::to_value(&item).unwrap(), planned(), "a board is written back with every commit's acceptance");
    }

    #[test]
    fn a_task_says_where_its_obstacle_stands() {
        let mut held = plain();
        held["obstacle"] = serde_json::json!({ "id": "o2", "state": "investigating", "text": "Lint should read the fields from the index,\nbut the index keeps only title and text.", "at": 1_761_068_460u64 });
        assert_eq!(checked(held.clone()), Ok(()));
        let item: Item = serde_json::from_value(held.clone()).unwrap();
        assert_eq!(item.obstacle.as_ref().map(|obstacle| obstacle.state), Some(ObstacleState::Investigating));
        assert_eq!(serde_json::to_value(&item).unwrap(), held);
        let decided = |state: &str, kind: &str, by: &str| {
            let mut one = held.clone();
            one["obstacle"]["state"] = serde_json::json!(state);
            one["obstacle"]["decision"] = serde_json::json!({ "kind": kind, "by": by, "text": "Frontmatter belongs in the index." });
            one
        };
        for (state, kind, by) in [("decided", "subtask", "investigator"), ("decided", "workaround", "lead"), ("decided", "task", "you"), ("asked", "ask", "investigator"), ("with-lead", "workaround", "investigator")] {
            let one = decided(state, kind, by);
            assert_eq!(checked(one.clone()), Ok(()), "{state} {kind} {by}");
            assert_eq!(serde_json::to_value(serde_json::from_value::<Item>(one.clone()).unwrap()).unwrap(), one);
        }
        let mut asked = held.clone();
        asked["obstacle"]["state"] = serde_json::json!("with-lead");
        assert_eq!(checked(asked), Ok(()), "an obstacle with the lead need not carry a decision yet");
        for wrong in [decided("decided", "ask", "investigator"), decided("asked", "subtask", "investigator"), decided("investigating", "subtask", "investigator"), decided("decided", "merge", "investigator"), decided("decided", "subtask", "task")] {
            assert!(checked(wrong.clone()).is_err(), "{wrong}");
        }
        let mut bare = decided("decided", "subtask", "investigator");
        bare["obstacle"].as_object_mut().unwrap().remove("decision");
        assert!(checked(bare).is_err(), "a decided obstacle carries its decision");
        let mut long = decided("decided", "subtask", "investigator");
        long["obstacle"]["decision"]["text"] = serde_json::json!("x".repeat(MAX_OBSTACLE_CHARS + 1));
        assert!(checked(long).is_err());
        let mut timeless = held.clone();
        timeless["obstacle"].as_object_mut().unwrap().remove("at");
        assert!(checked(timeless).is_err(), "an obstacle says when it was raised");
        for (key, value) in [("state", serde_json::json!("raised")), ("text", serde_json::json!(" ")), ("text", serde_json::json!("a\u{7}b")), ("id", serde_json::json!("")), ("text", serde_json::json!("x".repeat(MAX_OBSTACLE_CHARS + 1)))] {
            let mut wrong = held.clone();
            wrong["obstacle"][key] = value.clone();
            assert!(checked(wrong).is_err(), "{key}: {value}");
        }
    }

    #[test]
    fn steps_from_a_machine_not_yet_updated_open_as_subtasks_in_order_named_by_their_numbers() {
        let mut stepped = plain();
        stepped["steps"] = serde_json::json!({ "pushed": 1, "required": true, "items": [
            { "number": 1, "title": "Docs", "state": "pushed", "commits": [{ "sha": sha('1'), "subject": "Say what a step is" }] },
            { "number": 2, "title": "Charge through a PaymentGateway", "state": "accepted", "commits": [{ "sha": sha('2'), "subject": "Add PaymentGateway" }, { "sha": sha('3'), "subject": "Charge orders through it" }] },
            { "number": 3, "title": "Add the new gateway", "state": "ready", "commits": [{ "sha": sha('4'), "subject": "Add the gateway's client" }], "rules": { "state": "ok" } },
            { "number": 4, "title": "Remove the old payment client", "state": "planned", "commits": [] },
        ] });
        let item: Item = serde_json::from_value(stepped.clone()).unwrap();
        let subtasks = item.subtasks.as_ref().unwrap();
        assert_eq!(subtasks.iter().map(|subtask| (subtask.id.as_str(), subtask.state, subtask.commits.len())).collect::<Vec<_>>(), [("1", SubtaskState::Pushed, 1), ("2", SubtaskState::Accepted, 2), ("3", SubtaskState::Ready, 1), ("4", SubtaskState::Planned, 0)]);
        assert_eq!(item.order.as_ref().unwrap().iter().map(|row| (row.subtask.as_str(), row.subject.as_str())).collect::<Vec<_>>(), [("2", "Add PaymentGateway"), ("2", "Charge orders through it"), ("3", "Add the gateway's client")]);
        assert_eq!(item.pushable, Some(Pushable { count: 2, through: sha('3'), subtasks: vec!["2".into()] }));
        assert_eq!((item.require_accept, subtasks[2].rules.as_ref().map(|rules| rules.state)), (Some(true), Some(RulesState::Ok)));
        assert_eq!(checked(stepped.clone()), Ok(()));
        let written = serde_json::to_value(&item).unwrap();
        assert!(written.get("steps").is_none() && written.get("subtasks").is_some(), "a board is written back in the new shape");
        let mut oldest = plain();
        oldest["steps"] = serde_json::json!({ "pushed": 0, "items": [
            { "number": 1, "title": "Charge through a PaymentGateway", "state": "committed", "sha": sha('2') },
            { "number": 2, "title": "Add the new gateway", "state": "in-progress", "sha": null },
        ] });
        let item: Item = serde_json::from_value(oldest).unwrap();
        assert_eq!(item.subtasks.as_ref().unwrap()[0].state, SubtaskState::Ready, "committed reads as ready");
        assert_eq!(item.order.unwrap(), vec![OrderRow { sha: sha('2'), subject: "Charge through a PaymentGateway".into(), subtask: "1".into(), accepted: false, group: None, tests: None, moved: None, stage: None }]);
        assert_eq!(item.pushable, None);
        let mut both = stepped;
        both["subtasks"] = serde_json::json!([]);
        assert!(serde_json::from_value::<Item>(both).is_err(), "subtasks or steps, never both");
    }

    #[test]
    fn a_task_and_each_subtask_carry_where_the_rules_check_stands_and_an_approved_flag_is_ignored() {
        let mut ruled = plain();
        ruled["git"] = serde_json::json!({ "upstream": "main", "ahead": 1, "behind": 0, "uncommitted": 0, "rules": { "state": "breaks", "breaks": 2, "names": ["Every commit says why"], "override": { "by": "lead" } } });
        ruled["subtasks"] = serde_json::json!([{ "id": "docs", "title": "Docs", "state": "ready", "commits": [sha('1')], "rules": { "state": "checking" } }]);
        ruled["order"] = serde_json::json!([{ "sha": sha('1'), "subject": "Docs", "subtask": "docs", "accepted": false }]);
        let item: Item = serde_json::from_value(ruled.clone()).unwrap();
        let rules = item.git.as_ref().unwrap().rules.as_ref().unwrap();
        assert_eq!((rules.state, rules.breaks, rules.overridden.as_ref().map(|by| by.by)), (RulesState::Breaks, Some(2), Some(OverrideBy::Lead)));
        assert_eq!(item.subtasks.as_ref().unwrap()[0].rules.as_ref().unwrap().state, RulesState::Checking);
        assert_eq!(serde_json::to_value(&item).unwrap(), ruled);
        let mut unchecked = ruled.clone();
        unchecked["git"]["rules"] = serde_json::Value::Null;
        let item: Item = serde_json::from_value(unchecked).unwrap();
        assert!(serde_json::to_value(&item).unwrap()["git"].get("rules").is_none(), "a task without a rules check leaves the field out");
        let mut approved = ruled.clone();
        approved["git"]["approved"] = serde_json::json!(true);
        let item: Item = serde_json::from_value(approved).unwrap();
        assert_eq!(serde_json::to_value(&item).unwrap(), ruled, "a machine not yet updated still sends approved, which is read and dropped");
        let mut unknown = ruled;
        unknown["git"]["rules_ok"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Item>(unknown).is_err());
    }

    #[test]
    fn a_rules_check_counts_what_it_breaks_and_only_a_check_that_did_not_pass_is_overridden() {
        let rules = |state, breaks: Option<u32>, names: &[&str], overridden: Option<OverrideBy>| Rules {
            state,
            breaks,
            names: names.iter().map(|name| name.to_string()).collect(),
            overridden: overridden.map(|by| RulesOverride { by }),
        };
        assert_eq!(rules(RulesState::Checking, None, &[], None).check(), Ok(()));
        assert_eq!(rules(RulesState::Ok, None, &[], None).check(), Ok(()));
        assert_eq!(rules(RulesState::Breaks, Some(2), &["Every commit says why"], Some(OverrideBy::User)).check(), Ok(()));
        assert_eq!(rules(RulesState::Checking, None, &[], Some(OverrideBy::Lead)).check(), Ok(()));
        assert!(rules(RulesState::Breaks, None, &[], None).check().is_err());
        assert!(rules(RulesState::Breaks, Some(0), &[], None).check().is_err());
        assert!(rules(RulesState::Ok, Some(1), &[], None).check().is_err());
        assert!(rules(RulesState::Checking, None, &["Every commit says why"], None).check().is_err());
        assert!(rules(RulesState::Breaks, Some(1), &["One", "Two"], None).check().is_err());
        assert!(rules(RulesState::Breaks, Some(1), &["One\nTwo"], None).check().is_err());
        assert!(rules(RulesState::Breaks, Some(1), &[&"x".repeat(MAX_RULE_NAME_CHARS + 1)], None).check().is_err());
        assert!(rules(RulesState::Ok, None, &[], Some(OverrideBy::Lead)).check().is_err());
    }

    #[test]
    fn a_question_carries_its_plan_and_a_task_its_card_with_or_without_when_they_were_written() {
        let blob = serde_json::json!({ "id": format!("blb_{}", "ab".repeat(32)), "key": "k".repeat(43), "media_type": "text/markdown", "bytes": 6144 });
        let mut asked = plain();
        asked["question"] = serde_json::json!({ "id": "q1", "text": "Keep the old endpoint?", "options": [], "recommend": null, "kept_for_user": false, "plan": blob.clone(), "at": 1_761_069_600u64 });
        asked["card"] = serde_json::json!({ "sha": "9f3c2a1", "current": true, "blob": blob.clone() });
        let item: Item = serde_json::from_value(asked.clone()).unwrap();
        assert_eq!(item.question.as_ref().unwrap().plan_at, None);
        assert_eq!(serde_json::to_value(&item).unwrap(), asked);
        asked["question"]["plan_at"] = serde_json::json!(1_761_068_280u64);
        let item: Item = serde_json::from_value(asked.clone()).unwrap();
        assert_eq!(item.question.as_ref().unwrap().plan_at, Some(1_761_068_280));
        assert_eq!(serde_json::to_value(&item).unwrap(), asked);
        let mut leaked = asked.clone();
        leaked["card"]["blob"]["text"] = serde_json::json!("the card in clear");
        assert!(serde_json::from_value::<Item>(leaked).is_err(), "a blob ref never carries its content");
        asked["question"]["plan"]["at"] = serde_json::json!(1_761_068_280u64);
        assert!(serde_json::from_value::<Item>(asked).is_err(), "the plan's time sits on the question, not in the ref");
    }

    fn staged() -> serde_json::Value {
        let mut item = plain();
        item["subtasks"] = serde_json::json!([
            { "id": "both", "title": "Write to both columns", "state": "pushed", "commits": [sha('1')] },
            { "id": "backfill", "title": "Fill the old rows", "state": "pushed", "commits": [sha('2')] },
            { "id": "read-new", "title": "Read the new column", "state": "accepted", "commits": [sha('3')] },
            { "id": "drop-old", "title": "Drop the old column", "state": "accepted", "commits": [sha('4')] },
        ]);
        item["order"] = serde_json::json!([
            { "sha": sha('3'), "subject": "Read the email from the new column", "subtask": "read-new", "accepted": true, "stage": "switch" },
            { "sha": sha('4'), "subject": "Drop the old email column", "subtask": "drop-old", "accepted": true, "stage": "switch" },
        ]);
        item["stages"] = serde_json::json!([
            { "id": "both", "title": "Add the new column, write to both", "state": "met", "requirement": { "kind": "deploy" }, "through": sha('1'), "pushed_at": 1_761_062_000u64, "met_at": 1_761_062_600u64 },
            { "id": "backfill", "title": "Fill the old rows", "state": "waiting", "requirement": { "kind": "verified", "detail": "Has the backfill gone through in production?" }, "through": sha('2'), "pushed_at": 1_761_066_000u64 },
            { "id": "switch", "title": "Read the new column, drop the old", "state": "locked", "requirement": { "kind": "deploy" } },
        ]);
        item["kebbie_questions"] = serde_json::json!([{ "id": "verify-backfill", "kind": "verify", "text": "Stage 2 is out. Has the backfill gone through in production?", "stage": "backfill", "at": 1_761_066_600u64, "options": [
            { "label": "Verified", "meaning": "Stage 3 unlocks and can be pushed" },
            { "label": "Not yet", "meaning": "Stage 3 stays locked and this question waits" },
            { "label": "Something is wrong", "meaning": "Tell the task what you see; stage 3 stays locked", "comment": true },
        ] }]);
        item
    }

    fn verified() -> serde_json::Value {
        let mut item = with(staged(), &[("/stages/1/state", serde_json::json!("met")), ("/stages/2/state", serde_json::json!("open"))]);
        item["stages"][1]["met_at"] = serde_json::json!(1_761_067_000u64);
        item.as_object_mut().unwrap().remove("kebbie_questions");
        item["pushable"] = serde_json::json!({ "count": 2, "through": sha('4'), "subtasks": ["read-new", "drop-old"] });
        item
    }

    #[test]
    fn a_task_in_stages_pushes_nothing_past_a_stage_whose_requirement_before_it_is_not_met() {
        assert_eq!(checked(staged()), Ok(()));
        let item: Item = serde_json::from_value(staged()).unwrap();
        assert_eq!(serde_json::to_value(&item).unwrap(), staged());
        let stages = item.stages.as_deref().unwrap();
        assert_eq!(stages.iter().map(|stage| (stage.state, stage.requirement.kind)).collect::<Vec<_>>(), [(StageState::Met, RequirementKind::Deploy), (StageState::Waiting, RequirementKind::Verified), (StageState::Locked, RequirementKind::Deploy)]);
        assert_eq!(computed(&staged()), None, "both commits are accepted, but their stage waits on stage 2");
        let mut pushed = staged();
        pushed["pushable"] = serde_json::json!({ "count": 1, "through": sha('3'), "subtasks": ["read-new"] });
        assert_eq!(checked(pushed), Err("a pushable run stops where a stage before it is not met".into()));
        assert_eq!(checked(verified()), Ok(()));
        assert_eq!(computed(&verified()), Some((2, sha('4'))), "verifying stage 2 opens stage 3, and the run goes through it");
        let fix = {
            let mut item = verified();
            item["stages"][1]["state"] = serde_json::json!("waiting");
            item["stages"][1].as_object_mut().unwrap().remove("met_at");
            item["stages"][2]["state"] = serde_json::json!("locked");
            item["subtasks"].as_array_mut().unwrap().insert(0, serde_json::json!({ "id": "fix", "title": "Write both in one transaction", "state": "accepted", "commits": [sha('5')] }));
            item["order"].as_array_mut().unwrap().insert(0, serde_json::json!({ "sha": sha('5'), "subject": "Write both in one transaction", "subtask": "fix", "accepted": true, "stage": "both" }));
            item["pushable"] = serde_json::json!({ "count": 1, "through": sha('5'), "subtasks": ["fix"] });
            item
        };
        assert_eq!(checked(fix.clone()), Ok(()), "a fix in a met stage goes out while a later stage waits");
        assert_eq!(computed(&fix), Some((1, sha('5'))));
        let regressed = with(fix, &[("/stages/0/state", serde_json::json!("waiting")), ("/stages/1/state", serde_json::json!("met"))]);
        let mut regressed = regressed;
        regressed["stages"][0].as_object_mut().unwrap().remove("met_at");
        regressed["stages"][1]["met_at"] = serde_json::json!(1_761_067_000u64);
        assert_eq!(computed(&regressed), Some((1, sha('5'))), "stage 3 stays locked while stage 1 waits again, though stage 2 is met");
        assert_eq!(checked(regressed), Ok(()));
    }

    #[test]
    fn a_stage_says_where_its_requirement_stands_and_its_commits_sit_together_after_the_stages_before() {
        let wrong: Vec<(&str, serde_json::Value)> = vec![
            ("/stages/2/state", serde_json::json!("open")),
            ("/stages/2/state", serde_json::json!("met")),
            ("/stages/2/state", serde_json::json!("done")),
            ("/stages/1/state", serde_json::json!("open")),
            ("/stages/1/id", serde_json::json!("both")),
            ("/stages/1/id", serde_json::json!("../backfill")),
            ("/stages/1/title", serde_json::json!("Fill\nthe rows")),
            ("/stages/1/through", serde_json::json!("2222222")),
            ("/stages/0/requirement", serde_json::json!({ "kind": "deploy", "detail": "CI is green" })),
            ("/stages/1/requirement", serde_json::json!({ "kind": "verified" })),
            ("/stages/1/requirement", serde_json::json!({ "kind": "check", "detail": " " })),
            ("/stages/1/requirement", serde_json::json!({ "kind": "manual", "detail": "Look" })),
            ("/stages/1/note", serde_json::json!("Two\nlines")),
            ("/order/0/stage", serde_json::json!("nope")),
            ("/order/1/stage", serde_json::json!("both")),
            ("/order/0/stage", serde_json::Value::Null),
            ("/kebbie_questions/0/stage", serde_json::json!("both")),
            ("/kebbie_questions/0/stage", serde_json::json!("nope")),
            ("/kebbie_questions/0/kind", serde_json::json!("close")),
            ("/kebbie_questions/0/options/0/label", serde_json::json!("Yes")),
            ("/kebbie_questions/0/options/2/comment", serde_json::json!(false)),
            ("/kebbie_questions/0/options/1/meaning", serde_json::json!(" ")),
            ("/kebbie_questions/0/text", serde_json::json!("a\u{7}b")),
            ("/kebbie_questions/0/id", serde_json::json!("a b")),
        ];
        for (pointer, value) in wrong {
            let mut item = staged();
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            item.pointer_mut(parent).unwrap()[key] = value.clone();
            assert!(checked(item).is_err(), "{pointer} = {value}");
        }
        for (key, stage) in [("pushed_at", 0), ("met_at", 0), ("through", 1)] {
            let mut item = staged();
            item["stages"][stage].as_object_mut().unwrap().remove(key);
            assert!(checked(item).is_err(), "a stage without {key}");
        }
        let mut early = staged();
        early["stages"][2]["met_at"] = serde_json::json!(1);
        assert_eq!(checked(early), Err("a stage says when it was met, and only a met one".into()));
        let mut split = staged();
        split["subtasks"][2]["commits"] = serde_json::json!([sha('3'), sha('4')]);
        split["subtasks"].as_array_mut().unwrap().remove(3);
        split["order"][1]["subtask"] = serde_json::json!("read-new");
        assert_eq!(checked(split.clone()), Ok(()));
        split["stages"][1]["state"] = serde_json::json!("met");
        split["stages"][1]["met_at"] = serde_json::json!(1);
        split["stages"][2]["state"] = serde_json::json!("open");
        split.as_object_mut().unwrap().remove("kebbie_questions");
        split["subtasks"].as_array_mut().unwrap().push(serde_json::json!({ "id": "fix", "title": "Fix", "state": "accepted", "commits": [sha('5')] }));
        split["order"].as_array_mut().unwrap().push(serde_json::json!({ "sha": sha('5'), "subject": "Fix", "subtask": "fix", "accepted": true, "stage": "both" }));
        assert_eq!(checked(split.clone()), Err("a stage's commits sit after the stages before it".into()));
        let moved = split["order"].as_array_mut().unwrap().pop().unwrap();
        split["order"].as_array_mut().unwrap().insert(0, moved);
        split["order"][0]["subtask"] = serde_json::json!("read-new");
        split["subtasks"][2]["commits"] = serde_json::json!([sha('5'), sha('3'), sha('4')]);
        split["subtasks"].as_array_mut().unwrap().pop();
        assert_eq!(checked(split), Err("a subtask's commits sit in one stage".into()));
        let grouped = row(&row(&verified(), 0, "group", serde_json::json!("swap")), 1, "group", serde_json::json!("swap"));
        assert_eq!(checked(grouped.clone()), Ok(()));
        let mut across = grouped;
        across["subtasks"].as_array_mut().unwrap().insert(0, serde_json::json!({ "id": "fix", "title": "Fix", "state": "accepted", "commits": [sha('5')] }));
        across["order"].as_array_mut().unwrap().insert(0, serde_json::json!({ "sha": sha('5'), "subject": "Fix", "subtask": "fix", "accepted": true, "stage": "backfill", "group": "swap" }));
        across["order"][2].as_object_mut().unwrap().remove("group");
        across["pushable"] = serde_json::json!({ "count": 2, "through": sha('3'), "subtasks": ["fix", "read-new"] });
        assert_eq!(checked(across), Err("a group's commits sit in one stage".into()));
        let mut unstaged = planned();
        unstaged["order"][0]["stage"] = serde_json::json!("both");
        assert_eq!(checked(unstaged), Err("a commit names a stage the task does not have".into()));
        let mut bare = staged();
        bare.as_object_mut().unwrap().remove("subtasks");
        bare.as_object_mut().unwrap().remove("order");
        assert_eq!(checked(bare), Err("stages belong to a task's subtasks".into()));
        let mut none = staged();
        none["stages"] = serde_json::json!([]);
        none["order"] = serde_json::json!([]);
        none.as_object_mut().unwrap().remove("kebbie_questions");
        assert_eq!(checked(none), Err("a task holds one to sixteen stages".into()));
        let mut stepped = plain();
        stepped["steps"] = serde_json::json!({ "pushed": 0, "items": [] });
        stepped["stages"] = staged()["stages"].clone();
        assert!(serde_json::from_value::<Item>(stepped).is_err(), "stages never ride with steps from an older machine");
    }

    #[test]
    fn unlocking_opens_each_stage_not_yet_pushed_whose_stages_before_it_are_met() {
        let mut item: Item = serde_json::from_value(staged()).unwrap();
        let stages = item.stages.as_mut().unwrap();
        stages[1].state = StageState::Met;
        stages[1].met_at = Some(1);
        unlock_stages(stages);
        assert_eq!(stages[2].state, StageState::Open);
        stages[0].state = StageState::Failed;
        stages[0].met_at = None;
        unlock_stages(stages);
        assert_eq!(stages.iter().map(|stage| stage.state).collect::<Vec<_>>(), [StageState::Failed, StageState::Met, StageState::Locked], "a pushed stage keeps its state, and one not pushed locks behind a stage that failed");
    }

    #[test]
    fn kebbie_asks_beside_the_task_with_its_kinds_fixed_answers() {
        let mut both = staged();
        both["question"] = serde_json::json!({ "id": "q1", "text": "Keep the old endpoint?", "options": [], "recommend": null, "kept_for_user": false, "plan": null, "at": 1_761_069_600u64 });
        assert_eq!(checked(both.clone()), Ok(()), "a verification never holds the task's own question back");
        both["question"]["id"] = serde_json::json!("verify-backfill");
        assert_eq!(checked(both), Err("a question is asked once".into()));
        let close = serde_json::json!({ "id": "close-docs", "kind": "close", "text": "docs says it is done: the README names every flag. Close it?", "at": 1_761_069_000u64, "options": [
            { "label": "Close", "meaning": "The task closes and its worktree is freed" },
            { "label": "Not yet", "meaning": "The task stays open" },
        ] });
        let manual = serde_json::json!({ "id": "check-mail", "kind": "verify", "text": "Does the signup mail arrive at a Gmail address?", "at": 1_761_069_000u64, "options": staged()["kebbie_questions"][0]["options"].clone() });
        let mut asked = plain();
        asked["kebbie_questions"] = serde_json::json!([close.clone(), manual.clone()]);
        assert_eq!(checked(asked.clone()), Ok(()), "a manual check and a close need no stages");
        let item: Item = serde_json::from_value(asked.clone()).unwrap();
        assert_eq!(item.kebbie_questions.as_ref().unwrap().iter().map(|question| question.kind).collect::<Vec<_>>(), [KebbieQuestionKind::Close, KebbieQuestionKind::Verify]);
        assert_eq!(serde_json::to_value(&item).unwrap(), asked);
        let mut staged_close = asked.clone();
        staged_close["kebbie_questions"][0]["stage"] = serde_json::json!("both");
        assert_eq!(checked(staged_close), Err("only a verification names a stage".into()));
        let mut verify_answers = asked.clone();
        verify_answers["kebbie_questions"][0]["options"] = manual["options"].clone();
        assert_eq!(checked(verify_answers), Err("kebbie's question offers its kind's fixed answers".into()));
        let mut five = plain();
        five["kebbie_questions"] = serde_json::json!((0..=MAX_KEBBIE_QUESTIONS).map(|n| { let mut one = manual.clone(); one["id"] = serde_json::json!(format!("check-{n}")); one }).collect::<Vec<_>>());
        assert_eq!(checked(five), Err("a task holds four of kebbie's questions at most".into()));
        let mut extra = asked;
        extra["kebbie_questions"][0]["recommend"] = serde_json::json!(0);
        assert!(serde_json::from_value::<Item>(extra).is_err(), "kebbie recommends nothing");
    }

    fn plain_board() -> serde_json::Value {
        serde_json::json!({ "rev": 3, "kebbie": "0.24.0", "away_since": null, "checkouts": [], "routines": [], "items": [plain()], "sessions_outside_tasks": 0 })
    }

    #[test]
    fn a_board_from_a_kebbie_without_next_parses_and_is_written_back_unchanged() {
        let board: Board = serde_json::from_value(plain_board()).unwrap();
        assert_eq!((board.next.as_ref(), board.counts.as_ref(), board.later.as_ref(), board.accepts.as_ref(), board.devices.as_ref(), board.backlog.as_ref()), (None, None, None, None, None, None));
        assert_eq!((board.items[0].title.as_ref(), board.items[0].second_opinion.as_ref()), (None, None));
        assert_eq!(serde_json::to_value(&board).unwrap(), plain_board());
    }

    #[test]
    fn a_board_carries_next_counts_and_the_tasks_set_aside_and_an_item_its_agent_profile_recipe_and_reviews() {
        let mut full = plain_board();
        full["next"] = serde_json::json!([
            { "part": "stopped", "address": "hub@kebbie", "rank": 1, "line": "answer Codex's prompt", "since": 1_761_069_000u64, "waits_behind": 0 },
            { "part": "push", "address": "payments@api", "rank": 2, "line": "2 commits · 1 waits on it", "since": null, "waits_behind": 1 },
        ]);
        full["counts"] = serde_json::json!({ "working": 4, "queued": 1, "parked": 1 });
        full["accepts"] = serde_json::json!(["answer", "push", "prompt-answer"]);
        full["later"] = serde_json::json!([
            { "address": "mail@shop", "title": "Mail DNS", "state": "queued", "line": "waits for go-live", "until": null, "question": false },
            { "address": "editor@app", "title": null, "state": "parked", "line": "nothing yet", "until": 1_761_100_000u64, "question": true },
        ]);
        let item = &mut full["items"][0];
        item["title"] = serde_json::json!("Charge through the gateway");
        item["profile"] = serde_json::json!("task");
        item["recipe"] = serde_json::json!("small fix");
        item["assessment"] = serde_json::json!("review suggested: touches money");
        item["agent"] = serde_json::json!({ "status": "blocked", "kind": "codex", "model": "gpt-6.1" });
        item["verdict"] = serde_json::json!({ "verdict": "ok", "findings": 0, "sha": "abc", "stale": false, "running": false, "held": true });
        item["second_opinion"] = serde_json::json!({ "by": "codex-review", "points": 2, "running": false });
        item["subtasks"] = serde_json::json!([]);
        item["order"] = serde_json::json!([]);
        item["require_accept"] = serde_json::json!(false);
        item["prompt"] = serde_json::json!({ "id": "3f2a9c1d", "question": "Run it?", "options": [{ "label": "Yes, proceed (y)", "takes_text": false, "key": "y" }, { "label": "Type something", "takes_text": true }] });
        let board: Board = serde_json::from_value(full.clone()).unwrap();
        assert_eq!(board.next.as_ref().unwrap().iter().map(|row| (row.part, row.rank)).collect::<Vec<_>>(), [(NextPart::Stopped, 1), (NextPart::Push, 2)]);
        assert_eq!(board.later.as_ref().unwrap()[1].state, LaterState::Parked);
        assert_eq!(board.items[0].verdict.as_ref().unwrap().held, Some(true));
        assert_eq!(board.items[0].prompt.as_ref().unwrap().options[0].key.as_deref(), Some("y"));
        assert_eq!(board.accepts.as_deref(), Some([Kind::Answer, Kind::Push, Kind::PromptAnswer].as_slice()));
        assert_eq!(serde_json::to_value(&board).unwrap(), full);
        for (path, value) in [("/next/0/part", serde_json::json!("later")), ("/later/0/state", serde_json::json!("working")), ("/later/0/path", serde_json::json!("/Users/x"))] {
            let mut wrong = full.clone();
            let (parent, key) = path.rsplit_once('/').unwrap();
            wrong.pointer_mut(parent).unwrap()[key] = value;
            assert!(serde_json::from_value::<Board>(wrong).is_err(), "{path}");
        }
    }

    #[test]
    fn an_assessment_is_a_word_with_why_or_one_line_from_a_machine_not_yet_updated() {
        let assessed = |assessment: serde_json::Value| {
            let mut board = plain_board();
            board["items"] = serde_json::json!([plain()]);
            board["items"][0]["assessment"] = assessment;
            serde_json::from_value::<Board>(board).map_err(|_| "unreadable").and_then(|board| board.check().map(|()| board.items[0].assessment.clone().unwrap()))
        };
        assert_eq!(assessed(serde_json::json!("review suggested: touches money")), Ok(Assessment::Said("review suggested: touches money".into())));
        assert_eq!(
            assessed(serde_json::json!({ "word": "recommended", "reason": "Changes how refunds round", "at": 1_761_069_600u64 })),
            Ok(Assessment::Judged(Judgement { word: AssessmentWord::Recommended, reason: Some("Changes how refunds round".into()), at: Some(1_761_069_600) }))
        );
        assert!(assessed(serde_json::json!({ "word": "assessing" })).is_ok());
        assert!(assessed(serde_json::json!({ "word": "not-needed" })).is_ok());
        assert!(assessed(serde_json::json!({ "word": "recommended" })).is_err(), "a recommendation says why");
        assert!(assessed(serde_json::json!({ "word": "assessing", "reason": "x" })).is_err());
        assert!(assessed(serde_json::json!({ "word": "running" })).is_err(), "a running review is the verdict's to say");
        assert!(assessed(serde_json::json!({ "word": "recommended", "reason": "a\nb" })).is_err());
    }

    #[test]
    fn a_board_says_where_each_device_stands_on_its_machine() {
        let phone = Id::ulid(Prefix::Device, 1, [1; 10]).unwrap();
        let tablet = Id::ulid(Prefix::Device, 1, [2; 10]).unwrap();
        let laptop = Id::ulid(Prefix::Device, 1, [3; 10]).unwrap();
        let mut full = plain_board();
        full["devices"] = serde_json::json!({
            phone.as_str(): { "state": "approved", "fingerprint": "7F3A-91C2", "at": 1_761_069_000u64 },
            tablet.as_str(): { "state": "waiting", "fingerprint": "0C11-5E9A", "until": null },
            laptop.as_str(): { "state": "denied", "fingerprint": "B204-77D1", "at": 1_761_069_100u64 },
        });
        let board: Board = serde_json::from_value(full.clone()).unwrap();
        let devices = board.devices.as_ref().unwrap();
        assert_eq!(devices[&tablet], DeviceStanding::Waiting { fingerprint: "0C11-5E9A".into(), until: None });
        assert_eq!(serde_json::to_value(&board).unwrap(), full);
        let account = Id::account([1; 16]);
        for (key, value) in [
            (phone.as_str(), serde_json::json!({ "state": "approved", "fingerprint": "7F3A-91C2", "at": 1, "until": 2 })),
            (phone.as_str(), serde_json::json!({ "state": "trusted", "fingerprint": "7F3A-91C2", "at": 1 })),
            (phone.as_str(), serde_json::json!({ "state": "denied", "fingerprint": "7F3A-91C2" })),
            (account.as_str(), serde_json::json!({ "state": "approved", "fingerprint": "7F3A-91C2", "at": 1 })),
        ] {
            let mut wrong = plain_board();
            wrong["devices"] = serde_json::json!({ key: value });
            assert!(serde_json::from_value::<Board>(wrong).is_err(), "{key}: {value}");
        }
    }
}
