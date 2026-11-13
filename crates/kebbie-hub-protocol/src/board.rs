use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::backlog::BacklogRow;
use crate::envelope::Kind;
use crate::ids::{Id, Prefix};
use crate::message::{check, is_line};
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
#[serde(deny_unknown_fields)]
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
    pub steps: Option<Steps>,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecondOpinion {
    pub by: String,
    pub points: Option<u32>,
    pub running: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Steps {
    pub pushed: u32,
    pub items: Vec<Step>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub number: u32,
    pub title: String,
    pub state: StepState,
    pub sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Rules>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepState {
    Planned,
    InProgress,
    Committed,
    Accepted,
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
            item.steps.iter().flat_map(|steps| &steps.items).filter_map(|step| step.rules.as_ref()).try_for_each(Rules::check)
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
    fn an_item_from_a_kebbie_without_steps_parses_and_is_written_back_without_the_field() {
        let item: Item = serde_json::from_value(plain()).unwrap();
        assert_eq!(item.steps, None);
        assert_eq!(serde_json::to_value(&item).unwrap(), plain());
    }

    #[test]
    fn an_item_carries_its_steps_in_order_with_the_sha_of_each_commit() {
        let mut stepped = plain();
        stepped["steps"] = serde_json::json!({ "pushed": 1, "items": [
            { "number": 2, "title": "Charge through a PaymentGateway", "state": "committed", "sha": "2222222bbbb" },
            { "number": 3, "title": "Add the new gateway", "state": "in-progress", "sha": null },
            { "number": 4, "title": "Remove the old payment client", "state": "planned", "sha": null },
            { "number": 5, "title": "Docs", "state": "accepted", "sha": "5555555eeee" },
        ] });
        let item: Item = serde_json::from_value(stepped.clone()).unwrap();
        let steps = item.steps.as_ref().unwrap();
        assert_eq!(steps.pushed, 1);
        assert_eq!(steps.items.iter().map(|step| (step.number, step.state)).collect::<Vec<_>>(), [(2, StepState::Committed), (3, StepState::InProgress), (4, StepState::Planned), (5, StepState::Accepted)]);
        assert_eq!(serde_json::to_value(&item).unwrap(), stepped);
        let mut subject = stepped.clone();
        subject["steps"]["items"][0]["subject"] = serde_json::json!("Charge through a PaymentGateway");
        assert!(serde_json::from_value::<Item>(subject).is_err(), "a step carries only its allow-listed fields");
        let mut unknown = stepped;
        unknown["steps"]["items"][0]["state"] = serde_json::json!("pushed");
        assert!(serde_json::from_value::<Item>(unknown).is_err());
    }

    #[test]
    fn a_task_and_each_step_carry_where_the_rules_check_stands_and_an_approved_flag_is_ignored() {
        let mut ruled = plain();
        ruled["git"] = serde_json::json!({ "upstream": "main", "ahead": 1, "behind": 0, "uncommitted": 0, "rules": { "state": "breaks", "breaks": 2, "names": ["Every commit says why"], "override": { "by": "lead" } } });
        ruled["steps"] = serde_json::json!({ "pushed": 0, "items": [{ "number": 1, "title": "Docs", "state": "committed", "sha": "1111111aaaa", "rules": { "state": "checking" } }] });
        let item: Item = serde_json::from_value(ruled.clone()).unwrap();
        let rules = item.git.as_ref().unwrap().rules.as_ref().unwrap();
        assert_eq!((rules.state, rules.breaks, rules.overridden.as_ref().map(|by| by.by)), (RulesState::Breaks, Some(2), Some(OverrideBy::Lead)));
        assert_eq!(item.steps.as_ref().unwrap().items[0].rules.as_ref().unwrap().state, RulesState::Checking);
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
        item["steps"] = serde_json::json!({ "pushed": 0, "items": [], "required": false });
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
