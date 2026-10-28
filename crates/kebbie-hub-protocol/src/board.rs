use serde::{Deserialize, Serialize};

use crate::ids::Id;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    pub rev: u64,
    // Machines and phones paired before the rename send and read this field under the old name.
    #[serde(rename = "kelpie")]
    pub kebbie: String,
    pub away_since: Option<u64>,
    pub checkouts: Vec<Checkout>,
    pub routines: Vec<RoutineSummary>,
    pub items: Vec<Item>,
    pub sessions_outside_tasks: u32,
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Steps {
    pub pushed: u32,
    pub items: Vec<Step>,
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
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionOption {
    pub label: String,
    pub meaning: String,
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
            "address": "refunds@greenhouse", "group": "WORKING", "urgency": 0, "summary": "1 commit", "next_for_you": null,
            "agent": null, "report": null, "question": null, "prompt": null, "git": null, "head": null, "verdict": null,
            "push_permission": null, "push_waits": null, "follows_main": false, "ci": null, "recover": null, "routine": null, "card": null,
        });
        assert!(serde_json::from_value::<Item>(item.clone()).is_ok());
        let mut leaked = item;
        leaked["path"] = serde_json::json!("/Users/someone/projects/greenhouse");
        assert!(serde_json::from_value::<Item>(leaked).is_err());
    }

    fn plain() -> serde_json::Value {
        serde_json::json!({
            "address": "stripe@billing", "group": "WORKING", "urgency": 0, "summary": "2 commits", "next_for_you": null,
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
            { "number": 3, "title": "Add Stripe as the gateway", "state": "in-progress", "sha": null },
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
}
