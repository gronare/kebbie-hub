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
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assessment: Option<String>,
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
    order: Option<Vec<OrderRow>>,
    #[serde(default)]
    pushable: Option<Pushable>,
    #[serde(default)]
    require_accept: Option<bool>,
    #[serde(default)]
    obstacle: Option<Obstacle>,
    #[serde(default)]
    steps: Option<Steps>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    profile: Option<String>,
    #[serde(default)]
    recipe: Option<String>,
    #[serde(default)]
    assessment: Option<String>,
    #[serde(default)]
    second_opinion: Option<SecondOpinion>,
    #[serde(default)]
    review: Option<ReviewOffer>,
}

impl TryFrom<ItemWire> for Item {
    type Error = &'static str;

    fn try_from(wire: ItemWire) -> Result<Self, Self::Error> {
        let (subtasks, order, pushable, require_accept) = match wire.steps {
            None => (wire.subtasks, wire.order, wire.pushable, wire.require_accept),
            Some(_) if wire.subtasks.is_some() || wire.order.is_some() || wire.pushable.is_some() || wire.require_accept.is_some() => return Err("an item carries subtasks or, from an older machine, steps"),
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

pub const MAX_SUBTASK_ID_CHARS: usize = 32;
pub const MAX_ADDED_REASON_CHARS: usize = 500;
pub const MAX_OBSTACLE_CHARS: usize = 2_000;

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
        for row in order {
            check(is_full_sha(&row.sha), "a commit in the push order is not a full sha")?;
            check(placed.insert(row.sha.as_str()), "a commit sits once in the push order")?;
            let owner = self.subtask(&row.subtask).ok_or("a commit in the push order names a subtask the task does not have")?;
            check(owner.commits.contains(&row.sha), "a commit in the push order is not one of its subtask's")?;
            check(owner.state != SubtaskState::Pushed, "a pushed subtask has nothing left in the push order")?;
        }
        let Some(pushable) = &self.pushable else {
            return Ok(());
        };
        let count = pushable.count as usize;
        check((1..=order.len()).contains(&count), "a pushable run holds one to all of the push order")?;
        check(order[count - 1].sha == pushable.through, "a pushable run ends at its last commit")?;
        let mut run: Vec<&str> = Vec::new();
        for row in &order[..count] {
            if !run.contains(&row.subtask.as_str()) {
                run.push(&row.subtask);
            }
        }
        check(run == pushable.subtasks.iter().map(String::as_str).collect::<Vec<_>>(), "a pushable run names the subtasks of its commits in order")?;
        check(run.iter().all(|id| self.subtask(id).is_some_and(|subtask| subtask.state == SubtaskState::Accepted)), "every commit in a pushable run belongs to an accepted subtask")
    }
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
            commits(step).into_iter().map(|(sha, subject)| OrderRow { sha, subject, subtask: step.number.to_string() })
        }).collect();
        (subtasks.clone(), order.clone(), pushable_of(&subtasks, &order))
    }
}

fn pushable_of(subtasks: &[Subtask], order: &[OrderRow]) -> Option<Pushable> {
    let accepted = |row: &OrderRow| subtasks.iter().any(|subtask| subtask.id == row.subtask && subtask.state == SubtaskState::Accepted);
    let count = order.iter().take_while(|row| accepted(row)).count();
    let last = order[..count].last()?;
    let mut run: Vec<String> = Vec::new();
    for row in &order[..count] {
        if !run.contains(&row.subtask) {
            run.push(row.subtask.clone());
        }
    }
    Some(Pushable { count: count as u32, through: last.sha.clone(), subtasks: run })
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
            item.review.as_ref().map_or(Ok(()), |offer| check(offer.subtask.as_deref().is_none_or(is_subtask_id), "a review offer names a subtask by its id"))?;
            item.check_plan()
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
            { "sha": sha('1'), "subject": "Give the tests their own vault", "subtask": "env" },
            { "sha": sha('2'), "subject": "Clean the vault between tests", "subtask": "env" },
            { "sha": sha('3'), "subject": "Keep frontmatter in the index", "subtask": "fm" },
            { "sha": sha('4'), "subject": "Read lint's fields from the index", "subtask": "lint" },
            { "sha": sha('5'), "subject": "Run CI on Python 3.14", "subtask": "ci" },
            { "sha": sha('6'), "subject": "Drop lint's fetch of every note", "subtask": "lint" },
            { "sha": sha('7'), "subject": "Move the backend factory", "subtask": "split" },
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
        later["pushable"] = serde_json::json!({ "count": 6, "through": sha('6'), "subtasks": ["env", "fm", "lint", "ci"] });
        assert_eq!(checked(later), Ok(()), "accepting lint frees ci, which waited behind it");
    }

    #[test]
    fn a_push_order_and_its_run_hold_together_with_the_subtasks() {
        let wrong: [(&str, serde_json::Value); 20] = [
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
        extra["order"][0]["group"] = serde_json::json!("g1");
        assert!(checked(extra).is_err(), "a row carries only its allow-listed fields");
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
        assert_eq!(item.order.unwrap(), vec![OrderRow { sha: sha('2'), subject: "Charge through a PaymentGateway".into(), subtask: "1".into() }]);
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
        ruled["order"] = serde_json::json!([{ "sha": sha('1'), "subject": "Docs", "subtask": "docs" }]);
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
