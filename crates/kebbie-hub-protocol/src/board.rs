use serde::{Deserialize, Serialize};

use crate::envelope::Kind;
use crate::ids::Id;

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
#[serde(deny_unknown_fields)]
pub struct Git {
    pub upstream: String,
    pub ahead: u32,
    pub behind: u32,
    pub uncommitted: u32,
    pub approved: bool,
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
        assert_eq!((board.next.as_ref(), board.counts.as_ref(), board.later.as_ref(), board.accepts.as_ref()), (None, None, None, None));
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
}
